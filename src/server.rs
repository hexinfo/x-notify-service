use std::io::Read as _;
use std::sync::Arc;

use tiny_http::{Header, Method, Request, Response, Server};

use crate::api::{self, ErrorResponse, HealthResponse, NotifyResponse};

/// 演示页随二进制内嵌:装完浏览器打开 http://127.0.0.1:{port}/ 即测,无需找文件
const DEMO_HTML: &str = include_str!("../assets/demo.html");

use crate::config::Config;
use crate::notify;

const MAX_BODY: usize = 64 * 1024;
/// 单 HTTP 工作线程:进程模型保持最简(主线程 GUI 事件循环 + 1 个 HTTP 线程),
/// 本地单用户场景串行处理足够
const WORKERS: usize = 1;

// 入参均为 ASCII 字面量,from_bytes 实际不可失败
#[allow(clippy::expect_used)]
fn header(k: &str, v: &str) -> Header {
    Header::from_bytes(k.as_bytes(), v.as_bytes()).expect("非法响应头")
}

/// 按配置与请求 Origin 计算 CORS 头:
/// 白名单含 "*" → 全开;否则精确匹配请求 Origin 并回显;不匹配则不带
/// Allow-Origin(浏览器将拦截响应读取,即拒绝该来源)。
fn cors_headers(cfg: &Config, origin: Option<&str>) -> Vec<Header> {
    let mut headers = Vec::with_capacity(5);
    let allow = if cfg.cors_origins.iter().any(|o| o == "*") {
        Some("*".to_string())
    } else {
        origin
            .filter(|o| cfg.cors_origins.iter().any(|allowed| allowed == o))
            .map(str::to_string)
    };
    if let Some(allow) = allow {
        headers.push(header("Access-Control-Allow-Origin", &allow));
    }
    headers.push(header("Access-Control-Allow-Methods", "GET, POST, OPTIONS"));
    let mut allow_headers = "Content-Type".to_string();
    if cfg.token.is_some() {
        allow_headers.push_str(", X-Token");
    }
    headers.push(header("Access-Control-Allow-Headers", &allow_headers));
    if cfg.allow_private_network {
        // 新版 Chrome Local Network Access 预检需要
        headers.push(header("Access-Control-Allow-Private-Network", "true"));
    }
    headers.push(header("Access-Control-Max-Age", "86400"));
    headers
}

/// 鉴权:配置了 token 时要求 X-Token 头精确匹配;未配置恒通过(默认无鉴权)
fn authorized(cfg: &Config, req: &Request) -> bool {
    match &cfg.token {
        None => true,
        Some(expected) => req
            .headers()
            .iter()
            .any(|h| h.field.equiv("X-Token") && h.value.as_str() == expected),
    }
}

fn request_origin(req: &Request) -> Option<String> {
    req.headers()
        .iter()
        .find(|h| h.field.equiv("Origin"))
        .map(|h| h.value.as_str().to_string())
}

/// 内嵌静态内容(演示页与 SDK,公开只读,无鉴权)。
// no-store:升级服务后浏览器立刻拿到新页面/新 SDK,杜绝旧缓存的静默行为差
fn serve_static(req: Request, cors: Vec<Header>, content_type: &str, body: &str) {
    let mut response = Response::from_string(body)
        .with_status_code(200)
        .with_header(header("Content-Type", content_type))
        .with_header(header("Cache-Control", "no-store"));
    for h in cors {
        response = response.with_header(h);
    }
    let _ = req.respond(response);
}

fn send_json(req: Request, cors: Vec<Header>, status: u16, body: String) {
    let mut response = Response::from_string(body)
        .with_status_code(status)
        .with_header(header("Content-Type", "application/json; charset=utf-8"));
    for h in cors {
        response = response.with_header(h);
    }
    let _ = req.respond(response);
}

