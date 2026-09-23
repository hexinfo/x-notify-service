//! GPUI 事件循环与通知窗口的唯一所有者。

use std::cell::RefCell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::time::Duration;

use futures::StreamExt as _;
use gpui_kit::{App, AppContext as _, QuitMode, WindowHandle};

use crate::notify::popup;
use crate::notify::view::{CloseRequested, PopupPayload, PopupView};
use crate::screen::WorkArea;

#[allow(clippy::module_name_repetitions)]
#[derive(Debug)]
pub enum AppError {
    PlatformInit(String),
    WindowCreate(String),
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PlatformInit(detail) => write!(f, "GPUI 初始化失败: {detail}"),
            Self::WindowCreate(detail) => write!(f, "弹窗创建失败: {detail}"),
        }
    }
}

impl std::error::Error for AppError {}

const ACK_TIMEOUT: Duration = Duration::from_secs(3);
const ACK_PENDING: u8 = 0;
const ACK_CANCELLED: u8 = 1;
const ACK_SHOWN: u8 = 2;
const ACK_FAILED: u8 = 3;

/// GUI 和 HTTP 线程共同决定一条通知是否仍可展示。
#[derive(Debug, Clone)]
pub struct PopupAck {
    sender: SyncSender<bool>,
    status: Arc<AtomicU8>,
}

struct AckWaiter {
    receiver: Receiver<bool>,
    status: Arc<AtomicU8>,
}

impl PopupAck {
    fn new() -> (Self, AckWaiter) {
        let (sender, receiver) = sync_channel(1);
        let status = Arc::new(AtomicU8::new(ACK_PENDING));
        (
            Self {
                sender,
                status: Arc::clone(&status),
            },
            AckWaiter { receiver, status },
        )
    }

    fn is_cancelled(&self) -> bool {
        self.status.load(Ordering::Acquire) == ACK_CANCELLED
    }

    /// 只有待处理通知能完成；超时取消后的展示必须由 GUI 回滚。
    fn complete(&self, shown: bool) -> bool {
        let completed = if shown { ACK_SHOWN } else { ACK_FAILED };
        if self
            .status
            .compare_exchange(ACK_PENDING, completed, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return false;
        }
        let _ = self.sender.try_send(shown);
        true
    }
}

impl AckWaiter {
    fn wait(self, timeout: Duration) -> bool {
        if let Ok(shown) = self.receiver.recv_timeout(timeout) {
            return shown;
        }
        matches!(
            self.status.compare_exchange(
                ACK_PENDING,
                ACK_CANCELLED,
                Ordering::AcqRel,
                Ordering::Acquire,
            ),
            Err(ACK_SHOWN)
        )
    }
}

/// HTTP 工作线程只发送普通 Rust 数据，不接触 GPUI 句柄。
#[derive(Debug, Clone)]
pub enum Message {
    Notify {
        title: String,
        body_markdown: String,
        quit_on_close: bool,
        size: popup::Size,
        colors: popup::Colors,
        ack: Option<PopupAck>,
    },
    Close,
}

impl From<PopupPayload> for Message {
    fn from(payload: PopupPayload) -> Self {
        Self::notify(payload, None)
    }
}

impl Message {
    fn notify(payload: PopupPayload, ack: Option<PopupAck>) -> Self {
        Self::Notify {
            title: payload.title,
            body_markdown: payload.body_markdown,
            quit_on_close: payload.quit_on_close,
            size: payload.size,
            colors: payload.colors,
            ack,
        }
    }

    fn into_payload(self) -> Option<(PopupPayload, Option<PopupAck>)> {
        let Self::Notify {
            title,
            body_markdown,
            quit_on_close,
            size,
            colors,
            ack,
        } = self
        else {
            return None;
        };
        Some((
            PopupPayload {
                title,
                body_markdown,
                size,
                colors,
                quit_on_close,
            },
            ack,
        ))
    }
}

struct PopupController {
    window: Option<WindowHandle<PopupView>>,
    quit_on_close: bool,
    failed: bool,
    failure: Rc<RefCell<Option<AppError>>>,
}

