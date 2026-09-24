// install/uninstall 为 CLI 用户交互命令,输出直连终端(此时可能无日志文件)
#![allow(clippy::print_stdout, clippy::print_stderr)]

use crate::{autostart, ctl, protocol};

/// install 子命令:注册自启动 + x-notify:// 协议(失败仅告警,不阻塞),
/// 随后分离启动服务进程并立即返回,供安装器/脚本调用不阻塞。
pub fn install() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(target_os = "macos")]
    if !old_macos_instance_stopped() {
        return Err("无法确认旧版服务已停止;请先停止旧版服务".into());
    }
    autostart::enable()?;
    log::info!("已注册开机自启动");
    protocol::register()?;
    log::info!("已注册 {}:// 协议", protocol::SCHEME);
    #[cfg(windows)]
    crate::windows_env::set_user_path(true);
    let expected_pid = if let Some(rec) = crate::single::read_port_file()
        && new_service_ready(rec.pid)
    {
        rec.pid
    } else {
        ctl::start_detached().ok_or("后台启动服务失败")?
    };
    let ready = (0..30).any(|_| {
        if new_service_ready(expected_pid) {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
        false
    });
    if !ready {
        return Err("新版服务未在限定时间内就绪".into());
    }
    #[cfg(target_os = "macos")]
    cleanup_old_macos_dirs_after_start();
    println!("安装完成,服务已在后台启动");
    Ok(())
}

fn new_service_ready(expected_pid: u32) -> bool {
    let Some(rec) = crate::single::read_port_file() else {
        return false;
    };
    expected_service(expected_pid, &rec, ctl::probe_port(rec.port))
}

fn expected_service(
    expected_pid: u32,
    rec: &crate::single::InstanceInfo,
    probe: Option<ctl::Probe>,
) -> bool {
    rec.pid == expected_pid
        && matches!(probe, Some(ctl::Probe::Ours { version }) if version == env!("CARGO_PKG_VERSION"))
}

#[cfg(target_os = "macos")]
fn old_macos_dirs() -> Option<(std::path::PathBuf, std::path::PathBuf)> {
    let home = dirs::home_dir()?;
    let old_data = home
        .join("Library/Application Support")
        .join(crate::config::APP_DIR_NAME);
    let old_logs = home.join("Library/Logs").join(crate::config::APP_DIR_NAME);
    Some((old_data, old_logs))
}

#[cfg(target_os = "macos")]
fn old_macos_instance_stopped() -> bool {
    use std::fs::OpenOptions;

    let Some((old_data, old_logs)) = old_macos_dirs() else {
        return false;
    };
    if !old_data.exists() && !old_logs.exists() {
        return true;
    }
    let Ok(lock_file) = OpenOptions::new()
        .read(true)
        .write(true)
        .open(old_data.join("instance.lock"))
    else {
        return false;
    };
    let mut lock = fd_lock::RwLock::new(lock_file);
    lock.try_write().is_ok()
}

#[cfg(target_os = "macos")]
fn cleanup_old_macos_dirs_after_start() {
    use std::fs::remove_dir_all;

    let Some((old_data, old_logs)) = old_macos_dirs() else {
        return;
    };
    if !old_data.exists() && !old_logs.exists() {
        return;
    }

    if !old_macos_instance_stopped() {
        log::warn!("无法确认旧版实例已停止,保留旧版目录");
        return;
    }

    for path in [&old_data, &old_logs] {
        if path.exists() {
            match remove_dir_all(path) {
                Ok(()) => log::info!("已清理旧版目录: {}", path.display()),
                Err(e) => log::warn!("清理旧版目录 {} 失败: {e}", path.display()),
            }
        }
    }
}

/// uninstall 子命令:先停止运行中的服务,再清理全部注册项,
/// 避免留下「还在跑但不再自启」的半卸载状态
pub fn uninstall() {
    #[cfg(windows)]
    crate::windows_env::set_user_path(false);
    ctl::stop();
    match autostart::disable() {
        Ok(()) => println!("已移除开机自启动"),
        Err(e) => eprintln!("移除自启动失败: {e}"),
    }
    match protocol::unregister() {
        Ok(()) => println!("已注销 {}:// 协议", protocol::SCHEME),
        Err(e) => eprintln!("注销 {}:// 协议失败: {e}", protocol::SCHEME),
    }
    println!("卸载完成(二进制与日志保留,可手动删除)");
}

#[cfg(test)]
mod tests {
    use super::expected_service;
    use crate::{ctl::Probe, single::InstanceInfo};

    #[test]
    fn install_readiness_requires_matching_pid_and_version() {
        let record = InstanceInfo {
            port: 17320,
            pid: 42,
        };
        let current = || {
            Some(Probe::Ours {
                version: env!("CARGO_PKG_VERSION").into(),
            })
        };
        assert!(expected_service(42, &record, current()));
        assert!(!expected_service(43, &record, current()));
        assert!(!expected_service(42, &record, Some(Probe::Foreign)));
        assert!(!expected_service(
            42,
            &record,
            Some(Probe::Ours {
                version: "old".into()
            })
        ));
    }
}
