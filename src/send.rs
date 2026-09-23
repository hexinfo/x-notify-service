//! `notify` 子命令:本机发一条通知——安装前手测弹窗/兜底用。
//! 服务在运行时经其 HTTP 通道投递(与浏览器/SDK 同路径),CLI 立即返回;
//! 未运行时本进程弹窗(窗口需事件循环驻留,点击关闭后进程退出);
//! `-f` 恒走本机系统通知(立即返回);`close` 子命令关闭当前弹窗(幂等)。

// CLI 子命令:进程退出码即结果语义,打印直连终端
#![allow(clippy::print_stdout, clippy::print_stderr, clippy::exit)]

use crate::config::Config;
use crate::notify;
use crate::notify::Presenter as _;

pub fn run(cfg: &Config, req: &crate::api::NotifyRequest, fallback: bool) {
    if let Err(e) = req.validate() {
        eprintln!("通知内容不合法: {e}");
        std::process::exit(2);
    }
    let title = req.title.trim().to_string();
    let body_markdown = req.body.clone().unwrap_or_default();
    // 弹窗尺寸解析:CLI 参数 > 默认(与服务端同口径;范围已随 req.validate 校验)
    let size = notify::popup::resolve_size(req.width, req.height);
    let colors = notify::popup::resolve_colors(
        req.header_background_color.as_deref(),
        req.header_text_color.as_deref(),
        req.body_background_color.as_deref(),
        req.body_text_color.as_deref(),
    );

    // 服务在运行:走 HTTP 通道(弹窗归服务持有,CLI 立即返回)
    if !fallback
        && let Some(rec) = crate::single::read_port_file()
        && matches!(
            crate::ctl::probe_port(rec.port),
            Some(crate::ctl::Probe::Ours { .. })
        )
    {
        deliver_via_service(rec.port, &title, &body_markdown, req);
        return;
    }

    if fallback || cfg.no_popup {
        let presenter = notify::fallback::SystemPresenter::new(cfg);
        if presenter.present(&title, &body_markdown, size, colors) {
            println!("已发送系统通知(via=system)");
        } else {
            eprintln!("系统通知发送失败(详见日志)");
            std::process::exit(1);
        }
        return;
    }

    println!("弹窗已显示(无运行中服务,本进程驻留至点击关闭)");
    // 单发模式:daemon 预置本条通知,弹窗关闭后事件循环退出、进程结束
    let payload = notify::view::PopupPayload {
        title: title.clone(),
        body_markdown: body_markdown.clone(),
        size,
        colors,
        quit_on_close: true,
    };
    if let Err(e) = notify::app::run_single(payload) {
        eprintln!("GPUI 弹窗不可用,降级系统通知: {e}");
        let presenter = notify::fallback::SystemPresenter::new(cfg);
        if !presenter.present(&title, &body_markdown, size, colors) {
            std::process::exit(1);
        }
    }
}

/// 经运行中服务的 /notify 投递(与浏览器/SDK 完全同路径)
fn deliver_via_service(
    port: u16,
    title: &str,
    body_markdown: &str,
    req: &crate::api::NotifyRequest,
) {
    let mut payload = serde_json::json!({ "title": title, "body": body_markdown });
    if let Some(w) = req.width {
        payload["width"] = w.into();
    }
    if let Some(h) = req.height {
        payload["height"] = h.into();
    }
    for (key, value) in [
        (
            "headerBackgroundColor",
            req.header_background_color.as_ref(),
        ),
        ("headerTextColor", req.header_text_color.as_ref()),
        ("bodyBackgroundColor", req.body_background_color.as_ref()),
        ("bodyTextColor", req.body_text_color.as_ref()),
    ] {
        if let Some(value) = value {
            payload[key] = value.clone().into();
        }
    }
    match crate::ctl::request(port, "POST", "/notify", &payload.to_string()) {
        Some(resp) if resp.contains("\"ok\":true") => {
            let via = serde_json::from_str::<serde_json::Value>(&resp)
                .ok()
                .and_then(|v| v["via"].as_str().map(str::to_string))
                .unwrap_or_else(|| "?".into());
            println!("已投递(via={via},经运行中服务)");
        }
        Some(resp) => {
            eprintln!("服务拒绝投递: {resp}");
            std::process::exit(1);
        }
        None => {
            eprintln!("服务连接失败");
            std::process::exit(1);
        }
    }
}

/// close 子命令:服务未运行时幂等成功
pub fn close() {
    let Some(rec) = crate::single::read_port_file() else {
        println!("服务未在运行,无弹窗可关");
        return;
    };
    match crate::ctl::request(rec.port, "POST", "/close", "") {
        Some(body) if body.contains("\"ok\":true") => println!("已关闭当前弹窗"),
        Some(_) => println!("服务已应答但未确认关闭(详见服务日志)"),
        None => println!("服务未在运行,无弹窗可关"),
    }
}
