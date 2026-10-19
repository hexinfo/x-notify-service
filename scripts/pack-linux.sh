#!/usr/bin/env bash
# 打包 Linux 交付物(正式):双架构合一 tar.xz 绿色版
# (bin/{x86_64,aarch64} + config + demo + sdk + install.sh + 图标;
#  install.sh 按 uname -m 自动选用本机架构二进制,用户不再挑包)
#
# 兼容性关键:一律在 Debian 10(glibc 2.28)容器内编译。glibc 向后兼容,
# 产出可跑在 glibc ≥ 2.28 的新老系统(UOS 20/麒麟 V10 ~ 最新发行版);
# 组装前断言二进制 GLIBC 需求 ≤ 2.28,防止构建环境漂移破坏兼容性。
#
# CI 分工:两架构在不同原生 runner 各自 build,由组装 job 下载两份二进制
# 执行 pack(单机交叉编译 arm64 走 qemu 模拟,慢数倍,不可取)。
# 环境变量:
#   XNS_BUILD_IMAGE   构建镜像,默认 debian:10-slim(多架构,amd64/arm64 原生)
#   XNS_DOCKER_PROXY  容器内代理,如 http://host.docker.internal:7890(无则不走代理)
set -euo pipefail
cd "$(dirname "$0")/.."

VERSION="$(grep -m1 '^version' Cargo.toml | sed 's/version = "\(.*\)"/\1/')"
IMAGE="${XNS_BUILD_IMAGE:-debian:10-slim}"
GLIBC_FLOOR=28   # 兼容地板:GLIBC_2.28(Debian 10)
BIN_NAME=x-notify-service

usage() {
    cat >&2 <<'EOF'
用法:
  scripts/pack-linux.sh build <arch>                     容器内编译单架构二进制(x86_64|aarch64)
  scripts/pack-linux.sh pack <x86_64-bin> <aarch64-bin>  构建 SDK 并组装双架构合一 tar.xz
  scripts/pack-linux.sh all                              本机完整流程(build 两架构 + pack)
EOF
    exit 1
}

need_docker() { command -v docker >/dev/null 2>&1 || { echo "需要 Docker(编译固定在 Debian 10 容器内)" >&2; exit 1; }; }

# 宿主机构建 JS(容器只负责编译二进制)
build_js() {
    (cd sdk/js && pnpm install --silent && pnpm -F @hexinfo/x-notify-service-sdk build)
}

# 容器内原生编译:platform = docker 平台,archname = target 子目录名
build_in_docker() { # $1=platform $2=archname
    local proxy_env=()
    if [ -n "${XNS_DOCKER_PROXY:-}" ]; then
        proxy_env=(-e "http_proxy=$XNS_DOCKER_PROXY" -e "https_proxy=$XNS_DOCKER_PROXY"
            -e "HTTP_PROXY=$XNS_DOCKER_PROXY" -e "HTTPS_PROXY=$XNS_DOCKER_PROXY"
            -e "no_proxy=localhost,127.0.0.1")
    fi
    mkdir -p .ci-cache/cargo target
    docker run --rm --platform "$1" -v "$PWD":/work -w /work \
        -v "$PWD/.ci-cache/cargo":/cargo \
        -e HOST_UID="$(id -u)" -e HOST_GID="$(id -g)" \
        "${proxy_env[@]+"${proxy_env[@]}"}" \
        -e CARGO_HOME=/cargo -e RUSTUP_HOME=/cargo/rustup \
        -e CARGO_TARGET_DIR=/work/target/linux-$2 \
        "$IMAGE" sh -exc '
        # Debian 10 已 EOL:main 与 security 都改指 archive(security 缺失会导致
        # fontconfig 依赖解析失败)。基础镜像没有 CA 证书，先依靠 apt 签名校验
        # 引导安装 ca-certificates，再恢复 HTTPS 证书校验。
        if grep -q buster /etc/os-release 2>/dev/null; then
            printf "deb https://archive.debian.org/debian buster main\ndeb https://archive.debian.org/debian-security buster/updates main\n" > /etc/apt/sources.list
            n=0
            until apt-get -o Acquire::Check-Valid-Until=false \
                -o Acquire::https::Verify-Peer=false -o Acquire::https::Verify-Host=false update -qq; do
                n=$((n + 1)); [ "$n" -ge 3 ] && exit 1
                sleep 3
            done
            apt-get -o Acquire::Retries=8 \
                -o Acquire::https::Verify-Peer=false -o Acquire::https::Verify-Host=false \
                install -y -qq --no-install-recommends ca-certificates
        else
            apt-get update -qq
        fi
        apt-get -o Acquire::Retries=8 install -y -qq --no-install-recommends \
            build-essential curl ca-certificates xz-utils pkg-config libfontconfig1-dev \
            libxcb1-dev libxkbcommon-dev libxkbcommon-x11-dev
        export PATH="/cargo/bin:$PATH"
        # 缓存恢复的 .ci-cache/cargo 可能是空目录(首次无缓存):rustup 存在但
        # default toolchain 缺失会报"no default is configured",重装兜底
        command -v cargo >/dev/null 2>&1 && cargo --version >/dev/null 2>&1 \
            || curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable --no-modify-path
        cargo build --release
        # 容器内 root 写的文件会让 host 上 tar(actions/cache)Permission denied,
        # 退出前改属主为 host 侧 runner 用户(UID 经 env 传入)
        chown -R "$HOST_UID:$HOST_GID" /cargo /work/target 2>/dev/null || true
        '
}

