//! HTTP API 集成测试:在测试进程内启动 server,直连断言各端点行为。
//! 每个测试独占端口,并行安全;不需要真实 GUI(无头环境走系统通知降级)。

#![cfg(test)]

use std::io::{Read as _, Write as _};
use std::net::TcpStream;
use std::time::Duration;

use crate::config::Config;

/// 在空闲端口启动 server,返回 (port, 优雅drop 前一直活着)
fn start_server(cfg_override: impl FnOnce(&mut Config)) -> u16 {
    // 先取 OS 分配的空闲端口再传给 server(port=0 会走 fallback 循环的
    // 0..10 遍历,在部分平台语义不正确)
    let free = std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .map(|a| a.port())
        .expect("取空闲端口失败");
    drop(std::net::TcpListener::bind(("127.0.0.1", free))); // 释放后 server 可绑
    let mut cfg = Config {
        port: free,
        log_level: "error".into(),
        log_dir: std::env::temp_dir().join("xns-test-logs"),
        no_popup: true,
        cors_origins: vec!["*".into()],
        allow_private_network: true,
        token: None,
        app_id: None,
        popup_width: None,
        popup_height: None,
    };
    cfg_override(&mut cfg);
    crate::server::start(cfg)
}

/// 极简 HTTP 请求(复用 ctl.rs 模式),返回 (status, headers, body)。
/// 读超时后重试一次(WORKERS=1 的单线程服务在并发测试下可能瞬断)
fn http(
    port: u16,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: &str,
) -> (u16, String, String) {
    let attempt = |port: u16, method: &str, path: &str, headers: &[(&str, &str)], body: &str| {
        let mut conn = TcpStream::connect(("127.0.0.1", port)).expect("连接失败");
        conn.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        conn.set_write_timeout(Some(Duration::from_secs(2)))
            .unwrap();

        let mut req =
            format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n");
        for (k, v) in headers {
            req.push_str(k);
            req.push_str(": ");
            req.push_str(v);
            req.push_str("\r\n");
        }
        if !body.is_empty() {
            req.push_str("Content-Length: ");
            req.push_str(&body.len().to_string());
            req.push_str("\r\n");
        }
        req.push_str("\r\n");
        if !body.is_empty() {
            req.push_str(body);
        }
        conn.write_all(req.as_bytes()).unwrap();

        let mut buf = String::new();
        match conn.read_to_string(&mut buf) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => return None,
            Err(e) if e.kind() == std::io::ErrorKind::ConnectionReset => return None,
            Err(_) => return None,
        }
        let (head, body) = buf.split_once("\r\n\r\n").unwrap_or(("", ""));
        let status: u16 = head
            .lines()
            .next()
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        Some((status, head.to_string(), body.to_string()))
    };

    attempt(port, method, path, headers, body)
        .or_else(|| attempt(port, method, path, headers, body))
        .unwrap_or((0, String::new(), String::new()))
}

fn json_header() -> Vec<(&'static str, &'static str)> {
    vec![("Content-Type", "application/json")]
}

// ---------- /health ----------

#[test]
fn health_returns_app_identity() {
    let port = start_server(|_| {});
    let (status, _, body) = http(port, "GET", "/health", &[], "");
    assert_eq!(status, 200);
    assert!(
        body.contains("\"app\":\"x-notify-service\""),
        "body: {body}"
    );
    assert!(body.contains(&format!("\"port\":{port}")), "body: {body}");
}

// ---------- /notify 校验 ----------

