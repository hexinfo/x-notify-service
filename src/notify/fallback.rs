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

#[cfg(not(target_os = "linux"))]
const fn supports_body_markup() -> bool {
    false
}

/// 系统通知兜底渠道(notify-rust:Linux `DBus` / macOS / Windows)
pub struct SystemPresenter {
    /// Windows toast 的 AppId(AUMID);未配置时用库默认(PowerShell)
    #[cfg_attr(not(windows), allow(dead_code))]
    app_id: Option<String>,
}

impl SystemPresenter {
    pub fn new(cfg: &Config) -> Self {
        Self {
            app_id: cfg.app_id.clone(),
        }
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
        // 系统通知为纯文本(尺寸不适用),Markdown 正文投影为可读文本
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
        #[cfg(windows)]
        if let Some(app_id) = &self.app_id {
            n.app_id(app_id);
        }
        match n.show() {
            Ok(_) => true,
            Err(e) => {
                log::error!("系统通知发送失败: {e}");
                false
            }
        }
    }
}

/// 弹窗路径内部降级时使用(无 Config 场景)
pub fn show_raw(title: &str, plain_body: &str) {
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