# 兼容性闸门:二进制 GLIBC 需求不得超过地板(用最大次版本号数值比较,兼容 macOS 的 BSD sort)
assert_glibc() { # $1=二进制
    local worst
    worst=$(strings "$1" | grep -o 'GLIBC_2\.[0-9]*' | sed 's/GLIBC_2\.//' | sort -n | tail -1)
    if [ "${worst:-0}" -gt "$GLIBC_FLOOR" ]; then
        echo "FAIL: 二进制 GLIBC_2.$worst 超过兼容地板 2.$GLIBC_FLOOR(构建环境漂移?)" >&2
        exit 1
    fi
    echo "PASS: GLIBC 需求 ≤ 2.$GLIBC_FLOOR(实际最高 GLIBC_2.${worst:-无})"
}

# 组装双架构合一 tar.xz:$1=x86_64 二进制 $2=aarch64 二进制
assemble() {
    local pkg="x-notify-service-${VERSION}-linux"
    local out="dist/${pkg}"
    assert_glibc "$1"
    assert_glibc "$2"
    rm -rf "$out"
    mkdir -p "$out/bin/x86_64" "$out/bin/aarch64" "$out/config" "$out/icons"
    cp "$1" "$out/bin/x86_64/$BIN_NAME"
    cp "$2" "$out/bin/aarch64/$BIN_NAME"
    cp scripts/templates/config.toml "$out/config/"
    cp scripts/templates/install-linux.sh "$out/install.sh"
    chmod +x "$out/install.sh"
    cp assets/sdk-使用手册.md "$out/sdk-使用手册.md"
    cp sdk/js/packages/sdk/dist/x-notify-service-sdk.js "$out/sdk.js"
    cp sdk/js/packages/sdk/dist/x-notify-service-sdk.umd.js "$out/sdk.umd.js"
    cp -R assets/icons/hicolor "$out/icons/"
    tar -cJf "dist/${pkg}.tar.xz" -C dist "$pkg"
    rm -rf "$out"
    ls -lh "dist/${pkg}.tar.xz" | awk '{print "==> 产出:", $5, $9}'
}

[ $# -ge 1 ] || usage
case "$1" in
    build)
        [ $# -eq 2 ] || usage
        case "$2" in
            x86_64) platform=linux/amd64 ;;
            aarch64) platform=linux/arm64 ;;
            *) usage ;;
        esac
        need_docker
        echo "==> Docker($IMAGE,$platform)编译 $2"
        build_in_docker "$platform" "$2"
        assert_glibc "target/linux-$2/release/$BIN_NAME"
        ;;
    pack)
        [ $# -eq 3 ] || usage
        [ -f "$2" ] && [ -f "$3" ] || { echo "二进制不存在:$2 或 $3" >&2; exit 1; }
        build_js
        assemble "$2" "$3"
        ;;
    all)
        need_docker
        build_in_docker linux/amd64 x86_64
        build_in_docker linux/arm64 aarch64
        build_js
        assemble "target/linux-x86_64/release/$BIN_NAME" "target/linux-aarch64/release/$BIN_NAME"
        ;;
    *) usage ;;
esac