impl PopupController {
    const fn new(failure: Rc<RefCell<Option<AppError>>>) -> Self {
        Self {
            window: None,
            quit_on_close: false,
            failed: false,
            failure,
        }
    }

    /// 返回新建的窗口句柄；已有窗口更新不重复注册事件。
    fn notify(
        &mut self,
        payload: PopupPayload,
        area: WorkArea,
        cx: &mut App,
    ) -> Result<Option<WindowHandle<PopupView>>, AppError> {
        self.quit_on_close = payload.quit_on_close;
        if let Some(handle) = self.window {
            handle
                .update(cx, |view, window, view_cx| {
                    view.set_payload(payload, view_cx);
                    if let Err(error) =
                        super::window::sync_geometry(window, area, view.payload().size)
                    {
                        log::warn!("弹窗位置/尺寸复校失败: {error}");
                    }
                    window.bounds_changed(view_cx);
                })
                .map_err(|error| AppError::WindowCreate(error.to_string()))?;
            return Ok(None);
        }

        let handle = cx
            .open_window(
                super::window::window_options(area, payload.size),
                |_, cx| cx.new(|_| PopupView::new(payload)),
            )
            .map_err(|error| AppError::WindowCreate(error.to_string()))?;
        self.window = Some(handle);
        handle
            .update(cx, |view, window, view_cx| {
                if let Err(error) = super::window::sync_geometry(window, area, view.payload().size)
                {
                    log::warn!("弹窗位置/尺寸复校失败: {error}");
                }
                window.bounds_changed(view_cx);
            })
            .map_err(|error| AppError::WindowCreate(error.to_string()))?;
        Ok(Some(handle))
    }

    fn close(&mut self, cx: &mut App) {
        let Some(handle) = self.window.take() else {
            return;
        };
        let _ = handle.update(cx, |view, window, view_cx| {
            view.reset_interaction(view_cx);
            window.remove_window();
        });
        if self.quit_on_close {
            cx.quit();
        }
    }

    fn on_window_closed(&mut self, id: gpui_kit::WindowId, cx: &App) {
        if self.window.is_some_and(|handle| handle.window_id() == id) {
            self.window = None;
            if self.quit_on_close {
                cx.quit();
            }
        }
    }

    fn handle_message(
        &mut self,
        message: Message,
        cx: &mut App,
    ) -> Option<WindowHandle<PopupView>> {
        let Some((payload, ack)) = message.into_payload() else {
            self.close(cx);
            return None;
        };
        if ack.as_ref().is_some_and(PopupAck::is_cancelled) {
            return None;
        }
        if self.failed {
            if let Some(ack) = ack {
                ack.complete(false);
            } else if payload.quit_on_close {
                *self.failure.borrow_mut() =
                    Some(AppError::WindowCreate("弹窗已不可用".to_owned()));
                cx.quit();
            } else {
                show_system(&payload);
            }
            return None;
        }
        let previous = ack.as_ref().and_then(|_| {
            let handle = self.window?;
            handle.read(cx).ok().map(|view| view.payload().clone())
        });
        let result = super::window::work_area_for_popup(payload.size)
            .ok_or_else(|| AppError::WindowCreate("无法获取屏幕工作区".to_owned()))
            .and_then(|area| {
                self.notify(payload.clone(), area, cx)
                    .map(|new_window| (new_window, area))
            });
        match result {
            Ok((new_window, area)) => {
                if let Some(ack) = ack
                    && !ack.complete(true)
                {
                    // HTTP 已超时并决定使用系统通知，撤销本次 GUI 展示。
                    if let Some(previous) = previous {
                        let previous_area =
                            super::window::work_area_for_popup(previous.size).unwrap_or(area);
                        if let Err(error) = self.notify(previous, previous_area, cx) {
                            log::warn!("取消超时弹窗后恢复旧内容失败: {error}");
                            self.close(cx);
                        }
                    } else {
                        self.close(cx);
                    }
                    return None;
                }
                new_window
            }
            Err(error) => {
                log::error!("{error}");
                self.failed = true;
                super::POPUP_AVAILABLE.store(false, Ordering::Release);
                bridge::clear();
                self.close(cx);
                if let Some(ack) = ack {
                    ack.complete(false);
                } else if payload.quit_on_close {
                    *self.failure.borrow_mut() = Some(error);
                    cx.quit();
                } else {
                    show_system(&payload);
                }
                None
            }
        }
    }
}