fn send_error(req: Request, cors: Vec<Header>, err: &api::NotifyError) {
    let body = serde_json::to_string(&ErrorResponse {
        ok: false,
        error: err.to_string(),
    })
    .unwrap_or_else(|_| r#"{"ok":false,"error":"internal"}"#.into());
    send_json(req, cors, err.status(), body);
}

/// 绑定端口并启动 HTTP 服务:默认端口被占依次向后探测 10 个;
/// 全部被占则绑定随机端口(此时浏览器无法发现,仅系统通知可用)。
/// 返回实际端口。
// 工作线程启动失败属致命错误,panic 即失败退出
#[allow(clippy::expect_used)]
pub fn start(cfg: Config) -> u16 {
    let (server, used_port) = bind_with_fallback(cfg.port);
    let server = Arc::new(server);
    // 运行期共享状态:/health 报告实际监听端口(可能与配置端口不同)
    let state = Arc::new(Runtime {
        cfg,
        port: used_port,
    });
    for i in 0..WORKERS {
        let server = Arc::clone(&server);
        let state = Arc::clone(&state);
        std::thread::Builder::new()
            .name(format!("http-{i}"))
            .spawn(move || {
                loop {
                    match server.recv() {
                        Ok(req) => handle(&state, req),
                        Err(e) => {
                            log::error!("接收请求失败: {e}");
                            return;
                        }
                    }
                }
            })
            .expect("启动 HTTP 工作线程失败");
    }
    used_port
}

/// 绑定 127.0.0.1:默认端口起向后探测 10 个;全被占则让 OS 随机分配
/// (此时浏览器无法发现服务,仅系统通知可用)。
// 连随机端口都绑定失败属环境级故障,panic 即失败退出
#[allow(clippy::expect_used)]
fn bind_with_fallback(default_port: u16) -> (Server, u16) {
    for port in default_port..default_port.saturating_add(10) {
        if let Ok(server) = Server::http(("127.0.0.1", port)) {
            if port != default_port {
                log::warn!("默认端口 {default_port} 被占用,实际监听 {port}");
            }
            return (server, port);
        }
    }
    // 先经 TcpListener 拿到 OS 分配的空闲端口再交给 tiny_http
    let server = std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .map_err(|e| e.to_string())
        .and_then(|a| Server::http(("127.0.0.1", a.port())).map_err(|e| e.to_string()))
        .expect("绑定随机端口失败");
    let port = server.server_addr().to_ip().map_or(0, |a| a.port());
    log::warn!(
        "端口 {}~{} 全部被占用,已绑定随机端口 {port},浏览器将无法自动发现服务",
        default_port,
        default_port + 9
    );
    (server, port)
}

/// HTTP 工作线程共享的运行期状态
struct Runtime {
    cfg: Config,
    port: u16,
}

fn handle(rt: &Runtime, req: Request) {
    let Runtime { cfg, port } = rt;
    let method = req.method().clone();
    let path = req.url().split('?').next().unwrap_or("/").to_string();
    let cors = cors_headers(cfg, request_origin(&req).as_deref());

    if method == Method::Options {
        let mut response = Response::empty(204u16);
        for h in cors {
            response = response.with_header(h);
        }
        let _ = req.respond(response);
        return;
    }

    match (method, path.as_str()) {
        (Method::Get, "/") => serve_static(req, cors, "text/html; charset=utf-8", DEMO_HTML),
        (Method::Get, "/sdk.js") => serve_static(
            req,
            cors,
            "text/javascript; charset=utf-8",
            // SDK 产物由 build.rs 复制到 OUT_DIR(纯 cargo build 时为占位)
            include_str!(concat!(env!("OUT_DIR"), "/embedded-sdk.js")),
        ),
        (Method::Get, "/health") => {
            // /health 保持开放:仅身份与端口,供 SDK 探测;无敏感动作
            let body = HealthResponse {
                app: api::APP_ID,
                version: api::VERSION,
                port: *port,
            };
            let _ = serde_json::to_string(&body).map(|b| send_json(req, cors, 200, b));
        }
        (Method::Post, "/notify") if authorized(cfg, &req) => handle_notify(cfg, cors, req),
        (Method::Post, "/close") if authorized(cfg, &req) => {
            // 显式关闭当前弹窗(幂等);经 bridge 通道投递到 GUI 线程
            if !notify::app::post(notify::app::Message::Close) {
                log::warn!("关闭弹窗投递失败(事件循环未就绪)");
            }
            send_json(req, cors, 200, r#"{"ok":true}"#.into());
        }
        (Method::Post, "/notify" | "/close") => {
            log::warn!("鉴权失败(X-Token 不匹配或缺失): {path}");
            send_json(
                req,
                cors,
                401,
                r#"{"ok":false,"error":"unauthorized"}"#.into(),
            );
        }
        _ => {
            let body = serde_json::to_string(&ErrorResponse {
                ok: false,
                error: "not found".into(),
            })
            .unwrap_or_default();
            send_json(req, cors, 404, body);
        }
    }
}

fn handle_notify(cfg: &Config, cors: Vec<Header>, mut req: Request) {
    let mut body = String::new();
    let mut reader = req.as_reader().take(MAX_BODY as u64 + 1);
    if reader.read_to_string(&mut body).is_err() {
        send_error(
            req,
            cors,
            &api::NotifyError::BadJson("请求体不是有效的 UTF-8 文本".into()),
        );
        return;
    }
    if body.len() > MAX_BODY {
        send_error(req, cors, &api::NotifyError::BadJson("请求体过大".into()));
        return;
    }
    // 解析+校验语义归 api 模块;这里只做传输层(读体/限长/响应)
    let parsed = match api::NotifyRequest::from_json(&body).and_then(|r| r.validate().map(|()| r)) {
        Ok(r) => r,
        Err(err) => {
            send_error(req, cors, &err);
            return;
        }
    };

    let via = notify::dispatch(cfg, &parsed);
    let resp = NotifyResponse { ok: true, via };
    log::info!(
        "通知已投递(via={}): {}",
        via.as_str(),
        parsed.title.chars().take(50).collect::<String>()
    );
    match serde_json::to_string(&resp) {
        Ok(b) => send_json(req, cors, 200, b),
        Err(_) => send_error(
            req,
            cors,
            &api::NotifyError::BadJson("响应序列化失败".into()),
        ),
    }
}
