#!/usr/bin/env bash
# 打包 Windows 交付物(正式):NSIS 安装器 setup.exe(per-user 免 UAC、可选安装目录、LZMA 压缩)
# 用法: scripts/pack-windows.sh [target]
#   默认 target: x86_64-pc-windows-msvc(与 CI/release 一致;原生 Windows 编译机执行)
set -euo pipefail
cd "$(dirname "$0")/.."

TARGET="${1:-x86_64-pc-windows-msvc}"
VERSION="$(grep -m1 '^version' Cargo.toml | sed 's/version = "\(.*\)"/\1/')"
STAGE="dist/.stage-windows"

# 先构建 JSSDK:build.rs 在编译期把 dist 内嵌进二进制(演示页同源引用),
# 顺序颠倒会把占位桩嵌进正式包
echo "==> pnpm build sdk"
(cd sdk/js && pnpm install --silent && pnpm -F @hexinfo/x-notify-service-sdk build)

echo "==> cargo build --release --target $TARGET"
cargo build --release --target "$TARGET"

rm -rf "$STAGE"
mkdir -p "$STAGE" dist
cp "target/$TARGET/release/x-notify-service.exe" "$STAGE/"
cp scripts/templates/config.toml "$STAGE/config.toml"
cp sdk/js/packages/sdk/dist/x-notify-service-sdk.js "$STAGE/sdk.js"
cp sdk/js/packages/sdk/dist/x-notify-service-sdk.umd.js "$STAGE/sdk.umd.js"
cp assets/sdk-使用手册.md "$STAGE/sdk-manual.md"
cp assets/icons/x-notify-service.ico "$STAGE/"

echo "==> makensis"
makensis -NOCD -DSTAGE="$STAGE" -DVERSION="$VERSION" scripts/pack-windows.nsi
rm -rf "$STAGE"
echo "==> 产出: dist/x-notify-service-$VERSION-windows-x86_64-setup.exe"
ls -lh "dist/x-notify-service-$VERSION-windows-x86_64-setup.exe" | awk '{print $5, $9}'