fn show_system(payload: &PopupPayload) {
    super::fallback::show_raw(
        &payload.title,
        &crate::markdown_body::to_plain_text(&payload.body_markdown),
    );
}

fn deliver(controller: &Rc<RefCell<PopupController>>, message: Message, cx: &mut App) {
    let new_window = controller.borrow_mut().handle_message(message, cx);
    if let Some(handle) = new_window {
        install_view_events(controller, handle, cx);
    }
}

fn on_window_closed(controller: &Rc<RefCell<PopupController>>, id: gpui_kit::WindowId, cx: &App) {
    if let Ok(mut controller) = controller.try_borrow_mut() {
        controller.on_window_closed(id, cx);
    } else {
        // remove_window may invoke this observer before close releases its controller borrow.
        // A native close during another update still needs its state cleanup on the next turn.
        let controller = Rc::clone(controller);
        cx.spawn(async move |cx| {
            cx.update(|cx| controller.borrow_mut().on_window_closed(id, cx));
        })
        .detach();
    }
}

fn install_view_events(
    controller: &Rc<RefCell<PopupController>>,
    handle: WindowHandle<PopupView>,
    cx: &mut App,
) {
    if let Ok(view) = handle.entity(cx) {
        let on_close = Rc::clone(controller);
        cx.subscribe(&view, move |_, _: &CloseRequested, cx| {
            let on_close = Rc::clone(&on_close);
            cx.defer(move |cx| on_close.borrow_mut().close(cx));
        })
        .detach();
    }
}

/// 服务模式在主线程进入 GPUI，事件循环只通过显式 quit 结束。
pub fn run_service() -> Result<(), AppError> {
    run(None)
}

/// 单发模式的窗口关闭后退出；启动失败由调用方负责系统通知兜底。
pub fn run_single(mut payload: PopupPayload) -> Result<(), AppError> {
    payload.quit_on_close = true;
    run(Some(payload))
}

fn run(initial: Option<PopupPayload>) -> Result<(), AppError> {
    super::POPUP_AVAILABLE.store(false, Ordering::Release);
    let failure = Rc::new(RefCell::new(None));
    let observed_failure = Rc::clone(&failure);
    let result = catch_unwind(AssertUnwindSafe(move || {
        gpui_kit::application()
            .with_quit_mode(QuitMode::Explicit)
            .run(move |cx| {
                gpui_kit::init(cx);
                super::window::hide_app_from_dock();
                let controller = Rc::new(RefCell::new(PopupController::new(observed_failure)));
                let on_close = Rc::clone(&controller);
                cx.on_window_closed(move |cx, id| on_window_closed(&on_close, id, cx))
                    .detach();
                if let Some(payload) = initial {
                    deliver(&controller, Message::from(payload), cx);
                    return;
                }

                let mut receiver = bridge::install();
                let consumer = Rc::clone(&controller);
                cx.spawn(async move |cx| {
                    while let Some(message) = receiver.next().await {
                        cx.update(|cx| deliver(&consumer, message, cx));
                    }
                })
                .detach();
                super::POPUP_AVAILABLE.store(true, Ordering::Release);
            });
    }));
    super::POPUP_AVAILABLE.store(false, Ordering::Release);
    bridge::clear();
    match result {
        Err(panic) => Err(AppError::PlatformInit(panic_message(panic.as_ref()))),
        Ok(()) => failure.borrow_mut().take().map_or(Ok(()), Err),
    }
}

fn panic_message(panic: &(dyn std::any::Any + Send)) -> String {
    if let Some(message) = panic.downcast_ref::<String>() {
        message.clone()
    } else if let Some(message) = panic.downcast_ref::<&str>() {
        (*message).to_owned()
    } else {
        "未知平台错误".to_owned()
    }
}

