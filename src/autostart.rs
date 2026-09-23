//! 用户级开机自启动,统一以 `<二进制> serve` 启动服务本体
//! (无参数的语义是显示帮助,自启动必须显式带子命令)。
//! 不用 auto-launch crate:其 API 无法附带启动参数。
//! - Windows: HKCU\Software\Microsoft\Windows\CurrentVersion\Run
//! - Linux: ~/.config/autostart/*.desktop(XDG)
//! - macOS: ~/Library/LaunchAgents/*.plist

use std::io::ErrorKind;

const SERVE_ARG: &str = "serve";

#[cfg(windows)]
pub fn enable() -> Result<(), Box<dyn std::error::Error>> {
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_SET_VALUE};
    let exe = std::env::current_exe()?;
    let run = RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(
        "Software\\Microsoft\\Windows\\CurrentVersion\\Run",
        KEY_SET_VALUE,
    )?;
    run.set_value(
        crate::config::APP_DIR_NAME,
        &format!("\"{}\" {SERVE_ARG}", exe.display()),
    )?;
    Ok(())
}

#[cfg(windows)]
pub fn disable() -> Result<(), Box<dyn std::error::Error>> {
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_WRITE};
    let run = RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(
        "Software\\Microsoft\\Windows\\CurrentVersion\\Run",
        KEY_WRITE,
    )?;
    match run.delete_value(crate::config::APP_DIR_NAME) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

#[cfg(windows)]
pub fn is_enabled() -> Result<bool, Box<dyn std::error::Error>> {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;
    let run = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Run")?;
    let present = run
        .get_value::<String, _>(crate::config::APP_DIR_NAME)
        .is_ok();
    Ok(present)
}

#[cfg(not(windows))]
fn entry_path() -> std::path::PathBuf {
    if cfg!(target_os = "macos") {
        dirs::home_dir()
            .map_or_else(
                || std::path::PathBuf::from("."),
                |h| h.join("Library/LaunchAgents"),
            )
            .join(format!("{}.plist", crate::config::APP_DIR_NAME))
    } else {
        dirs::config_dir()
            .map_or_else(|| std::path::PathBuf::from("."), |c| c.join("autostart"))
            .join(format!("{}.desktop", crate::config::APP_DIR_NAME))
    }
}

#[cfg(target_os = "linux")]
pub fn enable() -> Result<(), Box<dyn std::error::Error>> {
    let exe = std::env::current_exe()?;
    let path = entry_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, linux_desktop_entry(&exe))?;
    Ok(())
}

#[cfg(any(target_os = "linux", test))]
fn linux_desktop_entry(exe: &std::path::Path) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name={name}\n\
         Exec=\"{exe}\" {SERVE_ARG}\n\
         StartupWMClass={name}\n\
         NoDisplay=true\n\
         Terminal=false\n\
         StartupNotify=false\n",
        name = crate::config::APP_DIR_NAME,
        exe = exe.display(),
    )
}

#[cfg(target_os = "macos")]
pub fn enable() -> Result<(), Box<dyn std::error::Error>> {
    let exe = std::env::current_exe()?;
    let path = entry_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <plist version=\"1.0\">\n\
         <dict>\n\
         \t<key>Label</key>\n\
         \t<string>{label}</string>\n\
         \t<key>ProgramArguments</key>\n\
         \t<array>\n\
         \t\t<string>{exe}</string>\n\
         \t\t<string>{SERVE_ARG}</string>\n\
         \t</array>\n\
         \t<key>RunAtLoad</key>\n\
         \t<true/>\n\
         </dict>\n\
         </plist>\n",
        label = crate::config::APP_DIR_NAME,
        exe = exe.display(),
    );
    std::fs::write(&path, content)?;
    Ok(())
}

#[cfg(not(windows))]
pub fn disable() -> Result<(), Box<dyn std::error::Error>> {
    match std::fs::remove_file(entry_path()) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

#[cfg(not(windows))]
// Result 包装为与 Windows 版签名保持一致(info 统一处理)
#[allow(clippy::unnecessary_wraps)]
pub fn is_enabled() -> Result<bool, Box<dyn std::error::Error>> {
    Ok(entry_path().exists())
}

#[cfg(test)]
mod tests {
    use super::linux_desktop_entry;

    #[test]
    fn linux_autostart_is_hidden_and_terminal_free() {
        let entry = linux_desktop_entry(std::path::Path::new("/opt/x-notify-service"));
        assert!(entry.contains("NoDisplay=true\n"));
        assert!(entry.contains("Terminal=false\n"));
        assert!(entry.contains("StartupNotify=false\n"));
    }
}
