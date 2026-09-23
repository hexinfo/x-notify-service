use crate::{notify::popup::Size, screen::WorkArea};
use gpui_kit::{
    Bounds, Window, WindowBounds, WindowDecorations, WindowKind, WindowOptions, point, px,
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

/// GPUI 的实际 Linux 后端也会在运行时从窗口句柄复核。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendKind {
    #[cfg(windows)]
    Windows,
    #[cfg(target_os = "macos")]
    MacOS,
    #[cfg(target_os = "linux")]
    X11,
    #[cfg(target_os = "linux")]
    Wayland,
}

/// 纯环境选择：有 Wayland socket 时优先使用 Layer Shell，避免落到 XWayland。
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
#[cfg_attr(not(target_os = "linux"), allow(clippy::unnecessary_wraps))]
pub const fn backend_kind_from_env(
    wayland_display: Option<&str>,
    x_display: Option<&str>,
) -> Option<BackendKind> {
    #[cfg(target_os = "linux")]
    {
        if matches!(wayland_display, Some(display) if !display.is_empty()) {
            return Some(BackendKind::Wayland);
        }
        if matches!(x_display, Some(display) if !display.is_empty()) {
            return Some(BackendKind::X11);
        }
        None
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (wayland_display, x_display);
        #[cfg(windows)]
        {
            Some(BackendKind::Windows)
        }
        #[cfg(target_os = "macos")]
        {
            Some(BackendKind::MacOS)
        }
    }
}

#[cfg(target_os = "linux")]
fn backend_kind() -> Option<BackendKind> {
    if gpui_kit::guess_compositor() == "Headless" {
        return None;
    }
    backend_kind_from_env(
        std::env::var("WAYLAND_DISPLAY").ok().as_deref(),
        std::env::var("DISPLAY").ok().as_deref(),
    )
}

/// Layer Shell 的锚点与边距由 compositor 处理；工作区只需携带请求逻辑尺寸。
#[cfg(any(target_os = "linux", test))]
fn layer_shell_area(size: Size) -> WorkArea {
    WorkArea {
        x: 0.0,
        y: 0.0,
        w: size.width + 14.0,
        h: size.height + 14.0,
        scale: 1.0,
    }
}

#[cfg(any(target_os = "linux", test))]
fn work_area_with_layer_shell(
    size: Size,
    use_layer_shell: bool,
    read_work_area: impl FnOnce() -> Option<WorkArea>,
) -> Option<WorkArea> {
    if use_layer_shell {
        Some(layer_shell_area(size))
    } else {
        read_work_area()
    }
}

#[cfg(target_os = "linux")]
pub fn work_area_for_popup(size: Size) -> Option<WorkArea> {
    work_area_with_layer_shell(
        size,
        backend_kind() == Some(BackendKind::Wayland),
        crate::screen::work_area,
    )
}

#[cfg(not(target_os = "linux"))]
pub fn work_area_for_popup(_size: Size) -> Option<WorkArea> {
    crate::screen::work_area()
}

/// 工作区与请求尺寸均以物理像素计算，返回 `(x, y, width, height)`。
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub fn popup_bounds(area: WorkArea, size: Size) -> (i32, i32, u32, u32) {
    let scale = area.scale.max(1.0);
    let (x, y) = crate::notify::popup::landing(&area, size);
    (
        x,
        y,
        (size.width * scale).round() as u32,
        (size.height * scale).round() as u32,
    )
}

/// 创建时的位置由 GPUI 的逻辑像素表示；原生更新使用物理像素。
#[allow(clippy::cast_precision_loss)]
#[allow(clippy::cast_possible_truncation, clippy::module_name_repetitions)]
pub fn window_options(area: WorkArea, popup_size: Size) -> WindowOptions {
    let (x, y, width, height) = popup_bounds(area, popup_size);
    let scale = area.scale.max(1.0) as f32;
    let bounds = Bounds {
        origin: point(px(x as f32 / scale), px(y as f32 / scale)),
        size: gpui_kit::size(px(width as f32 / scale), px(height as f32 / scale)),
    };
    #[cfg(target_os = "linux")]
    let kind = if backend_kind() == Some(BackendKind::Wayland) {
        use gpui_kit::layer_shell::{Anchor, KeyboardInteractivity, Layer, LayerShellOptions};

        WindowKind::LayerShell(LayerShellOptions {
            namespace: "x-notify-service".to_owned(),
            layer: Layer::Overlay,
            anchor: Anchor::RIGHT | Anchor::BOTTOM,
            margin: Some((px(0.0), px(14.0), px(14.0), px(0.0))),
            keyboard_interactivity: KeyboardInteractivity::None,
            ..LayerShellOptions::default()
        })
    } else {
        WindowKind::PopUp
    };
    #[cfg(not(target_os = "linux"))]
    let kind = WindowKind::PopUp;

    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: None,
        focus: false,
        kind,
        is_movable: false,
        is_resizable: false,
        is_minimizable: false,
        window_decorations: Some(WindowDecorations::Client),
        icon: {
            #[cfg(target_os = "linux")]
            {
                super::icon::popup_icon()
            }
            #[cfg(not(target_os = "linux"))]
            {
                None
            }
        },
        ..WindowOptions::default()
    }
}

