#!/bin/sh
set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
INSTALLER=$SCRIPT_DIR/../templates/install-linux.sh
TMP=$(mktemp -d)
trap 'rm -r -- "$TMP"' EXIT
HOME=$TMP/home
XDG_DATA_HOME=$HOME/'data with spaces'
XDG_CONFIG_HOME=$HOME/'config with spaces'
XDG_STATE_HOME=$HOME/'state with spaces'
export HOME XDG_DATA_HOME XDG_CONFIG_HOME XDG_STATE_HOME
PKG=$TMP/package
mkdir -p "$PKG/bin" "$PKG/config" "$PKG/icons" "$HOME/.local/bin" "$XDG_CONFIG_HOME/x-notify-service" "$XDG_DATA_HOME/x-notify-service" "$XDG_STATE_HOME/x-notify-service"
cp -R "$SCRIPT_DIR/../../assets/icons/hicolor" "$PKG/icons/"
[ -f "$PKG/icons/hicolor/x-notify-service-32.png" ]
cp "$INSTALLER" "$PKG/install.sh"
chmod +x "$PKG/install.sh"
printf 'new config\n' > "$PKG/config/config.toml"
printf 'sdk\n' > "$PKG/sdk.js"
printf 'umd\n' > "$PKG/sdk.umd.js"
printf 'manual\n' > "$PKG/sdk-使用手册.md"
printf 'old config\n' > "$XDG_CONFIG_HOME/x-notify-service/config.toml"
printf 'old data\n' > "$XDG_DATA_HOME/x-notify-service/port"
printf 'old log\n' > "$XDG_STATE_HOME/x-notify-service/service.log"
cat > "$HOME/.local/bin/x-notify-service" <<'EOF'
#!/bin/sh
printf '%s\n' "$1" >> "$HOME/old-calls"
EOF
chmod +x "$HOME/.local/bin/x-notify-service"
cat > "$PKG/bin/x-notify-service" <<'EOF'
#!/bin/sh
printf '%s\n' "$1" >> "$HOME/new-calls"
case "$1" in
    --version) [ ! -f "$HOME/fail-version" ] ;;
    install) [ ! -f "$HOME/fail-install" ] ;;
    notify) [ ! -f "$HOME/fail-notify" ] ;;
esac
EOF
chmod +x "$PKG/bin/x-notify-service"

"$PKG/install.sh" </dev/null >"$TMP/output" 2>&1 || { sed -n '1,80p' "$TMP/output" >&2; exit 1; }
[ "$(cat "$HOME/old-calls")" = uninstall ]
[ "$(grep -c '^install$' "$HOME/new-calls")" = 1 ]
[ "$(grep -c '^notify$' "$HOME/new-calls")" = 1 ]
[ "$(readlink "$HOME/.local/bin/x-notify-service")" = "$XDG_DATA_HOME/Hexinfo/x-notify-service/x-notify-service" ]
[ -f "$XDG_DATA_HOME/Hexinfo/x-notify-service/sdk.js" ]
[ -f "$XDG_DATA_HOME/icons/hicolor/32x32/apps/x-notify-service.png" ]
[ "$(cat "$XDG_CONFIG_HOME/Hexinfo/x-notify-service/config.toml")" = 'new config' ]
[ ! -e "$XDG_CONFIG_HOME/x-notify-service" ]
[ ! -e "$XDG_DATA_HOME/x-notify-service" ]
[ ! -e "$XDG_STATE_HOME/x-notify-service" ]

# 重装只更新新位置，不运行链接指向的新程序的卸载命令。
"$PKG/install.sh" </dev/null >"$TMP/output" 2>&1
[ "$(grep -c '^stop$' "$HOME/new-calls")" = 1 ]
[ "$(wc -l < "$HOME/old-calls" | tr -d ' ')" = 1 ]
[ "$(grep -c '^uninstall$' "$HOME/new-calls" || true)" = 0 ]

# 包内程序无法运行时，现有程序及其配置仍可用。
touch "$HOME/fail-version"
if "$PKG/install.sh" </dev/null >"$TMP/output" 2>&1; then exit 1; fi
[ -L "$HOME/.local/bin/x-notify-service" ]
[ -f "$XDG_CONFIG_HOME/Hexinfo/x-notify-service/config.toml" ]
rm "$HOME/fail-version"

# 无终端且 bundled notify 失败时，失败状态仍传回调用方并显示桌面消息。
touch "$HOME/fail-install" "$HOME/fail-notify"
mkdir -p "$XDG_CONFIG_HOME/x-notify-service" "$XDG_DATA_HOME/x-notify-service" "$XDG_STATE_HOME/x-notify-service"
printf '{"pid":1}\n' > "$XDG_DATA_HOME/x-notify-service/port"
mkdir -p "$TMP/tools"
cat > "$TMP/tools/zenity" <<'EOF'
#!/bin/sh
printf '%s\n' "$*" > "$HOME/dialog"
EOF
chmod +x "$TMP/tools/zenity"
PATH=$TMP/tools:$PATH
export PATH
if "$PKG/install.sh" </dev/null >"$TMP/output" 2>&1; then
    echo 'FAIL: failed install returned success' >&2
    exit 1
fi
[ -f "$HOME/dialog" ] || { sed -n '1,80p' "$TMP/output" >&2; exit 1; }
[ "$(grep -c 'Hexinfo 安装失败' "$HOME/dialog")" = 1 ]
[ -d "$XDG_CONFIG_HOME/x-notify-service" ]
[ -f "$XDG_DATA_HOME/x-notify-service/port" ]
[ -d "$XDG_STATE_HOME/x-notify-service" ]
echo 'PASS: Linux installer mock checks'
