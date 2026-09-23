use serde::{Deserialize, Serialize};

use crate::notify::popup::{MAX_H, MAX_W, MIN_H, MIN_W};

/// /health 响应中标识本服务的应用名,SDK 依赖它验明身份
pub const APP_ID: &str = "x-notify-service";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub const TITLE_MAX: usize = 200;
pub const BODY_MAX: usize = 2000;

#[derive(Debug, Deserialize)]
pub struct NotifyRequest {
    pub title: String,
    pub body: Option<String>,
    /// 弹窗宽度(逻辑像素;缺省走服务端配置与内置默认)
    pub width: Option<u16>,
    /// 弹窗高度(逻辑像素;缺省走服务端配置与内置默认)
    pub height: Option<u16>,
}

#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub app: &'static str,
    pub version: &'static str,
    pub port: u16,
}

/// 通知实际展示渠道,序列化为 "popup" / "system"
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum NotifyVia {
    Popup,
    System,
}

impl NotifyVia {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Popup => "popup",
            Self::System => "system",
        }
    }
}

#[derive(Debug, Serialize)]
pub struct NotifyResponse {
    pub ok: bool,
    pub via: NotifyVia,
}

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub ok: bool,
    pub error: String,
}

/// 请求校验/解析错误:携带面向调用方的消息与 HTTP 状态码
#[derive(Debug)]
pub enum NotifyError {
    /// JSON 解析失败(400)
    BadJson(String),
    /// 语义校验失败(422)
    EmptyTitle,
    TitleTooLong,
    BodyTooLong,
    WidthOutOfRange,
    HeightOutOfRange,
}

impl NotifyError {
    pub const fn status(&self) -> u16 {
        match self {
            Self::BadJson(_) => 400,
            Self::EmptyTitle
            | Self::TitleTooLong
            | Self::BodyTooLong
            | Self::WidthOutOfRange
            | Self::HeightOutOfRange => 422,
        }
    }
}

impl std::fmt::Display for NotifyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadJson(e) => write!(f, "JSON 解析失败: {e}"),
            Self::EmptyTitle => write!(f, "title 不能为空"),
            Self::TitleTooLong => write!(f, "title 过长(最多 {TITLE_MAX} 字符)"),
            Self::BodyTooLong => write!(f, "body 过长(最多 {BODY_MAX} 字符)"),
            Self::WidthOutOfRange => write!(f, "width 越界({MIN_W}-{MAX_W} 逻辑像素)"),
            Self::HeightOutOfRange => write!(f, "height 越界({MIN_H}-{MAX_H} 逻辑像素)"),
        }
    }
}

impl std::error::Error for NotifyError {}

impl NotifyRequest {
    /// 从 JSON 请求体解析(语法错误 → `BadJson`)
    pub fn from_json(body: &str) -> Result<Self, NotifyError> {
        serde_json::from_str(body).map_err(|e| NotifyError::BadJson(e.to_string()))
    }

    /// 语义校验(空标题/超长)
    pub fn validate(&self) -> Result<(), NotifyError> {
        let title = self.title.trim();
        if title.is_empty() {
            return Err(NotifyError::EmptyTitle);
        }
        if title.chars().count() > TITLE_MAX {
            return Err(NotifyError::TitleTooLong);
        }
        if let Some(body) = &self.body
            && body.chars().count() > BODY_MAX
        {
            return Err(NotifyError::BodyTooLong);
        }
        if let Some(w) = self.width
            && !(MIN_W..=MAX_W).contains(&w)
        {
            return Err(NotifyError::WidthOutOfRange);
        }
        if let Some(h) = self.height
            && !(MIN_H..=MAX_H).contains(&h)
        {
            return Err(NotifyError::HeightOutOfRange);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_json_bad() {
        assert!(matches!(
            NotifyRequest::from_json("{bad"),
            Err(NotifyError::BadJson(_))
        ));
    }

    #[test]
    fn validate_branches() {
        let mk = |title: &str| NotifyRequest {
            title: title.into(),
            body: None,
            width: None,
            height: None,
        };
        assert!(matches!(mk(" ").validate(), Err(NotifyError::EmptyTitle)));
        assert!(matches!(
            mk(&"x".repeat(TITLE_MAX + 1)).validate(),
            Err(NotifyError::TitleTooLong)
        ));
        let long = NotifyRequest {
            title: "t".into(),
            body: Some("b".repeat(BODY_MAX + 1)),
            width: None,
            height: None,
        };
        assert!(matches!(long.validate(), Err(NotifyError::BodyTooLong)));
        // 合法请求以 unwrap 断言(测试放宽见 clippy.toml)
        mk("正常").validate().unwrap();
    }

    #[test]
    fn validate_size_range() {
        let mut req = NotifyRequest {
            title: "t".into(),
            body: None,
            width: None,
            height: None,
        };
        req.width = Some(MIN_W - 1);
        assert!(matches!(req.validate(), Err(NotifyError::WidthOutOfRange)));
        req.width = Some(MAX_W + 1);
        assert!(matches!(req.validate(), Err(NotifyError::WidthOutOfRange)));
        req.width = Some(MAX_W);
        req.height = Some(MAX_H + 1);
        assert!(matches!(req.validate(), Err(NotifyError::HeightOutOfRange)));
        req.height = Some(MIN_H);
        req.validate().unwrap();
    }

    #[test]
    fn error_status_mapping() {
        assert_eq!(NotifyError::BadJson("x".into()).status(), 400);
        assert_eq!(NotifyError::EmptyTitle.status(), 422);
        assert_eq!(NotifyError::TitleTooLong.status(), 422);
        assert_eq!(NotifyError::WidthOutOfRange.status(), 422);
        assert_eq!(NotifyError::HeightOutOfRange.status(), 422);
    }
}
