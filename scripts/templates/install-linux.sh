#!/bin/sh
# Linux 用户级安装；可由文件管理器直接双击运行。
set -eu

SRC_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
DATA_ROOT=${XDG_DATA_HOME:-$HOME/.local/share}
CONFIG_ROOT=${XDG_CONFIG_HOME:-$HOME/.config}
STATE_ROOT=${XDG_STATE_HOME:-$HOME/.local/state}
APP_DIR=$DATA_ROOT/Hexinfo/x-notify-service
CONF_DIR=$CONFIG_ROOT/Hexinfo/x-notify-service
BIN_DIR=$HOME/.local/bin
OLD_BIN=$BIN_DIR/x-notify-service
NEW_BIN=$APP_DIR/x-notify-service
STEP='安装准备'

show_result() {
    status=$1
    if [ "$status" -eq 0 ]; then
        title='x-notify-service 安装成功'
        message="服务已安装并启动。配置：$CONF_DIR/config.toml"
    else
        title='x-notify-service 安装失败'
        message="失败步骤：${STEP}（退出码 ${status}）。请在终端运行 install.sh 查看详情。"
    fi
    if [ -x "$NEW_BIN" ] && "$NEW_BIN" notify --title "$title" --body "$message" </dev/null >/dev/null 2>&1; then return; fi
    if [ -x "$SRC_DIR/bin/x-notify-service" ] && "$SRC_DIR/bin/x-notify-service" notify --title "$title" --body "$message" </dev/null >/dev/null 2>&1; then return; fi
    if command -v zenity >/dev/null 2>&1 && zenity --info --title="$title" --text="$message" </dev/null >/dev/null 2>&1; then return; fi
    if command -v kdialog >/dev/null 2>&1 && kdialog --msgbox "$message" --title "$title" </dev/null >/dev/null 2>&1; then return; fi
    if command -v notify-send >/dev/null 2>&1 && notify-send "$title" "$message" >/dev/null 2>&1; then return; fi
    echo "$title：$message" >&2
}
on_exit() {
    status=$?
    trap - EXIT
    show_result "$status" || true
    exit "$status"
}
trap on_exit EXIT

STEP='检查安装包'
for file in "$SRC_DIR/bin/x-notify-service" "$SRC_DIR/config/config.toml" "$SRC_DIR/sdk.js" "$SRC_DIR/sdk.umd.js" "$SRC_DIR/sdk-使用手册.md"; do
    [ -f "$file" ] || { echo "安装包缺少：$file" >&2; exit 1; }
done
"$SRC_DIR/bin/x-notify-service" --version >/dev/null

STEP='卸载旧版本'
if [ -f "$OLD_BIN" ] && [ ! -L "$OLD_BIN" ] && [ -x "$OLD_BIN" ]; then
    "$OLD_BIN" uninstall
fi
if [ -x "$NEW_BIN" ] && [ ! -L "$NEW_BIN" ]; then
    # stop 内部校验 port 中实例的 /health 身份，不按 PID 文件直接 kill。
    "$NEW_BIN" stop
fi

STEP='清理旧程序入口'
if [ -e "$OLD_BIN" ] || [ -L "$OLD_BIN" ]; then rm -f -- "$OLD_BIN"; fi

STEP='放置程序和 SDK'
mkdir -p -- "$APP_DIR" "$CONF_DIR" "$BIN_DIR"
cp -- "$SRC_DIR/bin/x-notify-service" "$APP_DIR/.x-notify-service.new"
chmod +x "$APP_DIR/.x-notify-service.new"
mv -f -- "$APP_DIR/.x-notify-service.new" "$NEW_BIN"
cp -- "$SRC_DIR/sdk.js" "$SRC_DIR/sdk.umd.js" "$SRC_DIR/sdk-使用手册.md" "$APP_DIR/"

STEP='生成配置'
if [ ! -f "$CONF_DIR/config.toml" ]; then cp -- "$SRC_DIR/config/config.toml" "$CONF_DIR/config.toml"; fi
ln -s -- "$NEW_BIN" "$OLD_BIN"

STEP='安装图标'
if [ -d "$SRC_DIR/icons/hicolor" ]; then
    for png in "$SRC_DIR"/icons/hicolor/x-notify-service-*.png; do
        [ -f "$png" ] || continue
        size=$(basename "$png" .png | sed 's/.*-//')
        icon_dir=$DATA_ROOT/icons/hicolor/${size}x${size}/apps
        mkdir -p -- "$icon_dir"
        cp -- "$png" "$icon_dir/x-notify-service.png"
    done
    if command -v gtk-update-icon-cache >/dev/null 2>&1; then gtk-update-icon-cache -f -t "$DATA_ROOT/icons/hicolor" 2>/dev/null || true; fi
fi

STEP='注册并启动服务'
"$NEW_BIN" install

STEP='清理旧版配置和数据'
for old_dir in "$CONFIG_ROOT/x-notify-service" "$DATA_ROOT/x-notify-service" "$STATE_ROOT/x-notify-service"; do
    if [ -d "$old_dir" ] && [ ! -L "$old_dir" ]; then rm -r -- "$old_dir"; fi
    if [ -L "$old_dir" ]; then rm -f -- "$old_dir"; fi
done
echo "安装完成：$NEW_BIN；配置：$CONF_DIR/config.toml"
