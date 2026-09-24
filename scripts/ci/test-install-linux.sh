#!/bin/sh
# Shell-level installer contract using a mock package binary.
set -eu

original_path=$PATH
mode=${1:-package}
if [ "$mode" != --mock-only ] && [ "$#" -gt 1 ]; then
    echo 'usage: test-install-linux.sh [--mock-only | package.tar.xz]' >&2
    exit 2
fi
test_root=$(mktemp -d)
real_program=
cleanup() {
    if [ -n "$real_program" ] && [ -x "$real_program" ]; then
        "$real_program" uninstall >/dev/null 2>&1 || true
    fi
    rm -rf "$test_root"
}
trap cleanup EXIT
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

# On Linux, run a real executable from the managed path and prove reinstall stops it.
if [ -d /proc/self ]; then
    cp "$(command -v sleep)" "$program"
    "$program" 30 &
    running_pid=$!
    printf '{"pid":%s}\n' "$running_pid" > "$XDG_DATA_HOME/Hexinfo/x-notify-service/port"
    sh "$package/install.sh" </dev/null
    if wait "$running_pid"; then
        echo 'reinstall did not stop the running instance' >&2
        exit 1
    fi
    test -x "$program"
    printf '{"pid":%s}\n' "$$" > "$XDG_DATA_HOME/Hexinfo/x-notify-service/port"
    sh "$package/install.sh" </dev/null
    kill -0 "$$"
fi

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
echo 'PASS: mock installer migration, repeat install and visible feedback'

if [ "$mode" != --mock-only ]; then
    PATH=$original_path
    export PATH
    arch=$(uname -m)
    if [ "$mode" = package ]; then
        set -- dist/x-notify-service-*-linux-"$arch".tar.xz
        if [ ! -f "$1" ] || [ "$#" -ne 1 ]; then
            echo "expected exactly one assembled Linux package for $arch" >&2
            exit 1
        fi
        archive=$1
    else
        archive=$mode
    fi
    case ${archive##*/} in
        x-notify-service-*-linux-"$arch".tar.xz) ;;
        *) echo "package architecture does not match host: $archive" >&2; exit 1 ;;
    esac
    pkgname=${archive##*/}
    pkgname=${pkgname%.tar.xz}
    mkdir -p "$test_root/extracted"
    tar -xJf "$archive" -C "$test_root/extracted"
    extracted="$test_root/extracted/$pkgname"
    test -x "$extracted/install.sh"
    test -x "$extracted/bin/x-notify-service"
    test -f "$extracted/sdk.js"
    test -f "$extracted/sdk.umd.js"
    test -f "$extracted/sdk-使用手册.md"
    test -f "$extracted/config/config.toml"

    export HOME="$test_root/real home"
    export XDG_DATA_HOME="$test_root/real data root"
    export XDG_CONFIG_HOME="$test_root/real config root"
    export XDG_STATE_HOME="$test_root/real state root"
    mkdir -p "$HOME" "$XDG_CONFIG_HOME/x-notify-service" "$XDG_STATE_HOME/x-notify-service"
    printf 'log_level = "warn"\n' > "$XDG_CONFIG_HOME/x-notify-service/config.toml"
    printf 'old log\n' > "$XDG_STATE_HOME/x-notify-service/previous.log"
    real_program="$XDG_DATA_HOME/Hexinfo/x-notify-service/bin/x-notify-service"
    sh "$extracted/install.sh" </dev/null
    test -x "$real_program"
    test "$(readlink "$HOME/.local/bin/x-notify-service")" = "$real_program"
    test "$(cat "$XDG_CONFIG_HOME/Hexinfo/x-notify-service/config.toml")" = 'log_level = "warn"'
    test "$(cat "$XDG_STATE_HOME/Hexinfo/x-notify-service/previous.log")" = 'old log'
    test ! -d "$XDG_CONFIG_HOME/x-notify-service"
    test ! -d "$XDG_STATE_HOME/x-notify-service"
    cmp "$extracted/sdk.js" "$XDG_DATA_HOME/Hexinfo/x-notify-service/sdk.js"
    cmp "$extracted/sdk.umd.js" "$XDG_DATA_HOME/Hexinfo/x-notify-service/sdk.umd.js"
    cmp "$extracted/sdk-使用手册.md" "$XDG_DATA_HOME/Hexinfo/x-notify-service/sdk-使用手册.md"
    attempts=0
    while [ ! -f "$XDG_DATA_HOME/Hexinfo/x-notify-service/port" ] && [ "$attempts" -lt 50 ]; do
        sleep 0.1
        attempts=$((attempts + 1))
    done
    test -f "$XDG_DATA_HOME/Hexinfo/x-notify-service/port"
    "$real_program" uninstall
    test ! -f "$XDG_DATA_HOME/Hexinfo/x-notify-service/port"
    real_program=
    echo "PASS: assembled $arch package installation and cleanup"
fi
