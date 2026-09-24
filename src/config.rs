use std::path::PathBuf;

use clap::{Parser, Subcommand};

pub const DEFAULT_PORT: u16 = 17320;

#[derive(Debug, Parser)]
#[command(
    name = "x-notify-service",
    version,
    about = "跨平台消息通知服务:浏览器调用 → 右下角置顶弹窗(系统通知兜底)"
)]
pub struct Cli {
    /// 协议拉起(x-notify://)时由系统传入的 URL,忽略即可
    #[arg(hide = true)]
    pub url_arg: Option<String>,

    /// 监听端口(被占用时自动向后探测 10 个)
    #[arg(long)]
    pub port: Option<u16>,

    /// 配置文件路径(默认: 二进制同目录 > 平台用户配置目录)
    #[arg(long)]
    pub config: Option<PathBuf>,

    /// 日志级别(trace/debug/info/warn/error)
    #[arg(long)]
    pub log_level: Option<String>,

    /// 日志目录
    #[arg(long)]
    pub log_dir: Option<PathBuf>,

    /// 禁用弹窗,强制走系统通知
    #[arg(long)]
    pub no_popup: bool,

    /// Windows 兜底通知的 AppId(默认 PowerShell)
    #[arg(long)]
    pub app_id: Option<String>,

    #[command(subcommand)]
    pub cmd: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// 运行服务本体(常驻进程;开机自启与 start 拉起使用)
    Serve,
    /// 注册开机自启动 + x-notify:// 协议,并启动服务
    Install,
    /// 清理全部注册项(自启动 + 协议)
    Uninstall,
    /// 输出诊断快照(实例/端口/显示环境/工作区/注册状态),只读
    Info,
    /// 启动服务(后台分离进程;已在运行则幂等提示)
    Start,
    /// 停止运行中的服务(幂等:未运行也正常退出)
    Stop,
    /// 重启服务(未运行则等效于 start)
    Restart,
    /// 本机发一条通知(服务在跑走其 HTTP 通道;否则本进程弹窗)
    Notify {
        /// 通知标题
        #[arg(short = 't', long = "title")]
        title: String,

        /// 正文(可选;支持 Markdown)
        #[arg(short = 'b', long = "body")]
        body: Option<String>,

        /// 弹窗宽度(逻辑像素,220-800;缺省 220)
        #[arg(long)]
        width: Option<u16>,

        /// 弹窗高度(逻辑像素,80-600;缺省 100)
        #[arg(long)]
        height: Option<u16>,

        /// 标题栏背景色(#RRGGBB)
        #[arg(long)]
        header_background_color: Option<String>,

        /// 标题文字颜色(#RRGGBB)
        #[arg(long)]
        header_text_color: Option<String>,

        /// 正文背景色(#RRGGBB)
        #[arg(long)]
        body_background_color: Option<String>,

        /// 正文默认文字颜色(#RRGGBB)
        #[arg(long)]
        body_text_color: Option<String>,

        /// 强制走系统通知兜底(不经弹窗)
        #[arg(short, long)]
        fallback: bool,
    },
    /// 关闭当前显示的弹窗(经运行中服务的 /close;幂等)
    Close,
}

/// 最终生效配置(CLI 参数 > 配置文件 > 默认值)
#[derive(Debug, Clone)]
pub struct Config {
    pub port: u16,
    pub log_level: String,
    pub log_dir: PathBuf,
    pub no_popup: bool,
    /// CORS 允许的 Origin(精确匹配);`["*"]` 表示全开(默认)
    pub cors_origins: Vec<String>,
    /// 是否应答 Access-Control-Allow-Private-Network(新版 Chrome LNA 预检用)
    pub allow_private_network: bool,
    /// 非空时 /notify 与 /close 需带匹配的 X-Token 头;空 = 无鉴权(默认)
    pub token: Option<String>,
    /// Windows 兜底通知的 AppId(仅 Windows 读取)
    #[cfg_attr(not(windows), allow(dead_code))]
    pub app_id: Option<String>,
}

#[derive(Debug, Default, serde::Deserialize)]
struct FileConfig {
    port: Option<u16>,
    log_level: Option<String>,
    log_dir: Option<PathBuf>,
    no_popup: Option<bool>,
    app_id: Option<String>,
    cors_origins: Option<Vec<String>>,
    allow_private_network: Option<bool>,
    token: Option<String>,
}

