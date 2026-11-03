//! Windows 平台环境集成:GUI 子系统的控制台挂接与用户 PATH 管理

// CLI 终端输出语义
#![allow(clippy::print_stderr)]
/// GUI 子系统二进制无控制台:从 cmd/PowerShell 启动 CLI 子命令时把输出接回
/// 调用方终端(AttachConsole 父进程);双击/自启动/安装器等无控制台场景
/// 挂接失败,保持静默,不影响服务运行。
/// stdout 已被重定向(`info > out.txt`、管道、CI 捕获)时保持原句柄——
/// 此时挂控制台会把重定向内容整个丢掉(真机 Win7 实测 info 空输出)。
#[cfg(windows)]
// 取屏 FFI 同款豁免:仅此一处 Win32 控制台挂接
#[allow(unsafe_code, clippy::multiple_unsafe_ops_per_block)]
pub fn attach_parent_console() {
    use windows_sys::Win32::Foundation::{GENERIC_WRITE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_SHARE_WRITE, FILE_TYPE_DISK, FILE_TYPE_PIPE, GetFileType,
        OPEN_EXISTING,
    };
    use windows_sys::Win32::System::Console::{
        ATTACH_PARENT_PROCESS, AttachConsole, GetStdHandle, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE,
        SetStdHandle,
    };

    // SAFETY: 常量入参;句柄仅转交标准设备表,失败即原样返回静默运行
    unsafe {
        // stdout 已重定向(磁盘文件或管道)时不碰标准句柄,保住重定向语义
        let stdout = GetStdHandle(STD_OUTPUT_HANDLE);
        let redirected = !stdout.is_null()
            && stdout != INVALID_HANDLE_VALUE
            && matches!(GetFileType(stdout), FILE_TYPE_DISK | FILE_TYPE_PIPE);
        if redirected {
            return;
        }
        if AttachConsole(ATTACH_PARENT_PROCESS) == 0 {
            return;
        }
        let name: Vec<u16> = "CONOUT$\0".encode_utf16().collect();
        let con = CreateFileW(
            name.as_ptr(),
            GENERIC_WRITE,
            FILE_SHARE_WRITE,
            std::ptr::null(),
            OPEN_EXISTING,
            0,
            std::ptr::null_mut(),
        );
        if con != INVALID_HANDLE_VALUE {
            SetStdHandle(STD_OUTPUT_HANDLE, con);
            SetStdHandle(STD_ERROR_HANDLE, con);
        }
    }
}

#[cfg(not(windows))]
pub const fn attach_parent_console() {}

#[cfg(windows)]
// Win32 注册表/消息广播 FFI
#[allow(unsafe_code, clippy::multiple_unsafe_ops_per_block)]
pub fn set_user_path(add: bool) {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
    use winreg::{RegKey, RegValue};

    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let Some(dir) = exe.parent() else { return };
    let dir_text = dir.to_string_lossy().to_string();

    let env = match RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags("Environment", KEY_READ | KEY_WRITE)
    {
        Ok(k) => k,
        Err(e) => {
            eprintln!("配置用户 PATH 失败: {e}");
            return;
        }
    };
    let raw = env.get_raw_value("Path").unwrap_or(RegValue {
        bytes: Vec::new(),
        vtype: winreg::enums::REG_EXPAND_SZ,
    });
    // 注册表 Path 值为 UTF-16LE:位拼解码(低字节在前)
    let (words, _) = raw.bytes.as_chunks::<2>();
    let words: Vec<u16> = words
        .iter()
        .map(|b| {
            let [lo, hi] = *b;
            (u16::from(hi) << 8) | u16::from(lo)
        })
        .collect();
    let cur = String::from_utf16_lossy(&words);
    let mut entries: Vec<String> = cur
        .split(';')
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();
    let exists = entries.iter().any(|e| e.eq_ignore_ascii_case(&dir_text));
    let changed = match (add, exists) {
        (true, false) => {
            entries.push(dir_text);
            true
        }
        (false, true) => {
            entries.retain(|e| !e.eq_ignore_ascii_case(&dir_text));
            true
        }
        _ => false,
    };
    if !changed {
        return;
    }
    let joined = format!("{}\0", entries.join(";"));
    let bytes: Vec<u8> = joined.encode_utf16().flat_map(u16::to_le_bytes).collect();
    if let Err(e) = env.set_raw_value(
        "Path",
        &RegValue {
            bytes,
            vtype: raw.vtype,
        },
    ) {
        eprintln!("写入用户 PATH 失败: {e}");
        return;
    }
    // 环境变更广播:新开终端即可命中,已开终端需重开
    let param: Vec<u16> = "Environment\0".encode_utf16().collect();
    // SAFETY: 广播常量消息;指针指向本栈上有效的 UTF-16 参数串
    unsafe {
        use windows_sys::Win32::UI::WindowsAndMessaging;

        WindowsAndMessaging::SendMessageTimeoutW(
            WindowsAndMessaging::HWND_BROADCAST,
            WindowsAndMessaging::WM_SETTINGCHANGE,
            0,
            param.as_ptr() as isize,
            WindowsAndMessaging::SMTO_ABORTIFHUNG,
            3000,
            std::ptr::null_mut(),
        );
    };
}
