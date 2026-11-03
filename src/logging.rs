// 日志尚未初始化时的兜底告警直写 stderr
#![allow(clippy::print_stderr)]

use std::io::Write as _;

use flexi_logger::LoggerHandle;
use flexi_logger::{Age, Cleanup, Criterion, Duplicate, FileSpec, Logger, Naming, WriteMode};

use crate::config::Config;

/// 初始化日志(仅 serve 模式调用):按天滚动、保留 3 天(历史 3 + 当前 1,最多 4 个文件),
/// warn 及以上镜像 stderr;轮换/清理由输出线程承担。
/// 返回的 `LoggerHandle` 需在 main 存活期间保持持有(丢弃即停日志)。
pub fn init(cfg: &Config) -> Option<LoggerHandle> {
    if let Err(e) = std::fs::create_dir_all(&cfg.log_dir) {
        eprintln!("警告: 无法创建日志目录 {}: {e}", cfg.log_dir.display());
        return None;
    }
    let spec = FileSpec::default()
        .directory(&cfg.log_dir)
        .basename(crate::config::APP_DIR_NAME)
        .suppress_timestamp();

    // 全局默认 warn:第三方依赖(iced_winit 的 "Window attributes"、winit、
    // cosmic-text 等)的 INFO 调试日志不混入产品日志;本 crate 按用户配置级别
    let spec_str = format!("warn, x_notify_service={}", cfg.log_level);
    let base = match Logger::try_with_env_or_str(&spec_str)
        .or_else(|_| Logger::try_with_str("warn, x_notify_service=info"))
    {
        Ok(b) => b,
        Err(e) => {
            eprintln!("警告: 日志级别解析失败: {e}");
            return None;
        }
    };
    let logger = base
        .log_to_file(spec)
        .rotate(
            Criterion::Age(Age::Day),
            Naming::Timestamps,
            Cleanup::KeepLogFiles(3),
        )
        .duplicate_to_stderr(Duplicate::Warn)
        // 异步写:调用线程仅入队,输出线程持续落盘(流式)并每 300ms 刷盘
        .write_mode(WriteMode::AsyncWith {
            pool_capa: 64,
            message_capa: 256,
            flush_interval: std::time::Duration::from_millis(300),
        })
        .format_for_files(flexi_logger::opt_format);

    match logger.start() {
        Ok(handle) => {
            let _ = std::io::stdout().flush();
            Some(handle)
        }
        Err(e) => {
            eprintln!("警告: 日志初始化失败: {e}");
            None
        }
    }
}
