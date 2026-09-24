//! Windows 10/11 无边框弹窗的原生圆角裁切。

#![allow(unsafe_code)]

use std::ptr;

use windows_sys::Win32::Foundation::RECT;
use windows_sys::Win32::Graphics::Gdi::{CreateRoundRectRgn, DeleteObject, SetWindowRgn};
use windows_sys::Win32::System::Threading::{GetCurrentProcessId, GetCurrentThreadId};
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    FindWindowW, GWL_STYLE, GetWindowLongW, GetWindowRect, GetWindowThreadProcessId,
    SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SetWindowLongW,
    SetWindowPos, WM_NCACTIVATE, WM_NCDESTROY, WM_NCPAINT, WS_CAPTION, WS_SYSMENU,
};

use super::popup;

const SUBCLASS_ID: usize = 1;

unsafe extern "system" fn popup_subclass(
    hwnd: windows_sys::Win32::Foundation::HWND,
    message: u32,
    wparam: windows_sys::Win32::Foundation::WPARAM,
    lparam: windows_sys::Win32::Foundation::LPARAM,
    _: usize,
    _: usize,
) -> windows_sys::Win32::Foundation::LRESULT {
    match message {
        // 仍交给 winit 处理激活状态,但不让 DefWindowProc 重绘原生标题栏。
        // SAFETY: hwnd/message/参数来自当前窗口的 Win32 回调。
        WM_NCACTIVATE => unsafe { DefSubclassProc(hwnd, message, wparam, -1) },
        WM_NCPAINT => 0,
        WM_NCDESTROY => {
            // SAFETY: 在窗口销毁回调中移除当前窗口的当前子类过程。
            unsafe {
                RemoveWindowSubclass(hwnd, Some(popup_subclass), SUBCLASS_ID);
            }
            // SAFETY: 将原始销毁消息交给下一层窗口过程。
            unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
        }
        // SAFETY: 未拦截的消息与原参数交给下一层窗口过程。
        _ => unsafe { DefSubclassProc(hwnd, message, wparam, lparam) },
    }
}

pub fn apply() {
    let title: Vec<u16> = popup::WINDOW_TITLE.encode_utf16().chain([0]).collect();
    // SAFETY: 传入以 NUL 结尾的窗口标题;返回的句柄仅在以下同步调用中使用。
    let hwnd = unsafe { FindWindowW(ptr::null(), title.as_ptr()) };
    if hwnd.is_null() {
        return;
    }

    let mut owner = 0;
    // SAFETY: hwnd 由 FindWindowW 返回,owner 为有效输出指针。
    let thread = unsafe { GetWindowThreadProcessId(hwnd, &raw mut owner) };
    // SAFETY: 无参数 Win32 查询;确保不会裁切同名的其他进程窗口。
    if owner != unsafe { GetCurrentProcessId() } {
        return;
    }

    // SetWindowSubclass 必须由创建窗口的线程调用;apply 由 iced 窗口事件触发。
    // SAFETY: 无参数 Win32 当前线程查询。
    if thread != unsafe { GetCurrentThreadId() } {
        log::warn!("无法在其他线程修正 Windows 弹窗标题栏");
        return;
    }
    // SAFETY: hwnd 属于当前线程;相同回调/id 重复注册是幂等的,销毁时主动移除。
    if unsafe { SetWindowSubclass(hwnd, Some(popup_subclass), SUBCLASS_ID, 0) } == 0 {
        log::warn!("禁止 Windows 弹窗非客户区重绘失败");
        return;
    }

    // winit 的无框窗口仍保留 WS_CAPTION,仅在 WM_NCCALCSIZE 中隐藏它。
    // SetWindowRgn 后 Windows 失焦重绘非客户区时会露出原生标题栏。
    // 真正去掉标题栏样式,避免激活状态改变时再次绘制。
    // SAFETY: hwnd 已校验属于本进程;读取/修改的是该窗口的样式。
    let style = unsafe { GetWindowLongW(hwnd, GWL_STYLE) };
    let frameless = style & (!(WS_CAPTION | WS_SYSMENU)).cast_signed();
    if style != frameless {
        // SAFETY: hwnd 属于本进程,仅清除该窗口的标题栏样式。
        unsafe {
            SetWindowLongW(hwnd, GWL_STYLE, frameless);
        }
        // SAFETY: hwnd 属于本进程;SWP_FRAMECHANGED 刷新非客户区且不改变位置、尺寸和层级。
        if unsafe {
            SetWindowPos(
                hwnd,
                ptr::null_mut(),
                0,
                0,
                0,
                0,
                SWP_FRAMECHANGED | SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER,
            )
        } == 0
        {
            log::warn!("刷新 Windows 弹窗无边框样式失败");
        }
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
