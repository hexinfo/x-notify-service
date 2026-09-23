pub mod app;
pub mod fallback;
pub mod popup;
#[cfg(target_os = "linux")]
pub mod window_icon;

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
        body_html: &str,
        size: popup::Size,
        colors: popup::Colors,
    ) -> bool;
}

/// 右下角置顶弹窗(主渠道):经 bridge 通道投递给 iced 事件循环。
/// 通道天然按序投递,每条各触发一次内容更新——窗口单实例,
/// 后到的通知自然顶掉先到的(latest-only 语义不变)。
pub struct PopupPresenter;

impl Presenter for PopupPresenter {
    fn present(
        &self,
        title: &str,
        body_html: &str,
        size: popup::Size,
        colors: popup::Colors,
    ) -> bool {
        if !POPUP_AVAILABLE.load(Ordering::Relaxed) {
            return false;
        }
        let posted = app::post(app::Message::Notify {
            title: title.to_owned(),
            body_html: body_html.to_owned(),
            quit_on_close: false,
            size,
            colors,
        });
        if !posted {
            log::warn!("弹窗投递失败,降级系统通知");
        }
        posted
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
    if PopupPresenter.present(title, body, size, colors) {
        return NotifyVia::Popup;
    }
    fallback::SystemPresenter::new(cfg).present(title, body, size, colors);
    NotifyVia::System
}