/// 事件循环未就绪或已失效时返回 false，调用方降级到系统通知。
pub fn post(message: Message) -> bool {
    bridge::post(message)
}

/// HTTP 请求在 GUI 确认展示后才报告 via=popup；超时的排队通知会被取消。
pub fn present_notification(payload: PopupPayload) -> bool {
    let (ack, waiter) = PopupAck::new();
    if !post(Message::notify(payload, Some(ack))) {
        return false;
    }
    waiter.wait(ACK_TIMEOUT)
}

mod bridge {
    use super::Message;
    use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
    use std::sync::Mutex;

    static SENDER: Mutex<Option<UnboundedSender<Message>>> = Mutex::new(None);

    fn sender() -> std::sync::MutexGuard<'static, Option<UnboundedSender<Message>>> {
        SENDER
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub(super) fn install() -> UnboundedReceiver<Message> {
        let (tx, rx) = unbounded();
        *sender() = Some(tx);
        rx
    }

    pub(super) fn clear() {
        *sender() = None;
    }

    pub(super) fn post(message: Message) -> bool {
        sender()
            .as_ref()
            .is_some_and(|sender| sender.unbounded_send(message).is_ok())
    }
}

#[cfg(test)]
#[allow(clippy::needless_pass_by_ref_mut)] // GPUI 测试宏要求 &mut TestAppContext。
mod lifecycle_tests {
    use super::{
        Message, PopupAck, PopupController, PopupPayload, bridge, install_view_events,
        on_window_closed,
    };
    use crate::notify::popup::{Colors, Size};
    use crate::screen::WorkArea;
    use futures::StreamExt as _;
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{AppContext as _, TestAppContext, px};
    use std::time::Duration;
    use std::{cell::RefCell, rc::Rc};

    fn payload(title: &str) -> PopupPayload {
        PopupPayload {
            title: title.to_owned(),
            body_markdown: "正文".to_owned(),
            size: Size::DEFAULT,
            colors: Colors::DEFAULT,
            quit_on_close: false,
        }
    }

    fn area() -> WorkArea {
        WorkArea {
            x: 0.0,
            y: 0.0,
            w: 1024.0,
            h: 768.0,
            scale: 1.0,
        }
    }

    #[test]
    fn bridge_rejects_posts_before_ready_and_after_disconnect() {
        bridge::clear();
        assert!(!bridge::post(Message::Close));
        let mut receiver = bridge::install();
        assert!(bridge::post(Message::Close));
        assert!(matches!(
            futures::executor::block_on(receiver.next()),
            Some(Message::Close)
        ));
        bridge::clear();
        assert!(!bridge::post(Message::Close));
    }

    #[test]
    fn notification_ack_reports_completed_popup() {
        let (ack, waiter) = PopupAck::new();
        assert!(ack.complete(true));
        assert!(waiter.wait(Duration::ZERO));
    }

    #[test]
    fn notification_ack_reports_failed_popup() {
        let (ack, waiter) = PopupAck::new();
        assert!(ack.complete(false));
        assert!(!waiter.wait(Duration::ZERO));
    }

    #[test]
    fn notification_ack_timeout_cancels_late_popup() {
        let (ack, waiter) = PopupAck::new();
        assert!(!waiter.wait(Duration::ZERO));
        assert!(ack.is_cancelled());
        assert!(!ack.complete(true));
    }

    #[gpui_kit::test]
    fn cancelled_notification_never_opens_a_window(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (ack, waiter) = PopupAck::new();
        assert!(!waiter.wait(Duration::ZERO));
        cx.update(|cx| {
            let mut controller = PopupController::new(Rc::new(RefCell::new(None)));
            let opened =
                controller.handle_message(Message::notify(payload("已取消"), Some(ack)), cx);
            assert!(opened.is_none());
            assert!(cx.windows().is_empty());
        });
    }