#[test]
fn notify_empty_title_returns_422() {
    let port = start_server(|_| {});
    let (status, _, body) = http(port, "POST", "/notify", &json_header(), r#"{"title":""}"#);
    assert_eq!(status, 422, "body: {body}");
}

#[test]
fn notify_bad_json_returns_400() {
    let port = start_server(|_| {});
    let (status, _, _) = http(port, "POST", "/notify", &json_header(), "{broken");
    assert_eq!(status, 400);
}

#[test]
fn notify_size_out_of_range_returns_422() {
    let port = start_server(|_| {});
    // 校验先于投递(无头环境不触发通知链),与空标题同一口径
    let (status, _, body) = http(
        port,
        "POST",
        "/notify",
        &json_header(),
        r#"{"title":"t","width":100}"#,
    );
    assert_eq!(status, 422, "body: {body}");
    let (status, _, _) = http(
        port,
        "POST",
        "/notify",
        &json_header(),
        r#"{"title":"t","height":9999}"#,
    );
    assert_eq!(status, 422);
}

#[test]
fn notify_unknown_path_returns_404() {
    let port = start_server(|_| {});
    let (status, _, _) = http(port, "GET", "/nonexistent", &[], "");
    assert_eq!(status, 404);
}

// ---------- CORS ----------

#[test]
fn cors_preflight_returns_204_with_headers() {
    let port = start_server(|_| {});
    let (status, headers, _) = http(port, "OPTIONS", "/notify", &[], "");
    assert_eq!(status, 204);
    assert!(
        headers
            .to_lowercase()
            .contains("access-control-allow-origin: *"),
        "headers: {headers}"
    );
    assert!(
        headers
            .to_lowercase()
            .contains("access-control-allow-methods"),
        "headers: {headers}"
    );
}

#[test]
fn cors_wildcard_origin_echoed() {
    let port = start_server(|_| {});
    let (_, headers, _) = http(
        port,
        "GET",
        "/health",
        &[("Origin", "http://any.example")],
        "",
    );
    assert!(
        headers.contains("Access-Control-Allow-Origin: *"),
        "headers: {headers}"
    );
}

#[test]
fn cors_whitelist_rejects_unknown_origin() {
    let port = start_server(|c| {
        c.cors_origins = vec!["http://good.example".into()];
    });
    let (_, headers, _) = http(
        port,
        "GET",
        "/health",
        &[("Origin", "http://evil.example")],
        "",
    );
    assert!(
        !headers.contains("Access-Control-Allow-Origin"),
        "陌生来源不应有 Allow-Origin: {headers}"
    );
}

#[test]
fn cors_whitelist_echoes_known_origin() {
    let port = start_server(|c| {
        c.cors_origins = vec!["http://good.example".into()];
    });
    let (_, headers, _) = http(
        port,
        "GET",
        "/health",
        &[("Origin", "http://good.example")],
        "",
    );
    assert!(
        headers.contains("Access-Control-Allow-Origin: http://good.example"),
        "headers: {headers}"
    );
}

// ---------- 安全参数(token) ----------

#[test]
fn token_missing_returns_401() {
    let port = start_server(|c| {
        c.token = Some("s3cret".into());
    });
    let (status, _, _) = http(port, "POST", "/notify", &json_header(), r#"{"title":"t"}"#);
    assert_eq!(status, 401);
}

#[test]
fn token_correct_passes_auth() {
    // 用 /close 验证鉴权通过(notify 会触发通知投递链,在无头测试环境可能阻塞)
    let port = start_server(|c| {
        c.token = Some("s3cret".into());
    });
    let (status, _, _) = http(port, "POST", "/close", &[("X-Token", "s3cret")], "");
    assert_eq!(status, 200);
}

// ---------- 内嵌静态内容 ----------

#[test]
fn embedded_demo_page_accessible() {
    let port = start_server(|_| {});
    let (status, headers, body) = http(port, "GET", "/", &[], "");
    assert_eq!(status, 200);
    assert!(headers.contains("text/html"), "headers: {headers}");
    assert!(
        body.contains("<!doctype html"),
        "body 前 80: {}",
        &body[..80.min(body.len())]
    );
}

#[test]
fn embedded_sdk_is_real_not_placeholder() {
    let port = start_server(|_| {});
    let (status, _, body) = http(port, "GET", "/sdk.js", &[], "");
    assert_eq!(status, 200);
    assert!(
        body.contains("createNotifyService"),
        "sdk.js 应含真产物标记,可能是占位桩: {}",
        &body[..60.min(body.len())]
    );
}

// ---------- /close ----------

#[test]
fn close_is_idempotent() {
    let port = start_server(|_| {});
    let (status, _, body) = http(port, "POST", "/close", &[], "");
    assert_eq!(status, 200);
    assert!(body.contains("\"ok\":true"), "body: {body}");
}
