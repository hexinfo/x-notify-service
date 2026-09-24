//! Windows 10/11 无边框弹窗的原生圆角裁切。

#![allow(unsafe_code)]

use std::ptr;

use windows_sys::Win32::Foundation::RECT;
use windows_sys::Win32::Graphics::Gdi::{CreateRoundRectRgn, DeleteObject, SetWindowRgn};
use windows_sys::Win32::System::Threading::GetCurrentProcessId;
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    FindWindowW, GetWindowRect, GetWindowThreadProcessId,
};

use super::popup;

pub fn apply() {
    let title: Vec<u16> = popup::WINDOW_TITLE.encode_utf16().chain([0]).collect();
    // SAFETY: 传入以 NUL 结尾的窗口标题;返回的句柄仅在以下同步调用中使用。
    let hwnd = unsafe { FindWindowW(ptr::null(), title.as_ptr()) };
    if hwnd.is_null() {
        return;
    }

    let mut owner = 0;
    // SAFETY: hwnd 由 FindWindowW 返回,owner 为有效输出指针。
    unsafe {
        GetWindowThreadProcessId(hwnd, &raw mut owner);
    }
    // SAFETY: 无参数 Win32 查询;确保不会裁切同名的其他进程窗口。
    if owner != unsafe { GetCurrentProcessId() } {
        return;
    }

    let mut rect = RECT::default();
    // SAFETY: hwnd 属于本进程,rect 是有效可写指针。
    if unsafe { GetWindowRect(hwnd, &raw mut rect) } == 0 {
        return;
    }
    let width = rect.right - rect.left;
    let height = rect.bottom - rect.top;
    if width <= 0 || height <= 0 {
        return;
    }

    // SAFETY: hwnd 属于本进程;0 DPI 由 corner_diameter 回退为 96。
    let dpi = unsafe { GetDpiForWindow(hwnd) };
    let diameter = popup::corner_diameter(dpi, width, height);
    // SAFETY: 矩形尺寸和圆角直径为正且直径不超过窗口短边。
    let region = unsafe { CreateRoundRectRgn(0, 0, width, height, diameter, diameter) };
    if region.is_null() {
        log::warn!("创建 Windows 弹窗圆角区域失败");
        return;
    }
    // SAFETY: hwnd 属于本进程;成功时系统接管 region 所有权,失败时由本函数释放。
    if unsafe { SetWindowRgn(hwnd, region, 1) } == 0 {
        // SAFETY: SetWindowRgn 失败时 region 仍由调用方持有。
        unsafe {
            DeleteObject(region);
        }
        log::warn!("设置 Windows 弹窗圆角区域失败");
    }
}
