// install/uninstall 为 CLI 用户交互命令,输出直连终端(此时可能无日志文件)
#![allow(clippy::print_stdout, clippy::print_stderr)]

use crate::{autostart, ctl, protocol};

/// install 子命令:注册自启动 + x-notify:// 协议,随后分离启动并确认服务就绪。
pub fn install() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(target_os = "macos")]
    {
        let Some((old_data, old_logs)) = old_macos_dirs() else {
            return Err("无法定位用户目录".into());
        };
        if old_port_running(&old_data) {
            return Err("旧版服务仍在运行;请先停止旧版服务".into());
        }
        let mut old_lock = open_old_lock(&old_data, &old_logs)?;
        let _guard = if let Some(lock) = old_lock.as_mut() {
            Some(
                lock.try_write()
                    .map_err(|_error| "旧版服务仍在运行;请先停止旧版服务")?,
            )
        } else {
            None
        };
        install_inner()?;
        cleanup_old_macos_dirs(&old_data, &old_logs);
        Ok(())
    }

    #[cfg(not(target_os = "macos"))]
    install_inner()
}

fn install_inner() -> Result<(), Box<dyn std::error::Error>> {
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
fn open_old_lock(
    old_data: &std::path::Path,
    old_logs: &std::path::Path,
) -> Result<Option<fd_lock::RwLock<std::fs::File>>, Box<dyn std::error::Error>> {
    use std::fs::OpenOptions;
    if !old_data.exists() && !old_logs.exists() {
        return Ok(None);
    }
    if let Ok(meta) = std::fs::symlink_metadata(old_data)
        && (meta.file_type().is_symlink() || !meta.is_dir())
    {
        return Err("旧版数据目录不是普通目录".into());
    }
    std::fs::create_dir_all(old_data)?;
    let lock_path = old_data.join("instance.lock");
    if let Ok(meta) = std::fs::symlink_metadata(&lock_path)
        && (meta.file_type().is_symlink() || !meta.is_file())
    {
        return Err("旧版实例锁不是普通文件".into());
    }
    let lock_file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock_path)?;
    Ok(Some(fd_lock::RwLock::new(lock_file)))
}

#[cfg(target_os = "macos")]
fn old_port_running(old_data: &std::path::Path) -> bool {
    let Ok(text) = std::fs::read_to_string(old_data.join("port")) else {
        return false;
    };
    let Ok(record) = serde_json::from_str::<serde_json::Value>(&text) else {
        return false;
    };
    let Some(port) = record["port"].as_u64().and_then(|p| u16::try_from(p).ok()) else {
        return false;
    };
    matches!(ctl::probe_port(port), Some(ctl::Probe::Ours { .. }))
}

#[cfg(target_os = "macos")]
fn cleanup_old_macos_dirs(old_data: &std::path::Path, old_logs: &std::path::Path) {
    use std::fs::remove_dir_all;
    for path in [old_data, old_logs] {
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

    #[cfg(target_os = "macos")]
    #[test]
    fn logs_only_without_legacy_lock_allows_install_preflight() {
        let test_root = std::env::temp_dir().join(format!(
            "x-notify-install-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let old_data = test_root.join("Application Support/x-notify-service");
        let old_logs = test_root.join("Logs/x-notify-service");
        std::fs::create_dir_all(&old_logs).unwrap();
        let mut lock = super::open_old_lock(&old_data, &old_logs).unwrap().unwrap();
        let guard = lock.try_write().unwrap();
        let mut competitor = super::open_old_lock(&old_data, &old_logs).unwrap().unwrap();
        competitor.try_write().unwrap_err();
        assert!(!super::old_port_running(&old_data));
        assert!(old_logs.exists());
        drop(guard);
        let competitor_guard = competitor.try_write().unwrap();
        drop(competitor_guard);
        std::fs::remove_dir_all(test_root).unwrap();
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn held_legacy_lock_refuses_second_owner_until_guard_drops() {
        let old_data = std::env::temp_dir().join(format!(
            "x-notify-install-lock-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&old_data).unwrap();
        std::fs::File::create(old_data.join("instance.lock")).unwrap();
        let old_logs = old_data.with_file_name("old-logs");
        let mut first = super::open_old_lock(&old_data, &old_logs).unwrap().unwrap();
        let guard = first.try_write().unwrap();
        let mut second = super::open_old_lock(&old_data, &old_logs).unwrap().unwrap();
        second.try_write().unwrap_err();
        drop(guard);
        let _new_guard = second.try_write().unwrap();
        std::fs::remove_dir_all(old_data).unwrap();
    }
}
