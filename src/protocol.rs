//! x-notify:// 自定义协议注册(全部用户级,无需提权)
//! - Windows: HKCU\Software\Classes\x-notify
//! - Linux/UOS/麒麟: ~/.local/share/applications + `MimeType`
//! - macOS: 依赖 .app bundle(仅测试环境,经 lsregister)
//!
//! 由 install/uninstall 统一调用,不单独暴露子命令。

pub const SCHEME: &str = "x-notify";

/// 协议是否已注册(info 诊断用)
#[cfg(windows)]
pub fn is_registered() -> bool {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(format!("Software\\Classes\\{SCHEME}"))
        .is_ok()
}

/// 协议是否已注册:desktop 文件存在即视为已注册
#[cfg(target_os = "linux")]
pub fn is_registered() -> bool {
    desktop_path().exists()
}

/// macOS 仅测试环境:不做查询,恒报未注册
#[cfg(target_os = "macos")]
pub const fn is_registered() -> bool {
    false
}

#[cfg(windows)]
pub fn register() -> Result<(), Box<dyn std::error::Error>> {
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_CREATE_SUB_KEY};

    let exe = std::env::current_exe()?;
    // 冲突检测:已有其他程序的注册则警告后覆盖(HKCU 后写者赢)
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let exe_path = exe.to_string_lossy();
    if let Ok(cmd) = hkcu
        .open_subkey(format!("Software\\Classes\\{SCHEME}\\shell\\open\\command"))
        .and_then(|k| k.get_value::<String, _>(""))
        && !cmd.contains(exe_path.as_ref())
    {
        log::warn!("{SCHEME}:// 协议已被其他程序注册({cmd}),将覆盖");
    }
    let classes = hkcu.open_subkey_with_flags("Software\\Classes", KEY_CREATE_SUB_KEY)?;
    let (key, _) = classes.create_subkey(SCHEME)?;
    key.set_value("", &format!("URL:{SCHEME} 协议"))?;
    key.set_value("URL Protocol", &"")?;
    let (cmd, _) = key.create_subkey("shell\\open\\command")?;
    cmd.set_value("", &format!("\"{}\" \"%1\"", exe.display()))?;
    Ok(())
}

#[cfg(windows)]
pub fn unregister() -> Result<(), Box<dyn std::error::Error>> {
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_WRITE};

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let classes = hkcu.open_subkey_with_flags("Software\\Classes", KEY_WRITE)?;
    match classes.delete_subkey_all(format!("{SCHEME}\\shell\\open\\command")) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    match classes.delete_subkey_all(SCHEME) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn desktop_path() -> std::path::PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from(".local/share"))
        .join("applications")
        .join(format!("{}.desktop", crate::config::APP_DIR_NAME))
}

#[cfg(target_os = "linux")]
pub fn register() -> Result<(), Box<dyn std::error::Error>> {
    let exe = std::env::current_exe()?;
    let path = desktop_path();
    // 冲突检测:查询当前 scheme 默认处理器,已被其他程序接管则警告后覆盖
    let our_desktop = format!("{}.desktop", crate::config::APP_DIR_NAME);
    if let Ok(out) = std::process::Command::new("xdg-mime")
        .args(["query", "default", &format!("x-scheme-handler/{SCHEME}")])
        .output()
    {
        let current = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !current.is_empty() && current != our_desktop {
            log::warn!("{SCHEME}:// 协议当前由 {current} 处理,将改为本程序接管");
        }
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name={name}\n\
         {exec_line}\n\
         Icon={name}\n\
         StartupWMClass={name}\n\
         X-Deepin-AppID={name}\n\
         NoDisplay=true\n\
         Terminal=false\n\
         MimeType=x-scheme-handler/{scheme};\n",
        name = crate::config::APP_DIR_NAME,
        exec_line = desktop_exec_line(&exe),
        scheme = SCHEME,
    );
    std::fs::write(&path, content)?;
    // 刷新桌面数据库并把本应用设为 scheme 默认处理器(失败不阻塞)
    if let Some(apps_dir) = path.parent() {
        let _ = std::process::Command::new("update-desktop-database")
            .arg(apps_dir)
            .status();
    }
    let _ = std::process::Command::new("xdg-mime")
        .args([
            "default",
            our_desktop.as_str(),
            &format!("x-scheme-handler/{SCHEME}"),
        ])
        .status();
    Ok(())
}

#[cfg(any(target_os = "linux", test))]
fn desktop_exec_line(path: &std::path::Path) -> String {
    format!("Exec=\"{}\" %u", desktop_exec_path(path))
}

#[cfg(any(target_os = "linux", test))]
fn desktop_exec_path(path: &std::path::Path) -> String {
    let mut escaped = String::new();
    for ch in path.to_string_lossy().chars() {
        match ch {
            '\\' => escaped.push_str("\\\\\\\\"),
            '"' => escaped.push_str("\\\\\\\""),
            '$' => escaped.push_str("\\\\$"),
            '`' => escaped.push_str("\\\\`"),
            '%' => escaped.push_str("%%"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

#[cfg(test)]
#[test]
fn desktop_exec_path_escapes_special_characters() {
    let path = std::path::Path::new("/home/a b/quo\"te\\cash$`%/x-notify-service");
    assert_eq!(
        desktop_exec_line(path),
        "Exec=\"/home/a b/quo\\\\\\\"te\\\\\\\\cash\\\\$\\\\`%%/x-notify-service\" %u"
    );
}

#[cfg(target_os = "linux")]
pub fn unregister() -> Result<(), Box<dyn std::error::Error>> {
    match std::fs::remove_file(desktop_path()) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    if let Some(apps_dir) = desktop_path().parent() {
        let _ = std::process::Command::new("update-desktop-database")
            .arg(apps_dir)
            .status();
    }
    Ok(())
}

#[cfg(target_os = "macos")]
/// macOS:仅测试环境支持——当前进程位于 .app bundle 内时经 lsregister 注册
pub fn register() -> Result<(), Box<dyn std::error::Error>> {
    let exe = std::env::current_exe()?;
    let Some(bundle) = find_app_bundle(&exe) else {
        return Err("macOS 协议注册需要 .app bundle(用 scripts/pack-macos.sh 生成后运行)".into());
    };
    let lsregister = "/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister";
    let status = std::process::Command::new(lsregister)
        .arg("-f")
        .arg(&bundle)
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err("lsregister 注册失败".into())
    }
}

#[cfg(target_os = "macos")]
pub fn unregister() -> Result<(), Box<dyn std::error::Error>> {
    let exe = std::env::current_exe()?;
    let Some(bundle) = find_app_bundle(&exe) else {
        return Ok(()); // 未运行在 .app bundle 内:本就未注册,视为无操作
    };
    let lsregister = "/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister";
    let _ = std::process::Command::new(lsregister)
        .arg("-u")
        .arg(&bundle)
        .status();
    Ok(())
}

#[cfg(target_os = "macos")]
fn find_app_bundle(exe: &std::path::Path) -> Option<std::path::PathBuf> {
    let mut dir = exe.parent()?;
    loop {
        if dir.extension().and_then(|e| e.to_str()) == Some("app") {
            return Some(dir.to_path_buf());
        }
        dir = dir.parent()?;
    }
}
