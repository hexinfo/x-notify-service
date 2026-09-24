# Hexinfo 路径迁移设计

## 目标与范围

所有平台的应用私有安装、配置、数据与日志目录增加 `Hexinfo` 组织层级；现有配置和日志在升级后保留，成功迁移的旧目录清理干净。程序名 `x-notify-service`、`x-notify://` 协议、HTTP/SDK 接口及默认端口不变。此改动只生成测试包，不自动发布新版本。

标准系统入口目录不迁移：Linux 的 `~/.local/bin`、XDG `autostart`/`applications` 与 hicolor 图标目录，macOS 的 `LaunchAgents`，Windows 的 Run、Classes 和卸载列表仍留在系统规定位置；其中指向程序的路径更新为新位置。

## 新旧路径

| 平台 | 用途 | 旧路径 | 新路径 |
| --- | --- | --- | --- |
| Windows | 默认安装 | `%LOCALAPPDATA%\Programs\x-notify-service` | `%LOCALAPPDATA%\Programs\Hexinfo\x-notify-service` |
| Windows | 用户数据、默认日志 | `%LOCALAPPDATA%\x-notify-service` | `%LOCALAPPDATA%\Hexinfo\x-notify-service` |
| Windows | 安装目录配置 | 旧安装目录的 `config.toml` | 新安装目录的 `config.toml` |
| Windows | 安装位置注册表 | `HKCU\Software\x-notify-service` | `HKCU\Software\Hexinfo\x-notify-service` |
| Linux | 程序和 SDK | `~/.local/bin/x-notify-service`、`$XDG_DATA_HOME/x-notify-service` | `$XDG_DATA_HOME/Hexinfo/x-notify-service/bin/x-notify-service`、同级 SDK；`~/.local/bin/x-notify-service` 保留为指向新程序的链接 |
| Linux | 配置 | `$XDG_CONFIG_HOME/x-notify-service` | `$XDG_CONFIG_HOME/Hexinfo/x-notify-service` |
| Linux | 状态和日志 | `$XDG_STATE_HOME/x-notify-service` | `$XDG_STATE_HOME/Hexinfo/x-notify-service` |
| macOS | 配置和数据 | `~/Library/Application Support/x-notify-service` | `~/Library/Application Support/Hexinfo/x-notify-service` |
| macOS | 默认日志 | `~/Library/Logs/x-notify-service` | `~/Library/Logs/Hexinfo/x-notify-service` |

Linux 的 XDG 根目录为空时分别采用 `~/.local/share`、`~/.config`、`~/.local/state`。Windows 默认安装目录使用正确的 `Programs` 拼写；不拼接 `%APPDATA%\Local`。macOS 当前 `.app` 仅为本地测试包，没有固定安装目录，本次不移动用户自行放置的 `.app`。

## 升级与清理

1. 安装器/安装脚本先停止旧服务，再迁移；注册新的自启和协议目标后启动新服务。新的实例端口和锁文件由新进程重新生成，不沿用旧端口/锁文件。
2. 对精确的旧默认目录：新目录不存在时优先整体移动，保留未知用户文件；新目录已存在时仅将不冲突的文件迁入，绝不覆盖新目录已有文件。冲突文件保留在旧目录并给出可诊断提示。
3. Windows 安装目录中的 `config.toml` 优先级：现有新目录配置 > 旧默认安装目录配置 > 包内模板。安装器覆盖程序与 SDK 文件，但不覆盖已有配置。仅在新程序已放置成功后清理旧默认安装目录；自定义旧安装路径不自动递归删除。
4. Linux 安装脚本尊重 XDG 环境变量，替换 `~/.local/bin/x-notify-service` 为新程序链接，更新 SDK、配置、状态目录，再调用程序的 `install` 刷新自启和协议项。macOS 在运行新版 `install` 或安全的首次启动时迁移旧应用数据目录。
5. 旧目录只在内容成功迁移或确认为可再生成的程序文件后删除；删除目标必须是上述精确旧路径。发生权限、同名文件或运行中实例冲突时，保留旧目录并报告，不作静默覆盖或递归删除。

## 验收

- Windows 原生 MSVC/NSIS 测试安装包安装到新默认路径；升级旧默认安装时保留配置、重启服务、旧默认安装目录消失；自定义旧安装目录不被删除。
- Linux x86_64/aarch64 包的安装脚本在默认及自定义 XDG 根目录下安装成功；命令入口、自启、协议、SDK、配置和日志指向新位置；旧路径按冲突规则清理。
- macOS 配置、数据、日志读写新路径，旧数据在安全迁移后保留，标准 LaunchAgent 路径不变。
- 迁移可重复运行；已有新配置不被旧配置或模板覆盖；冲突时旧文件可恢复；其余通知 API、Markdown、弹窗尺寸和外观不变。
