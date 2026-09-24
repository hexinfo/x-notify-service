#!/bin/sh
# Shell-level installer contract using a mock package binary.
set -eu

test_root=$(mktemp -d)
trap 'rm -rf "$test_root"' EXIT
package="$test_root/package"
    mkdir -p "$package/bin" "$package/config"
    cp "$(dirname "$0")/../templates/install-linux.sh" "$package/install.sh"
    printf 'template = true\n' > "$package/config/config.toml"
    printf 'new sdk\n' > "$package/sdk.js"
    printf 'new umd\n' > "$package/sdk.umd.js"
    printf 'manual\n' > "$package/sdk-使用手册.md"
    cat > "$package/bin/x-notify-service" <<'MOCK'
#!/bin/sh
case $1 in
    notify)
        printf '%s\n' "$*" >> "$MOCK_NOTIFY_LOG"
        exit "${MOCK_NOTIFY_STATUS:-0}"
        ;;
    install)
        for root in "${XDG_DATA_HOME:-$HOME/.local/share}" "${XDG_CONFIG_HOME:-$HOME/.config}" "${XDG_STATE_HOME:-$HOME/.local/state}"; do
            old="$root/x-notify-service"
            new="$root/Hexinfo/x-notify-service"
            [ -d "$old" ] || continue
            mkdir -p "$new"
            for item in "$old"/*; do
                [ -e "$item" ] || continue
                if [ ! -e "$new/${item##*/}" ]; then mv "$item" "$new/"; fi
            done
            rmdir "$old" 2>/dev/null || true
        done
        ;;
esac
MOCK
    chmod +x "$package/bin/x-notify-service"

export HOME="$test_root/home"
export XDG_DATA_HOME="$test_root/data root"
export XDG_CONFIG_HOME="$test_root/config root"
export XDG_STATE_HOME="$test_root/state root"
export MOCK_NOTIFY_LOG="$test_root/notify.log"
mkdir -p "$HOME" "$XDG_DATA_HOME/x-notify-service" "$XDG_CONFIG_HOME/x-notify-service" "$XDG_STATE_HOME/x-notify-service"
printf 'port = 17321\n' > "$XDG_CONFIG_HOME/x-notify-service/config.toml"
printf 'old sdk\n' > "$XDG_DATA_HOME/x-notify-service/sdk.js"
printf 'unknown\n' > "$XDG_DATA_HOME/x-notify-service/user.txt"
printf 'old log\n' > "$XDG_STATE_HOME/x-notify-service/service.log"
printf '{"pid":%s}\n' "$$" > "$XDG_DATA_HOME/x-notify-service/port"

sh "$package/install.sh" </dev/null
program="$XDG_DATA_HOME/Hexinfo/x-notify-service/bin/x-notify-service"
test -x "$program"
test "$(readlink "$HOME/.local/bin/x-notify-service")" = "$program"
test "$(cat "$XDG_CONFIG_HOME/Hexinfo/x-notify-service/config.toml")" = 'port = 17321'
test "$(cat "$XDG_DATA_HOME/Hexinfo/x-notify-service/user.txt")" = unknown
test "$(cat "$XDG_STATE_HOME/Hexinfo/x-notify-service/service.log")" = 'old log'
test ! -d "$XDG_CONFIG_HOME/x-notify-service"
test ! -d "$XDG_STATE_HOME/x-notify-service"
test ! -d "$XDG_DATA_HOME/x-notify-service"
test "$(cat "$XDG_DATA_HOME/Hexinfo/x-notify-service/sdk.js")" = 'new sdk'
grep -q '安装成功' "$MOCK_NOTIFY_LOG"
kill -0 "$$"

printf 'new config\n' > "$XDG_CONFIG_HOME/Hexinfo/x-notify-service/config.toml"
mkdir -p "$XDG_CONFIG_HOME/x-notify-service"
printf 'old conflict\n' > "$XDG_CONFIG_HOME/x-notify-service/config.toml"
sh "$package/install.sh" </dev/null
test "$(cat "$XDG_CONFIG_HOME/Hexinfo/x-notify-service/config.toml")" = 'new config'
test "$(cat "$XDG_CONFIG_HOME/x-notify-service/config.toml")" = 'old conflict'

# The package binary fails to present its result; a desktop dialog must take over.
mkdir -p "$test_root/tools"
cat > "$test_root/tools/zenity" <<'MOCK'
#!/bin/sh
printf '%s\n' "$*" >> "$MOCK_ZENITY_LOG"
MOCK
chmod +x "$test_root/tools/zenity"
export PATH="$test_root/tools:$PATH"
export MOCK_ZENITY_LOG="$test_root/zenity.log"
export MOCK_NOTIFY_STATUS=1
rm "$package/sdk.umd.js"
if sh "$package/install.sh" </dev/null; then
    echo 'expected missing SDK to fail' >&2
    exit 1
fi
grep -q -- '--error' "$MOCK_ZENITY_LOG"
grep -q '安装失败' "$MOCK_NOTIFY_LOG"
echo 'PASS: Linux installer paths, migration, visible feedback and failure status'