#[derive(Debug)]
#[allow(clippy::module_name_repetitions)]
pub struct WindowSyncError {
    backend: &'static str,
    detail: String,
}

impl WindowSyncError {
    fn new(backend: &'static str, detail: impl Into<String>) -> Self {
        Self {
            backend,
            detail: detail.into(),
        }
    }
}

impl std::fmt::Display for WindowSyncError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} window geometry: {}", self.backend, self.detail)
    }
}

impl std::error::Error for WindowSyncError {}

#[allow(clippy::missing_const_for_fn)]
fn expected_backend_name() -> &'static str {
    #[cfg(windows)]
    {
        "Win32"
    }
    #[cfg(target_os = "macos")]
    {
        "AppKit"
    }
    #[cfg(target_os = "linux")]
    {
        if backend_kind() == Some(BackendKind::Wayland) {
            "Wayland"
        } else {
            "X11"
        }
    }
}

/// 可见窗口更新时依据实际句柄分派，避免环境变量与 GPUI 后端不一致。
#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
pub fn sync_geometry(
    window: &mut Window,
    area: WorkArea,
    popup_size: Size,
) -> Result<(), WindowSyncError> {
    #[cfg(target_os = "macos")]
    let (x, y, _, height) = popup_bounds(area, popup_size);
    #[cfg(not(target_os = "macos"))]
    let (x, y, width, height) = popup_bounds(area, popup_size);
    window.resize(gpui_kit::size(
        px(popup_size.width as f32),
        px(popup_size.height as f32),
    ));
    let handle = HasWindowHandle::window_handle(window)
        .map_err(|error| WindowSyncError::new(expected_backend_name(), error.to_string()))?;
    match handle.as_raw() {
        #[cfg(windows)]
        RawWindowHandle::Win32(handle) => sync_windows(handle.hwnd.get(), x, y, width, height),
        #[cfg(target_os = "macos")]
        RawWindowHandle::AppKit(handle) => sync_macos(handle.ns_view, area, x, y, height),
        #[cfg(target_os = "linux")]
        RawWindowHandle::Xcb(handle) => sync_x11(handle.window.get(), x, y, width, height),
        #[cfg(target_os = "linux")]
        RawWindowHandle::Xlib(handle) => {
            let xid = u32::try_from(handle.window)
                .map_err(|error| WindowSyncError::new("X11", error.to_string()))?;
            sync_x11(xid, x, y, width, height)
        }
        #[cfg(target_os = "linux")]
        RawWindowHandle::Wayland(_) => Ok(()),
        other => Err(WindowSyncError::new(
            expected_backend_name(),
            format!("unsupported handle: {other:?}"),
        )),
    }
}

#[cfg(windows)]
#[allow(unsafe_code)]
fn sync_windows(
    hwnd: isize,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
) -> Result<(), WindowSyncError> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        HWND_TOPMOST, SWP_NOACTIVATE, SWP_SHOWWINDOW, SetWindowPos,
    };

    let width =
        i32::try_from(width).map_err(|error| WindowSyncError::new("Win32", error.to_string()))?;
    let height =
        i32::try_from(height).map_err(|error| WindowSyncError::new("Win32", error.to_string()))?;
    // SAFETY: HWND 来自当前活跃 GPUI Window，坐标和尺寸均为有效整型值。
    let result = unsafe {
        SetWindowPos(
            hwnd as _,
            HWND_TOPMOST,
            x,
            y,
            width,
            height,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        )
    };
    if result == 0 {
        return Err(WindowSyncError::new(
            "Win32",
            std::io::Error::last_os_error().to_string(),
        ));
    }
    Ok(())
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
fn sync_macos(
    native_view: std::ptr::NonNull<std::ffi::c_void>,
    area: WorkArea,
    x: i32,
    y: i32,
    height: u32,
) -> Result<(), WindowSyncError> {
    use dispatch2::{DispatchQueue, MainThreadBound};
    use objc2::{MainThreadMarker, Message as _};
    use objc2_app_kit::{NSScreen, NSView};
    use objc2_foundation::NSPoint;

    let main_thread = MainThreadMarker::new().ok_or_else(|| {
        WindowSyncError::new("AppKit", "window update must run on the main thread")
    })?;
    let screen = NSScreen::mainScreen(main_thread)
        .ok_or_else(|| WindowSyncError::new("AppKit", "main screen unavailable"))?;
    // SAFETY: 原生 NSView 指针由当前存活的 GPUI Window 借出，且本函数在主线程调用。
    let view = unsafe { native_view.cast::<NSView>().as_ref() };
    if view.window().is_none() {
        return Err(WindowSyncError::new("AppKit", "NSView has no NSWindow"));
    }
    let scale = area.scale.max(1.0);
    let screen_frame = screen.frame();
    let origin = (
        f64::from(x) / scale,
        screen_frame.origin.y + screen_frame.size.height
            - (f64::from(y) + f64::from(height)) / scale,
    );
    let view = MainThreadBound::new(view.retain(), main_thread);
    // GPUI 将自身的 resize 排入同一主队列；随后移动窗口可避开正在借用的 GPUI Window。
    DispatchQueue::main().exec_async(move || {
        if let Some(main_thread) = MainThreadMarker::new()
            && let Some(window) = view.get(main_thread).window()
        {
            window.setFrameOrigin(NSPoint::new(origin.0, origin.1));
        }
    });
    Ok(())
}

