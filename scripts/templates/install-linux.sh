#!/bin/sh
# 用户级安装；自启和协议注册由 x-notify-service install 完成。
set -e

SRC_DIR=$(cd "$(dirname "$0")" && pwd)
DATA_HOME=${XDG_DATA_HOME:-$HOME/.local/share}
CONFIG_HOME=${XDG_CONFIG_HOME:-$HOME/.config}
APP_DIR="$DATA_HOME/Hexinfo/x-notify-service"
PROGRAM="$APP_DIR/bin/x-notify-service"
BIN_DIR="$HOME/.local/bin"
CONF_DIR="$CONFIG_HOME/Hexinfo/x-notify-service"

report_install_result() {
    result=$?
    trap - EXIT
    set +e
    if [ "$result" -eq 0 ]; then
        message='x-notify-service 安装成功'
        kind=info
    else
        message='x-notify-service 安装失败，请查看终端输出'
        kind=error
    fi
    echo "$message"
    if [ -x "$SRC_DIR/bin/x-notify-service" ] &&
        "$SRC_DIR/bin/x-notify-service" notify -t "$message" -b "$message"; then
        :
    elif command -v zenity >/dev/null 2>&1 && zenity --"$kind" --text="$message"; then
        :
    elif command -v kdialog >/dev/null 2>&1 && {
        if [ "$result" -eq 0 ]; then kdialog --msgbox "$message";
        else kdialog --error "$message"; fi
    }; then
        :
    elif command -v notify-send >/dev/null 2>&1; then
        notify-send "$message" || true
    fi
    exit "$result"
}
trap report_install_result EXIT

# 仅停止端口文件指向、且可执行文件确实位于受管安装位置的实例。
OLD_PROGRAM="$BIN_DIR/x-notify-service"
stop_installed_instance() { # $1=port file $2=expected executable
    [ -f "$1" ] || return 0
    pid=$(sed -n 's/.*"pid":\([0-9][0-9]*\).*/\1/p' "$1")
    if [ -n "$pid" ]; then
        running_program=$(readlink "/proc/$pid/exe" 2>/dev/null || true)
        if [ "$running_program" = "$2" ] ||
            [ "$running_program" = "$2 (deleted)" ]; then
            kill "$pid" 2>/dev/null || true
            attempts=0
            while [ "$attempts" -lt 20 ]; do
                running_program=$(readlink "/proc/$pid/exe" 2>/dev/null || true)
                if [ "$running_program" != "$2" ] &&
                    [ "$running_program" != "$2 (deleted)" ]; then
                    echo "已停止旧实例(pid $pid)"
                    return 0
                fi
                sleep 0.1
                attempts=$((attempts + 1))
            done
            echo "实例仍在运行，安装中止(pid $pid)" >&2
            return 1
        fi
    fi
}
if [ ! -L "$OLD_PROGRAM" ]; then
    stop_installed_instance "$DATA_HOME/x-notify-service/port" "$OLD_PROGRAM"
fi
stop_installed_instance "$APP_DIR/port" "$PROGRAM"

mkdir -p "$APP_DIR/bin" "$BIN_DIR" "$CONF_DIR"
cp "$SRC_DIR/bin/x-notify-service" "$APP_DIR/bin/.x-notify-service.new"
chmod +x "$APP_DIR/bin/.x-notify-service.new"
mv -f "$APP_DIR/bin/.x-notify-service.new" "$PROGRAM"

# 同目录 rename 原子切换命令入口。目录不是合法的旧命令入口，不覆盖它。
if [ -d "$OLD_PROGRAM" ] && [ ! -L "$OLD_PROGRAM" ]; then
    echo "命令入口是目录，无法替换: $OLD_PROGRAM" >&2
    exit 1
fi
ln -s "$PROGRAM" "$BIN_DIR/.x-notify-service.new.$$"
mv -f "$BIN_DIR/.x-notify-service.new.$$" "$OLD_PROGRAM"

# install 负责迁移旧 XDG 配置、状态和数据（含 XDG_STATE_HOME），并刷新自启/协议目标。
"$PROGRAM" install

if [ ! -f "$CONF_DIR/config.toml" ]; then
    cp "$SRC_DIR/config/config.toml" "$CONF_DIR/config.toml"
    echo "已生成配置: $CONF_DIR/config.toml"
fi

cp "$SRC_DIR/sdk.js" "$SRC_DIR/sdk.umd.js" "$SRC_DIR/sdk-使用手册.md" "$APP_DIR/"

# 图标继续使用桌面规范规定的 hicolor 入口。
if [ -d "$SRC_DIR/icons" ]; then
    for png in "$SRC_DIR"/icons/x-notify-service-*.png; do
        [ -f "$png" ] || continue
        size=$(basename "$png" .png | sed 's/.*-//')
        mkdir -p "$HOME/.local/share/icons/hicolor/${size}x${size}/apps"
        cp "$png" "$HOME/.local/share/icons/hicolor/${size}x${size}/apps/x-notify-service.png"
    done
    gtk-update-icon-cache -f -t "$HOME/.local/share/icons/hicolor" 2>/dev/null || true
fi

case ":$PATH:" in
    *":$BIN_DIR:"*) ;;
    *) echo "提示: 建议将 $BIN_DIR 加入 PATH" ;;
esac
echo "安装完成: 开机自启 + x-notify:// 协议已注册, 服务已启动"
echo "卸载: $BIN_DIR/x-notify-service uninstall"
