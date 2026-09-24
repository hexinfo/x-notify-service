// Windows 始终使用 GUI 子系统:服务和弹窗不创建控制台窗口。
#![cfg_attr(windows, windows_subsystem = "windows")]

mod api;
mod autostart;
mod config;
mod ctl;
mod info;
mod install;
mod logging;
mod markdown_body;
mod notify;
mod protocol;
mod screen;
mod send;
mod server;
mod single;
#[cfg(test)]
mod tests_http;
mod windows_env;

use clap::Parser as _;

#[allow(clippy::print_stderr)]
fn main() {
    windows_env::attach_parent_console();
    let cli = config::Cli::parse();
    let cfg = config::resolve(&cli);
    // 文件日志仅服务进程需要;一次性 CLI 命令不建文件(避免空日志),输出走终端
    let serve_mode = match &cli.cmd {
        None => cli.url_arg.is_some(),
        Some(config::Command::Serve) => true,
        Some(_) => false,
    };
    let _logger = serve_mode.then(|| logging::init(&cfg));
    if serve_mode {
        // panic 默认走 stderr,分离启动的服务 stderr 已丢弃;
        // 挂钩把 panic 记入日志文件,真机故障可查(UOS 闪退事故教训)
        std::panic::set_hook(Box::new(|info| {
            log::error!("panic: {info}");
        }));
    }

    match cli.cmd {
        // 无参数:显示帮助;协议拉起(url 参数)时仍直接进入服务(单例语义)
        None => {
            if cli.url_arg.is_some() {
                serve(&cfg);
            } else {
                use clap::CommandFactory as _;
                let _ = config::Cli::command().print_help();
            }
        }
        Some(config::Command::Serve) => serve(&cfg),
        Some(config::Command::Install) => {
            // 注册 + 分离启动服务后立即退出,供安装器/脚本调用不阻塞
            if let Err(e) = install::install() {
                eprintln!("安装失败: {e}");
                std::process::exit(1);
            }
        }
        Some(config::Command::Uninstall) => {
            install::uninstall();
        }
        Some(config::Command::Info) => {
            info::run(&cfg);
        }
        Some(config::Command::Start) => ctl::start(),
        Some(config::Command::Stop) => ctl::stop(),
        Some(config::Command::Restart) => ctl::restart(),
        Some(config::Command::Notify {
            title,
            body,
            width,
            height,
            header_background_color,
            header_text_color,
            body_background_color,
            body_text_color,
            fallback,
        }) => {
            let req = api::NotifyRequest {
                title,
                body,
                width,
                height,
                header_background_color,
                header_text_color,
                body_background_color,
                body_text_color,
            };
            send::run(&cfg, &req, fallback);
        }
        Some(config::Command::Close) => send::close(),
    }
}

/// 服务主流程:单实例 → GUI 探测(先于 HTTP,避免启动早期请求降级)→ 绑端口 → 事件循环
fn serve(cfg: &config::Config) {
    log::info!(
        "x-notify-service {} 启动(默认端口 {},日志目录 {})",
        api::VERSION,
        cfg.port,
        cfg.log_dir.display()
    );

    if !single::acquire_lock() {
        log::info!("已有实例在运行,本进程静默退出");
        return;
    }

    // 启动前探测显示环境；iced 事件循环尚未就绪时请求会走系统通知。
    let gui_ok = !cfg.no_popup && notify::popup::gui_probe();
    notify::POPUP_AVAILABLE.store(gui_ok, std::sync::atomic::Ordering::Relaxed);
    if !gui_ok {
        if cfg.no_popup {
            log::info!("--no-popup:通知全部走系统通知");
        } else {
            log::warn!("弹窗不可用,通知将走系统通知兜底");
        }
    }

    let port = server::start(cfg.clone());
    single::write_port_file(port);
    log::info!("服务已就绪: http://127.0.0.1:{port}");

    if !gui_ok {
        // 无 GUI 模式:主线程挂起,HTTP 工作线程继续服务
        #[allow(clippy::infinite_loop)]
        loop {
            std::thread::park();
        }
    }

    // daemon:弹窗窗口关闭不会结束事件循环(服务常驻语义)
    if let Err(e) = notify::app::run_service() {
        log::error!("GUI 事件循环异常退出: {e}");
        // 通道已断,后续通知转系统通知兜底
        notify::POPUP_AVAILABLE.store(false, std::sync::atomic::Ordering::Relaxed);
        #[allow(clippy::infinite_loop)]
        loop {
            std::thread::park();
        }
    }
}
