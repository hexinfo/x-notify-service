use std::fs::File;
use std::path::PathBuf;

/// 用户级数据目录(日志 port 文件等)
pub fn data_dir() -> PathBuf {
    dirs::data_local_dir().map_or_else(
        || crate::config::private_dir(&std::env::temp_dir()),
        |d| crate::config::private_dir(&d),
    )
}

/// 单实例锁文件位置:Linux 优先 `XDG_RUNTIME_DIR`,其余平台放数据目录
fn lock_path() -> PathBuf {
    #[cfg(target_os = "linux")]
    {
        if let Some(rt) = std::env::var_os("XDG_RUNTIME_DIR") {
            return crate::config::private_dir(&PathBuf::from(rt)).join("instance.lock");
        }
    }
    data_dir().join("instance.lock")
}

/// 尝试获取单实例锁;false 表示已有实例在运行。
/// flock 语义:进程退出(含崩溃)自动释放,锁守卫被故意泄漏以持有到进程结束。
pub fn acquire_lock() -> bool {
    let path = lock_path();
    if let Some(parent) = path.parent()
        && std::fs::create_dir_all(parent).is_err()
    {
        return true; // 锁目录不可用时不阻止启动
    }
    let Ok(file) = File::create(&path) else {
        return true;
    };
    let lock: &'static mut fd_lock::RwLock<File> = Box::leak(Box::new(fd_lock::RwLock::new(file)));
    // 写守卫与锁本体同样泄漏:持锁到进程结束(flock 语义随进程退出释放)
    lock.try_write().is_ok_and(|guard| {
        Box::leak(Box::new(guard));
        log::debug!("单实例锁已获取: {}", path.display());
        true
    })
}

/// 是否已有实例持锁(只读探测:不获取也不持有,探测完即释放句柄)
pub fn is_locked() -> bool {
    let path = lock_path();
    if path.parent().is_some_and(|p| !p.exists()) {
        return false;
    }
    File::create(&path).is_ok_and(|file| fd_lock::RwLock::new(file).try_write().is_err())
}

/// 端口文件内容:实际监听端口 + 服务进程 pid
pub struct InstanceInfo {
    pub port: u16,
    pub pid: u32,
}

pub fn read_port_file() -> Option<InstanceInfo> {
    let text = std::fs::read_to_string(data_dir().join("port")).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    Some(InstanceInfo {
        port: u16::try_from(v["port"].as_u64()?).ok()?,
        pid: u32::try_from(v["pid"].as_u64()?).ok()?,
    })
}

pub fn remove_port_file() {
    let _ = std::fs::remove_file(data_dir().join("port"));
}

/// 把实际端口 + PID 写入数据目录,便于本地工具/调试定位服务
pub fn write_port_file(port: u16) {
    let dir = data_dir();
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let content = format!("{{\"port\":{port},\"pid\":{}}}\n", std::process::id());
    let _ = std::fs::write(dir.join("port"), content);
}
