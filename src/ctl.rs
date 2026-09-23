//! 服务生命周期控制(start/stop/restart 子命令)。
//! 只面向本机终端;不提供 HTTP 停止接口——服务无鉴权,任意网页可停是安全隐患。

#![allow(clippy::print_stdout)]

use crate::api;
use crate::single;

/// /health 探测结果:Ours = 我们的实例,Foreign = 端口被其他程序占用
pub enum Probe {
    Ours { version: String },
    Foreign,
}

/// 直连 GET /health(300ms 超时);None = 连不上,Some = 有服务在听
pub fn probe_port(port: u16) -> Option<Probe> {
    let body = request(port, "GET", "/health", "")?;
    let json: serde_json::Value = serde_json::from_str(body.trim()).ok()?;
    if json["app"] == api::APP_ID {
        Some(Probe::Ours {
            version: json["version"].as_str().unwrap_or("?").to_string(),
        })
    } else {
        Some(Probe::Foreign)
    }
}

/// 向本机服务发一次极简 HTTP 请求,返回响应 body;连不上返回 None
pub fn request(port: u16, method: &str, path: &str, body: &str) -> Option<String> {
    use std::io::{Read as _, Write as _};
    let mut conn = std::net::TcpStream::connect(("127.0.0.1", port)).ok()?;
    let _ = conn.set_read_timeout(Some(std::time::Duration::from_millis(300)));
    let _ = conn.set_write_timeout(Some(std::time::Duration::from_millis(300)));
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    conn.write_all(req.as_bytes()).ok()?;
    let mut buf = String::new();
    conn.read_to_string(&mut buf).ok()?;
    buf.split_once("\r\n\r\n").map(|(_, b)| b.to_string())
}

/// start 子命令:已在运行则幂等提示,否则分离启动并等就绪
pub fn start() {
    if let Some((port, Probe::Ours { .. })) = recorded_ours() {
        println!("服务已在运行(端口 {port})");
        return;
    }
    start_detached();
    for _ in 0u8..30 {
        if let Some((port, Probe::Ours { .. })) = recorded_ours() {
            println!("服务已启动(端口 {port})");
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    println!("服务进程已拉起(就绪探测未及,可用 info 查看)");
}

/// stop 子命令:幂等;/health 确认是我们的实例后,才 kill 端口文件里的 pid
pub fn stop() {
    let Some(rec) = single::read_port_file() else {
        println!("服务未在运行");
        return;
    };
    match probe_port(rec.port) {
        Some(Probe::Ours { .. }) => {
            kill(rec.pid);
            wait_down(rec.port);
            single::remove_port_file();
            println!("服务已停止(pid {})", rec.pid);
        }
        Some(Probe::Foreign) => println!("端口 {} 被其他程序占用,未执行停止", rec.port),
        None => {
            single::remove_port_file();
            println!("服务未在运行(已清理残留端口文件)");
        }
    }
}

/// restart 子命令:停止(容忍未运行)后启动
pub fn restart() {
    stop();
    start();
}

/// 端口文件记录的实例确实是我们且在应答
fn recorded_ours() -> Option<(u16, Probe)> {
    let rec = single::read_port_file()?;
    let probe = probe_port(rec.port)?;
    matches!(probe, Probe::Ours { .. }).then_some((rec.port, probe))
}

/// 分离启动服务进程(以 serve 子命令重新拉起自身);install 完成时同样走这里
pub fn start_detached() {
    let Ok(exe) = std::env::current_exe() else {
        log::warn!("无法定位自身路径,跳过后台启动");
        return;
    };
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("serve")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        // DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP:无控制台窗口、独立进程组
        cmd.creation_flags(0x0000_0008 | 0x0000_0200);
    }
    match cmd.spawn() {
        Ok(child) => {
            log::info!("服务进程已分离启动(pid {})", child.id());
            // 丢弃句柄,让子进程完全独立
            drop(child);
        }
        Err(e) => log::warn!("后台启动服务失败: {e}(可手动运行 x-notify-service)"),
    }
}

fn kill(pid: u32) {
    #[cfg(windows)]
    let _ = std::process::Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/F"])
        .status();
    #[cfg(not(windows))]
    let _ = std::process::Command::new("kill")
        .arg(pid.to_string())
        .status();
}

/// 等端口上的服务下线(最多 ~2s)
fn wait_down(port: u16) {
    for _ in 0u8..20 {
        if probe_port(port).is_none() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}
