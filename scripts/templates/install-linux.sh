#!/bin/sh
# x-notify-service Linux(UOS/麒麟)安装脚本 —— 用户级安装,无需 root
# 注册逻辑由二进制自身完成(x-notify-service install),本脚本只做文件放置
set -e

SRC_DIR="$(cd "$(dirname "$0")" && pwd)"
BIN_DIR="$HOME/.local/bin"
CONF_DIR="$HOME/.config/x-notify-service"

mkdir -p "$BIN_DIR" "$CONF_DIR"

# 停止在跑的旧实例:覆盖正在执行的二进制会报 Text file busy。
# 经端口文件取 pid,校验其 cmdline 含本程序名后再 kill,避免 pid 复用误杀
PORT_FILE="$HOME/.local/share/x-notify-service/port"
if [ -f "$PORT_FILE" ]; then
    pid=$(sed -n 's/.*"pid":\([0-9][0-9]*\).*/\1/p' "$PORT_FILE")
    if [ -n "$pid" ] && tr '\0' ' ' < "/proc/$pid/cmdline" 2>/dev/null | grep -q "x-notify-service"; then
        # 服务无信号处理器,TERM 即刻退出;短歇后 KILL 仅兜底僵死,无需长宽限
        kill "$pid" 2>/dev/null || true
        sleep 0.2
        kill -9 "$pid" 2>/dev/null || true
        echo "已停止旧实例(pid $pid)"
    fi
fi

# 同目录临时文件 + mv 覆盖:rename 对运行中二进制也合法,与停实例互为保险
cp "$SRC_DIR/bin/x-notify-service" "$BIN_DIR/.x-notify-service.new"
chmod +x "$BIN_DIR/.x-notify-service.new"
mv -f "$BIN_DIR/.x-notify-service.new" "$BIN_DIR/x-notify-service"

if [ ! -f "$CONF_DIR/config.toml" ]; then
    cp "$SRC_DIR/config/config.toml" "$CONF_DIR/config.toml"
    echo "已生成配置: $CONF_DIR/config.toml"
fi

# 图标安装到用户级 hicolor(供 desktop 文件引用;包内平铺于 icons/)
if [ -d "$SRC_DIR/icons" ]; then
    for png in "$SRC_DIR"/icons/x-notify-service-*.png; do
        [ -f "$png" ] || continue
        size="$(basename "$png" .png | sed 's/.*-//')"
        mkdir -p "$HOME/.local/share/icons/hicolor/${size}x${size}/apps"
        cp "$png" "$HOME/.local/share/icons/hicolor/${size}x${size}/apps/x-notify-service.png"
    done
    # 刷新用户级图标缓存,确保 desktop 文件的 Icon= 能被解析(DDE 任务栏走此链)
    gtk-update-icon-cache -f -t "$HOME/.local/share/icons/hicolor" 2>/dev/null || true
fi
# SDK 与手册放置到用户数据目录
mkdir -p "$HOME/.local/share/x-notify-service"
cp "$SRC_DIR/sdk.js" "$SRC_DIR/sdk.umd.js" "$SRC_DIR/sdk-使用手册.md" "$HOME/.local/share/x-notify-service/"

case ":$PATH:" in
    *":$BIN_DIR:"*) ;;
    *) echo "提示: 建议将 $BIN_DIR 加入 PATH" ;;
esac

"$BIN_DIR/x-notify-service" install

echo "安装完成:开机自启 + x-notify:// 协议已注册,服务已启动"
echo "卸载: $BIN_DIR/x-notify-service uninstall"