/// 平台标准日志目录
pub fn default_log_dir() -> PathBuf {
    if cfg!(target_os = "macos") {
        // ~/Library/Logs
        dirs::home_dir().map_or_else(
            || PathBuf::from(".").join("logs"),
            |h| private_dir(h.join("Library").join("Logs")),
        )
    } else if cfg!(windows) {
        // %LOCALAPPDATA%
        dirs::data_local_dir().map_or_else(
            || PathBuf::from(".").join("logs"),
            |d| private_dir(d).join("logs"),
        )
    } else {
        // XDG: ~/.local/state
        dirs::state_dir().map_or_else(
            || PathBuf::from(".").join("logs"),
            |d| private_dir(d).join("logs"),
        )
    }
}

/// 平台用户配置目录
fn user_config_path() -> Option<PathBuf> {
    dirs::config_local_dir()
        .or_else(dirs::config_dir)
        .map(|d| private_dir(d).join("config.toml"))
}

pub const APP_DIR_NAME: &str = "x-notify-service";
pub const ORG_DIR_NAME: &str = "Hexinfo";

#[allow(clippy::needless_pass_by_value)]
pub fn private_dir(base: PathBuf) -> PathBuf {
    base.join(ORG_DIR_NAME).join(APP_DIR_NAME)
}

#[allow(clippy::needless_pass_by_value)]
pub fn legacy_private_dir(base: PathBuf) -> PathBuf {
    base.join(APP_DIR_NAME)
}

pub fn resolve(cli: &Cli) -> Config {
    let file = load_file_config(cli.config.as_deref());
    Config {
        port: cli.port.or(file.port).unwrap_or(DEFAULT_PORT),
        log_level: cli
            .log_level
            .clone()
            .or(file.log_level)
            .unwrap_or_else(|| "info".into()),
        log_dir: cli
            .log_dir
            .clone()
            .or(file.log_dir)
            .unwrap_or_else(default_log_dir),
        no_popup: cli.no_popup || file.no_popup.unwrap_or(false),
        cors_origins: file.cors_origins.unwrap_or_else(|| vec!["*".into()]),
        allow_private_network: file.allow_private_network.unwrap_or(true),
        token: file.token.filter(|t| !t.is_empty()),
        app_id: cli.app_id.clone().or(file.app_id),
    }
}

/// 配置文件查找顺序:--config > 二进制同目录(绿色版友好) > 平台用户配置目录
// 解析失败告警直写 stderr:此刻日志可能尚未初始化
#[allow(clippy::print_stderr)]
fn load_file_config(explicit: Option<&std::path::Path>) -> FileConfig {
    for path in candidates(explicit) {
        if let Ok(text) = std::fs::read_to_string(&path) {
            match toml::from_str::<FileConfig>(&text) {
                Ok(cfg) => {
                    log::debug!("已加载配置文件: {}", path.display());
                    return cfg;
                }
                Err(e) => {
                    eprintln!("警告: 配置文件 {} 解析失败: {e}", path.display());
                }
            }
        }
    }
    FileConfig::default()
}

/// 当前实际生效的配置文件路径(info 诊断用);无则 None。
/// 不考虑 --config 显式传入(诊断展示按默认查找顺序即可)
pub fn config_in_use() -> Option<PathBuf> {
    candidates(None).into_iter().find(|p| p.is_file())
}

fn candidates(explicit: Option<&std::path::Path>) -> Vec<PathBuf> {
    if let Some(p) = explicit {
        return vec![p.to_path_buf()];
    }
    let mut v = Vec::new();
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        v.push(dir.join("config.toml"));
    }
    if let Some(p) = user_config_path() {
        v.push(p);
    }
    if let Some(p) = dirs::config_local_dir().or_else(dirs::config_dir) {
        v.push(legacy_private_dir(p).join("config.toml"));
    }
    v
}

#[cfg(test)]
mod tests {
    #[test]
    fn private_paths_include_organization_without_changing_app_name() {
        let base = std::path::PathBuf::from("root");
        assert_eq!(
            super::private_dir(base.clone()),
            base.join("Hexinfo/x-notify-service")
        );
        assert_eq!(
            super::legacy_private_dir(base.clone()),
            base.join("x-notify-service")
        );
        assert_eq!(super::APP_DIR_NAME, "x-notify-service");
    }
}