#[cfg(target_os = "linux")]
fn sync_x11(xid: u32, x: i32, y: i32, width: u32, height: u32) -> Result<(), WindowSyncError> {
    use x11rb::{
        connection::Connection as _,
        protocol::xproto::{ConfigureWindowAux, ConnectionExt as _, StackMode},
    };

    let (connection, _) =
        x11rb::connect(None).map_err(|error| WindowSyncError::new("X11", error.to_string()))?;
    connection
        .configure_window(
            xid,
            &ConfigureWindowAux::new()
                .x(x)
                .y(y)
                .width(width)
                .height(height)
                .stack_mode(StackMode::ABOVE),
        )
        .map_err(|error| WindowSyncError::new("X11", error.to_string()))?
        .check()
        .map_err(|error| WindowSyncError::new("X11", error.to_string()))?;
    connection
        .flush()
        .map_err(|error| WindowSyncError::new("X11", error.to_string()))
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "linux")]
    use super::{BackendKind, backend_kind_from_env};
    use super::{popup_bounds, sync_geometry, work_area_with_layer_shell};
    use crate::{notify::popup::Size, screen::WorkArea};
    use gpui_kit::{AppContext as _, EmptyView, TestAppContext, px, size};
    use std::cell::Cell;

    #[test]
    fn wayland_area_needs_no_x11_work_area() {
        let requested = Size {
            width: 400.0,
            height: 180.0,
        };
        let probed_x11 = Cell::new(false);
        let area = work_area_with_layer_shell(requested, true, || {
            probed_x11.set(true);
            None
        })
        .unwrap();
        assert_eq!(
            (probed_x11.get(), popup_bounds(area, requested)),
            (false, (0, 0, 400, 180))
        );
    }

    #[test]
    fn x11_area_uses_measured_work_area() {
        let measured = WorkArea {
            x: 0.0,
            y: 0.0,
            w: 1920.0,
            h: 1080.0,
            scale: 1.0,
        };
        let area = work_area_with_layer_shell(Size::DEFAULT, false, || Some(measured)).unwrap();
        assert_eq!(popup_bounds(area, Size::DEFAULT), (1686, 966, 220, 100));
    }

    #[gpui_kit::test]
    fn visible_resize_reaches_gpui_before_native_reposition(cx: &mut TestAppContext) {
        let handle = cx.open_window(size(px(220.0), px(100.0)), |_, _| EmptyView);
        let area = WorkArea {
            x: 0.0,
            y: 0.0,
            w: 1920.0,
            h: 1080.0,
            scale: 1.0,
        };

        cx.update_window(handle.into(), |_, window, _| {
            // Headless 窗口没有原生句柄；尺寸变化可证明 GPUI resize 先于句柄读取。
            let _sync_result = sync_geometry(
                window,
                area,
                Size {
                    width: 400.0,
                    height: 180.0,
                },
            );
            assert_eq!(window.bounds().size, size(px(400.0), px(180.0)));
        })
        .unwrap();
    }

    #[test]
    fn resizing_preserves_bottom_right_margin() {
        let area = WorkArea {
            x: 0.0,
            y: 30.0,
            w: 1920.0,
            h: 985.0,
            scale: 1.0,
        };
        assert_eq!(
            popup_bounds(
                area,
                Size {
                    width: 220.0,
                    height: 100.0
                }
            ),
            (1686, 901, 220, 100)
        );
        assert_eq!(
            popup_bounds(
                area,
                Size {
                    width: 400.0,
                    height: 140.0
                }
            ),
            (1506, 861, 400, 140)
        );
    }

    #[test]
    fn scaled_work_area_uses_physical_pixels() {
        let area = WorkArea {
            x: 0.0,
            y: 48.0,
            w: 2880.0,
            h: 1752.0,
            scale: 2.0,
        };
        assert_eq!(
            popup_bounds(
                area,
                Size {
                    width: 220.0,
                    height: 100.0
                }
            ),
            (2412, 1572, 440, 200)
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn wayland_display_selects_layer_shell() {
        assert_eq!(
            backend_kind_from_env(Some("wayland-0"), None),
            Some(BackendKind::Wayland)
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn x_display_selects_x11() {
        assert_eq!(
            backend_kind_from_env(None, Some(":0")),
            Some(BackendKind::X11)
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn wayland_takes_precedence_over_xwayland() {
        assert_eq!(
            backend_kind_from_env(Some("wayland-0"), Some(":0")),
            Some(BackendKind::Wayland)
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn no_display_is_unavailable() {
        assert_eq!(backend_kind_from_env(None, None), None);
    }
}