    #[gpui_kit::test]
    fn failed_gui_reports_failure_to_waiting_request(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (ack, waiter) = PopupAck::new();
        cx.update(|cx| {
            let mut controller = PopupController::new(Rc::new(RefCell::new(None)));
            controller.failed = true;
            assert!(
                controller
                    .handle_message(Message::notify(payload("失败"), Some(ack)), cx)
                    .is_none()
            );
        });
        assert!(!waiter.wait(Duration::ZERO));
    }

    #[gpui_kit::test]
    fn notify_burst_reuses_one_window_with_latest_payload(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        cx.update(|cx| {
            let mut controller = PopupController::new(Rc::new(RefCell::new(None)));
            for title in ["A", "B", "C"] {
                controller.notify(payload(title), area(), cx).unwrap();
            }
            assert_eq!(cx.windows().len(), 1);
            assert_eq!(
                controller.window.unwrap().read(cx).unwrap().payload().title,
                "C"
            );
        });
    }

    #[gpui_kit::test]
    fn visible_notification_resize_updates_viewport_and_layout(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let handle = cx.update(|cx| {
            let mut controller = PopupController::new(Rc::new(RefCell::new(None)));
            let handle = controller
                .notify(payload("A"), area(), cx)
                .unwrap()
                .unwrap();
            let mut updated = payload("B");
            updated.size = Size {
                width: 400.0,
                height: 180.0,
            };
            assert!(controller.notify(updated, area(), cx).unwrap().is_none());
            assert_eq!(controller.window, Some(handle));
            assert_eq!(cx.windows().len(), 1);
            handle
        });
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(window.viewport_size().width, px(400.));
            assert_eq!(window.viewport_size().height, px(180.));
            assert_eq!(window.find("popup-root").bounds().size.width, px(400.));
            assert_eq!(window.find("popup-header").bounds().size.width, px(398.));
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn close_then_notify_reopens_one_clean_window(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        cx.update(|cx| {
            let mut controller = PopupController::new(Rc::new(RefCell::new(None)));
            controller.notify(payload("A"), area(), cx).unwrap();
            controller.close(cx);
            controller.notify(payload("B"), area(), cx).unwrap();
            assert_eq!(cx.windows().len(), 1);
            assert_eq!(
                controller.window.unwrap().read(cx).unwrap().payload().title,
                "B"
            );
        });
    }

    #[gpui_kit::test]
    fn close_request_removes_window_before_reopen(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        cx.update(|cx| cx.set_reduce_motion(true));
        let controller = Rc::new(RefCell::new(PopupController::new(Rc::new(RefCell::new(
            None,
        )))));
        let handle = cx.update(|cx| {
            let on_close = Rc::clone(&controller);
            cx.on_window_closed(move |cx, id| on_window_closed(&on_close, id, cx))
                .detach();
            let handle = controller
                .borrow_mut()
                .notify(payload("A"), area(), cx)
                .unwrap()
                .unwrap();
            install_view_events(&controller, handle, cx);
            handle
        });
        cx.update_window(handle.into(), |_, window, cx| {
            window.click("popup-close", cx);
        })
        .unwrap();
        cx.run_until_parked();
        assert!(controller.borrow().window.is_none());
        cx.update(|cx| {
            controller
                .borrow_mut()
                .notify(payload("B"), area(), cx)
                .unwrap();
            assert_eq!(cx.windows().len(), 1);
        });
    }

    #[gpui_kit::test]
    fn native_window_close_clears_current_handle(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let controller = Rc::new(RefCell::new(PopupController::new(Rc::new(RefCell::new(
            None,
        )))));
        let handle = cx.update(|cx| {
            let on_close = Rc::clone(&controller);
            cx.on_window_closed(move |cx, id| on_window_closed(&on_close, id, cx))
                .detach();
            controller
                .borrow_mut()
                .notify(payload("A"), area(), cx)
                .unwrap()
                .unwrap()
        });
        cx.update_window(handle.into(), |_, window, _| window.remove_window())
            .unwrap();
        cx.run_until_parked();
        assert!(controller.borrow().window.is_none());
    }
}
