//! `info` 子命令:一次性输出诊断快照,排查"服务没起/弹窗没出现/位置不对"类问题。
//! 全部只读探测:实例与端口、显示环境、GUI/工作区(含弹窗理论落点)、
//! 配置与日志路径、自启动与协议注册状态。

#![allow(clippy::print_stdout)]

use crate::{api, autostart, config, ctl, notify, protocol, screen, single};

pub fn run(cfg: &config::Config) {
    println!(
        "x-notify-service {} ({} / {})",
        api::VERSION,
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    instance(cfg);
    display(cfg);
    paths(cfg);
    registrations();
}

/// 运行实例:端口文件记录 → /health 身份校验;辅以单例锁状态
fn instance(cfg: &config::Config) {
    let recorded = single::read_port_file();
    let port = recorded.as_ref().map_or(cfg.port, |r| r.port);
    match ctl::probe_port(port) {
        Some(ctl::Probe::Ours { version }) => {
            let pid = recorded
                .as_ref()
                .map_or_else(String::new, |r| format!(", pid {}", r.pid));
            println!("运行实例: 运行中(端口 {port}, 服务版本 {version}{pid})");
        }
        Some(ctl::Probe::Foreign) => {
            println!("运行实例: 端口 {port} 被其他程序占用(身份校验不符)");
        }
        None => {
            if recorded.is_some() {
                println!("运行实例: 未响应(端口文件记录 {port},但 /health 不通,进程可能已退出)");
            } else {
                println!("运行实例: 未发现(无端口文件,/health {port} 不通)");
            }
        }
    }
    println!(
        "单例锁:   {}",
        if single::is_locked() {
            "被持有(已有实例)"
        } else {
            "空闲"
        }
    );
}

/// 显示环境 + GUI 探测 + 工作区与弹窗理论落点(与弹窗定位共用同一计算)
fn display(cfg: &config::Config) {
    let disp = std::env::var_os("DISPLAY")
        .map_or_else(|| "未设置".into(), |v| v.to_string_lossy().into_owned());
    let way = std::env::var_os("WAYLAND_DISPLAY")
        .map_or_else(|| "未设置".into(), |v| v.to_string_lossy().into_owned());
    println!("显示环境: DISPLAY={disp}  WAYLAND_DISPLAY={way}");
    if cfg.no_popup {
        println!("弹窗配置: no_popup=true,通知强制走系统通知");
    }
    if !notify::popup::gui_probe() {
        println!("GUI 探测: 不可用(弹窗无法创建,通知将走系统通知兜底)");
        println!("工作区:   未探测(弹窗不可用,无需工作区)");
        return;
    }
    println!("GUI 探测: 可用(弹窗窗口可创建)");
    let size = notify::popup::resolve_size(None, None);
    match screen::work_area() {
        Some(area) => {
            let (x, y) = notify::popup::landing(&area, size);
            println!(
                "工作区:   ({},{},{}×{}) scale={} → 弹窗落点 ({x},{y}),尺寸 {}×{}",
                area.x, area.y, area.w, area.h, area.scale, size.width, size.height
            );
        }
        None => println!("工作区:   无法获取(Wayland 会话或无可用 X 屏幕)"),
    }
}

fn paths(cfg: &config::Config) {
    match config::config_in_use() {
        Some(p) => println!("配置文件: {}", p.display()),
        None => println!("配置文件: 未找到(使用默认值)"),
    }
    let logs = std::fs::read_dir(&cfg.log_dir)
        .map_or(0, |d| d.filter_map(std::result::Result::ok).count());
    println!("日志目录: {}({logs} 个文件)", cfg.log_dir.display());
    println!("数据目录: {}", single::data_dir().display());
    let cors = if cfg.cors_origins.iter().any(|o| o == "*") {
        "全开(*)".to_string()
    } else {
        format!("白名单 {} 项", cfg.cors_origins.len())
    };
    let auth = if cfg.token.is_some() {
        "已启用(X-Token)"
    } else {
        "未启用(默认)"
    };
    println!("安全配置: CORS={cors}, 鉴权={auth}");
}

fn registrations() {
    match autostart::is_enabled() {
        Ok(true) => println!("开机自启: 已注册"),
        Ok(false) => println!("开机自启: 未注册"),
        Err(e) => println!("开机自启: 检测失败({e})"),
    }
    println!(
        "协议注册: {}:// {}",
        protocol::SCHEME,
        if protocol::is_registered() {
            "已注册"
        } else {
            "未注册"
        }
    );
}
