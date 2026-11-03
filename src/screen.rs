/// 屏幕主显示器「工作区」(排除任务栏/dock/菜单栏),原点左上。
/// 所有坐标已统一换算为物理像素;scale 为物理→逻辑换算系数,
/// Windows 为系统 DPI/96(缩放屏非 1.0),Linux 为 1.0,macOS 为 backingScaleFactor。
#[derive(Debug, Clone, Copy)]
pub struct WorkArea {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub scale: f64,
}

#[cfg(windows)]
// 取屏 FFI 是 unsafe 的唯一入口;SystemParametersInfoW 读工作区,
// GetDeviceCaps 读系统 DPI(Win7 即有,GetDpiForSystem 为 Win8+ 不可用)
#[allow(unsafe_code)]
pub fn work_area() -> Option<WorkArea> {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::Graphics::Gdi::{GetDC, GetDeviceCaps, LOGPIXELSX, ReleaseDC};
    use windows_sys::Win32::UI::WindowsAndMessaging::{SPI_GETWORKAREA, SystemParametersInfoW};

    let mut rect = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    // SAFETY: 入参为常量标志与指向 RECT 的可写指针,函数无其他副作用
    let ok = unsafe {
        SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            std::ptr::from_mut(&mut rect).cast::<core::ffi::c_void>(),
            0,
        )
    };
    if ok == 0 {
        return None;
    }
    // 系统 DPI → 缩放系数:落点/尺寸换算逻辑坐标全靠它。此前写死 1.0,
    // 缩放屏(125%/150%)上物理落点被 winit 当逻辑坐标再乘一遍缩放后
    // 飞出屏幕,表现为「通知已投递但看不见弹窗」(浙商版 2560x1440 终端实测)。
    // 此时 winit 已置进程 DPI aware,SPI_GETWORKAREA/GetDeviceCaps 返回真实值。
    let scale = {
        // SAFETY: 取屏 DC 仅做同步 DPI 查询后配对释放
        let hdc = unsafe { GetDC(std::ptr::null_mut()) };
        if hdc.is_null() {
            1.0
        } else {
            // SAFETY: hdc 有效;LOGPIXELSX 常量索引只读
            let dpi = unsafe { GetDeviceCaps(hdc, LOGPIXELSX.cast_signed()).max(96) };
            // SAFETY: 与 GetDC 配对释放
            unsafe {
                ReleaseDC(std::ptr::null_mut(), hdc);
            }
            f64::from(dpi) / 96.0
        }
    };
    Some(WorkArea {
        x: f64::from(rect.left),
        y: f64::from(rect.top),
        w: f64::from(rect.right - rect.left),
        h: f64::from(rect.bottom - rect.top),
        scale,
    })
}

#[cfg(target_os = "macos")]
pub fn work_area() -> Option<WorkArea> {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSScreen;

    // 只能在主线程(GUI 事件循环线程)查询;坐标原点在左下,换算为左上原点物理像素
    let mtm = MainThreadMarker::new()?;
    let screen = NSScreen::mainScreen(mtm)?;
    let full = screen.frame();
    let vis = screen.visibleFrame();
    let scale = screen.backingScaleFactor();
    let y_top_pts = (full.origin.y + full.size.height) - (vis.origin.y + vis.size.height);
    Some(WorkArea {
        x: vis.origin.x * scale,
        y: y_top_pts * scale,
        w: vis.size.width * scale,
        h: vis.size.height * scale,
        scale,
    })
}

#[cfg(target_os = "linux")]
pub fn work_area() -> Option<WorkArea> {
    use x11rb::connection::Connection as _;
    use x11rb::protocol::xproto::ConnectionExt as _;

    // Wayland(含 XWayland:DISPLAY/WAYLAND_DISPLAY 并存):协议禁止客户端自定位,
    // 弹窗会被合成器随手摆放(常为左上角),返回 None 走系统通知兜底
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        log::info!("Wayland/XWayland 会话,弹窗自定位不可用,通知走系统通道");
        return None;
    }
    let (conn, screen_num) = x11rb::connect(None).ok()?;
    let screen = conn.setup().roots.get(screen_num)?;
    let full = WorkArea {
        x: 0.0,
        y: 0.0,
        w: f64::from(screen.width_in_pixels),
        h: f64::from(screen.height_in_pixels),
        scale: 1.0,
    };
    let wa = (|| -> Option<(f64, f64, f64, f64)> {
        let atom = conn
            .intern_atom(false, b"_NET_WORKAREA")
            .ok()?
            .reply()
            .ok()?;
        let reply = conn
            .get_property(
                false,
                screen.root,
                atom.atom,
                x11rb::protocol::xproto::AtomEnum::CARDINAL,
                0,
                4,
            )
            .ok()?
            .reply()
            .ok()?;
        parse_workarea(&reply.value)
    })();
    if let Some((x, y, w, h)) = wa {
        Some(WorkArea {
            x,
            y,
            w,
            h,
            scale: 1.0,
        })
    } else {
        log::info!("WM 未提供 _NET_WORKAREA(精简桌面?),退回全屏尺寸定位");
        Some(full)
    }
}

/// 解析 `_NET_WORKAREA` 属性字节:`CARDINAL` 数组,取第一个桌面 x/y/w/h
/// (字节偏移 0/4/8/12);宽高为 0 视为无效,由调用方退回全屏
#[cfg(target_os = "linux")]
fn parse_workarea(bytes: &[u8]) -> Option<(f64, f64, f64, f64)> {
    let u32at = |offset: usize| -> Option<f64> {
        let raw: [u8; 4] = bytes.get(offset..offset + 4)?.try_into().ok()?;
        Some(f64::from(u32::from_ne_bytes(raw)))
    };
    let (x, y, w, h) = (u32at(0)?, u32at(4)?, u32at(8)?, u32at(12)?);
    (w > 0.0 && h > 0.0).then_some((x, y, w, h))
}

#[cfg(target_os = "linux")]
#[cfg(test)]
mod tests {
    use super::*;

    /// 取第一个桌面的工作区(偏移 0/4/8/12 的 x/y/w/h)
    #[test]
    fn first_desktop_work_area() {
        let mut bytes = vec![0u8; 16];
        bytes[8..12].copy_from_slice(&1512u32.to_ne_bytes());
        bytes[12..16].copy_from_slice(&907u32.to_ne_bytes());
        assert_eq!(
            parse_workarea(&bytes),
            Some((0.0, 0.0, 1512.0, 907.0)),
            "应取出第 3/4 字段 w/h"
        );
    }

    /// 非零原点:四个字段均按 4 字节步进解析
    #[test]
    fn nonzero_origin_four_byte_stride() {
        let mut bytes = vec![0u8; 16];
        bytes[0..4].copy_from_slice(&10u32.to_ne_bytes());
        bytes[4..8].copy_from_slice(&20u32.to_ne_bytes());
        bytes[8..12].copy_from_slice(&1000u32.to_ne_bytes());
        bytes[12..16].copy_from_slice(&600u32.to_ne_bytes());
        assert_eq!(
            parse_workarea(&bytes),
            Some((10.0, 20.0, 1000.0, 600.0)),
            "四字段按偏移依次解出"
        );
    }

    /// 全零宽高或字节长度不足均视为无效
    #[test]
    fn all_zero_or_short_input_is_invalid() {
        assert_eq!(parse_workarea(&[0u8; 16]), None, "全零宽高无效");
        assert_eq!(parse_workarea(&[0u8; 8]), None, "长度不足无效");
    }
}
