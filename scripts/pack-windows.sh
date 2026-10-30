#!/usr/bin/env bash
# 打包 Windows 交付物(正式):NSIS 安装器 setup.exe(per-user 免 UAC、可选安装目录、LZMA 压缩)
# 与 Linux 双架构合一 tar.xz 同思路:一个 setup.exe 内含 x64/x86 双二进制,安装时按系统
# 位数自动选——32 位老 Win7 装 i686 版,64 位 Win7~Win11 装 x64 版,用户无需选包。
#
# Win7 兼容:*-win7-windows-msvc 目标(tier 3,无预编译 std)由 nightly -Zbuild-std
# 从源码编 std,std 内部自动降级 GetSystemTimePreciseAsFileTime、延迟加载 WaitOnAddress
# 族、去掉 ProcessPrng 硬导入;getrandom 走 windows_legacy(RtlGenRandom)后端;
# +crt-static 静态链接 CRT,目标机无需 VC++ 运行库或 UCRT 补丁。
#
# 用法: scripts/pack-windows.sh [x64_target] [x86_target]
#   默认 x86_64-win7-windows-msvc / i686-win7-windows-msvc(与 CI/release 一致;
#   原生 Windows 编译机执行,需 nightly + rust-src 组件与 NSIS)
set -euo pipefail
cd "$(dirname "$0")/.."

X64_TARGET="${1:-x86_64-win7-windows-msvc}"
X86_TARGET="${2:-i686-win7-windows-msvc}"
VERSION="$(grep -m1 '^version' Cargo.toml | sed 's/version = "\(.*\)"/\1/')"
STAGE="dist/.stage-windows"
# Windows runner 无 python3 命令名,本地无 python;两个环境各取所需
PY="$(command -v python3 || command -v python)"

# 先构建 JSSDK:build.rs 在编译期把 dist 内嵌进二进制(演示页同源引用),
# 顺序颠倒会把占位桩嵌进正式包
echo "==> pnpm build sdk"
(cd sdk/js && pnpm install --silent && pnpm -F @hexinfo/x-notify-service-sdk build)

export RUSTFLAGS='-C target-feature=+crt-static -C link-arg=/LARGEADDRESSAWARE --cfg getrandom_backend="windows_legacy"'

for T in "$X64_TARGET" "$X86_TARGET"; do
    echo "==> cargo build --release --target $T (-Zbuild-std)"
    cargo build --release --target "$T" -Zbuild-std=std
    # Win7 兼容闸门:导入表禁止 Win8+/Win10+ 硬导入与动态 CRT 依赖(防回归)
    "$PY" scripts/ci/check_win7_imports.py "target/$T/release/x-notify-service.exe"
done

rm -rf "$STAGE"
mkdir -p "$STAGE" dist
cp "target/$X64_TARGET/release/x-notify-service.exe" "$STAGE/x-notify-service.exe"
cp "target/$X86_TARGET/release/x-notify-service.exe" "$STAGE/x-notify-service-x86.exe"
cp scripts/templates/config.toml "$STAGE/config.toml"
cp sdk/js/packages/sdk/dist/x-notify-service-sdk.js "$STAGE/sdk.js"
cp sdk/js/packages/sdk/dist/x-notify-service-sdk.umd.js "$STAGE/sdk.umd.js"
cp assets/sdk-使用手册.md "$STAGE/sdk-manual.md"
cp assets/icons/x-notify-service.ico "$STAGE/"

echo "==> makensis"
makensis -NOCD -DSTAGE="$STAGE" -DVERSION="$VERSION" scripts/pack-windows.nsi
rm -rf "$STAGE"
echo "==> 产出: dist/x-notify-service-$VERSION-windows-setup.exe(单包,32/64 位安装时自动选)"
ls -lh "dist/x-notify-service-$VERSION-windows-setup.exe" | awk '{print $5, $9}'
