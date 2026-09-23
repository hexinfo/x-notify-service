use crate::config::Config;
use crate::notify::Presenter;

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
        body_html: &str,
        _size: super::popup::Size,
        _colors: super::popup::Colors,
    ) -> bool {
        // 系统通知为纯文本(尺寸不适用),HTML 正文先剥离标记
        let body = crate::html::to_plain_text(body_html);
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
    let mut n = notify_rust::Notification::new();
    n.appname("x-notify-service")
        .summary(title)
        .body(plain_body);
    if let Err(e) = n.show() {
        log::error!("系统通知发送失败: {e}");
    }
}
