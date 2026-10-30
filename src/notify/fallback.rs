use crate::config::Config;
use crate::notify::Presenter;

#[cfg(target_os = "linux")]
fn supports_body_markup() -> bool {
    notify_rust::get_capabilities().is_ok_and(|capabilities| {
        capabilities
            .iter()
            .any(|capability| capability == "body-markup")
    })
}

/// macOS 通知不支持能力探测,按纯文本投递
#[cfg(all(unix, not(target_os = "linux")))]
const fn supports_body_markup() -> bool {
    false
}

/// 系统通知兜底渠道(notify-rust:Linux `DBus` / macOS)。
/// Windows 构建不启用:notify-rust 的 Windows 后端依赖 WinRT(Win8+),
/// 为兼容 Win7 的单一安装包,Windows 仅保留弹窗主渠道。
pub struct SystemPresenter;

impl SystemPresenter {
    pub const fn new(_cfg: &Config) -> Self {
        Self
    }
}

impl Presenter for SystemPresenter {
    fn present(
        &self,
        title: &str,
        body_markdown: &str,
        _size: super::popup::Size,
        _colors: super::popup::Colors,
    ) -> bool {
        // Windows:弹窗主渠道失败即整体失败,不再降级系统通知
        #[cfg(windows)]
        {
            let _ = (title, body_markdown);
            log::warn!("Windows 构建未启用系统通知兜底(WinRT 后端与 Win7 不兼容)");
            false
        }
        // 系统通知为纯文本(尺寸不适用),Markdown 正文投影为可读文本
        #[cfg(unix)]
        {
            let plain = crate::markdown_body::to_plain_text(body_markdown);
            let body = crate::markdown_body::notification_body_for_capabilities(
                &plain,
                supports_body_markup(),
            );
            let mut n = notify_rust::Notification::new();
            n.appname("x-notify-service")
                .icon(crate::config::APP_DIR_NAME)
                .summary(title)
                .body(&body);
            match n.show() {
                Ok(_) => true,
                Err(e) => {
                    log::error!("系统通知发送失败: {e}");
                    false
                }
            }
        }
    }
}

/// 弹窗路径内部降级时使用(无 Config 场景)
pub fn show_raw(title: &str, plain_body: &str) {
    #[cfg(windows)]
    {
        let _ = (title, plain_body);
        log::warn!("Windows 构建未启用系统通知兜底(WinRT 后端与 Win7 不兼容)");
    }
    #[cfg(unix)]
    {
        let body = crate::markdown_body::notification_body_for_capabilities(
            plain_body,
            supports_body_markup(),
        );
        let mut n = notify_rust::Notification::new();
        n.appname("x-notify-service").summary(title).body(&body);
        if let Err(e) = n.show() {
            log::error!("系统通知发送失败: {e}");
        }
    }
}
