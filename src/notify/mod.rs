pub mod app;
pub mod fallback;
#[cfg(target_os = "linux")]
mod icon;
pub mod popup;
pub mod view;
pub mod window;

use std::sync::atomic::{AtomicBool, Ordering};

use crate::api::{NotifyRequest, NotifyVia};
use crate::config::Config;

/// 弹窗 GUI 是否可用(启动时探测,失败则全程走系统通知兜底)
pub static POPUP_AVAILABLE: AtomicBool = AtomicBool::new(false);

/// 通知展示渠道:实现方负责把一条通知真正呈现给用户。
/// `size` 为解析后的弹窗尺寸,仅弹窗渠道消费
pub trait Presenter {
    /// 展示通知;返回 false 表示本渠道投递失败(调用方降级到下一渠道)
    fn present(
        &self,
        title: &str,
        body_markdown: &str,
        size: popup::Size,
        colors: popup::Colors,
    ) -> bool;
}

/// 右下角置顶弹窗(主渠道):经 bridge 通道投递给 GPUI 事件循环。
/// 通道天然按序投递,每条各触发一次内容更新——窗口单实例,
/// 后到的通知自然顶掉先到的(latest-only 语义不变)。
pub struct PopupPresenter;

impl Presenter for PopupPresenter {
    fn present(
        &self,
        title: &str,
        body_markdown: &str,
        size: popup::Size,
        colors: popup::Colors,
    ) -> bool {
        if !POPUP_AVAILABLE.load(Ordering::Acquire) {
            return false;
        }
        let shown = app::present_notification(view::PopupPayload {
            title: title.to_owned(),
            body_markdown: body_markdown.to_owned(),
            size,
            colors,
            quit_on_close: false,
        });
        if !shown {
            log::warn!("弹窗展示未确认,降级系统通知");
        }
        shown
    }
}

/// 投递一条通知:弹窗为主,失败自动降级系统通知兜底。
/// 弹窗尺寸逐轴解析:/notify 请求 > 默认
pub fn dispatch(cfg: &Config, req: &NotifyRequest) -> NotifyVia {
    let title = req.title.trim();
    let body = req.body.as_deref().unwrap_or("");
    let size = popup::resolve_size(req.width, req.height);
    let colors = popup::resolve_colors(
        req.header_background_color.as_deref(),
        req.header_text_color.as_deref(),
        req.body_background_color.as_deref(),
        req.body_text_color.as_deref(),
    );
    present_with_fallback(
        &PopupPresenter,
        &fallback::SystemPresenter::new(cfg),
        title,
        body,
        size,
        colors,
    )
}

fn present_with_fallback(
    popup: &impl Presenter,
    system: &impl Presenter,
    title: &str,
    body: &str,
    size: popup::Size,
    colors: popup::Colors,
) -> NotifyVia {
    if popup.present(title, body, size, colors) {
        return NotifyVia::Popup;
    }
    system.present(title, body, size, colors);
    NotifyVia::System
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::{Presenter, present_with_fallback};
    use crate::api::NotifyVia;
    use crate::notify::popup::{Colors, Size};

    struct CountingPresenter {
        calls: Cell<usize>,
        result: bool,
    }

    impl Presenter for CountingPresenter {
        fn present(&self, _: &str, _: &str, _: Size, _: Colors) -> bool {
            self.calls.set(self.calls.get() + 1);
            self.result
        }
    }

    #[test]
    fn confirmed_popup_skips_system_notification() {
        let popup = CountingPresenter {
            calls: Cell::new(0),
            result: true,
        };
        let system = CountingPresenter {
            calls: Cell::new(0),
            result: true,
        };
        let via = present_with_fallback(
            &popup,
            &system,
            "标题",
            "正文",
            Size::DEFAULT,
            Colors::DEFAULT,
        );
        assert_eq!(via, NotifyVia::Popup);
        assert_eq!(system.calls.get(), 0);
    }

    #[test]
    fn unconfirmed_popup_triggers_one_system_notification() {
        let popup = CountingPresenter {
            calls: Cell::new(0),
            result: false,
        };
        let system = CountingPresenter {
            calls: Cell::new(0),
            result: true,
        };
        let via = present_with_fallback(
            &popup,
            &system,
            "标题",
            "正文",
            Size::DEFAULT,
            Colors::DEFAULT,
        );
        assert_eq!(via, NotifyVia::System);
        assert_eq!(popup.calls.get(), 1);
        assert_eq!(system.calls.get(), 1);
    }
}
