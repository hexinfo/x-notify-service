// Windows 始终使用 GUI 子系统:服务、自启动与双击均不创建控制台窗口；
// CLI 从现有终端运行时由 windows_env 主动附着父控制台。
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
            install::install();
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

/// 服务主流程:单实例 → 绑端口 → GPUI 事件循环(失败后保持系统通知服务)
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

    notify::POPUP_AVAILABLE.store(false, std::sync::atomic::Ordering::Release);

    let port = server::start(cfg.clone());
    single::write_port_file(port);
    log::info!("服务已就绪: http://127.0.0.1:{port}");

    if cfg.no_popup {
        log::info!("--no-popup:通知全部走系统通知");
        park_system_service();
    }

    // QuitMode::Explicit:弹窗窗口关闭不会结束事件循环(服务常驻语义)
    record_gui_exit(notify::app::run_service());
    park_system_service();
}

fn record_gui_exit(result: Result<(), notify::app::AppError>) {
    // 无论 GPUI 正常或异常退出，HTTP 工作线程都继续提供系统通知兜底。
    notify::POPUP_AVAILABLE.store(false, std::sync::atomic::Ordering::Release);
    match result {
        Ok(()) => log::warn!("GPUI 事件循环已退出,服务继续使用系统通知"),
        Err(error) => log::error!("GPUI 事件循环异常退出: {error}"),
    }
}

#[allow(clippy::infinite_loop)]
fn park_system_service() -> ! {
    loop {
        std::thread::park();
    }
}

#[cfg(test)]
mod lifecycle_tests {
    use super::record_gui_exit;

    #[test]
    fn windows_subsystem_is_gui_in_all_builds() {
        assert!(
            include_str!("main.rs").contains("cfg_attr(windows, windows_subsystem = \"windows\")")
        );
    }

    #[test]
    fn normal_gui_exit_disables_popup_channel() {
        crate::notify::POPUP_AVAILABLE.store(true, std::sync::atomic::Ordering::Release);
        record_gui_exit(Ok(()));
        assert!(!crate::notify::POPUP_AVAILABLE.load(std::sync::atomic::Ordering::Acquire));
    }

    #[test]
    fn failed_gui_exit_disables_popup_channel() {
        crate::notify::POPUP_AVAILABLE.store(true, std::sync::atomic::Ordering::Release);
        record_gui_exit(Err(crate::notify::app::AppError::PlatformInit(
            "test".into(),
        )));
        assert!(!crate::notify::POPUP_AVAILABLE.load(std::sync::atomic::Ordering::Acquire));
    }
}
