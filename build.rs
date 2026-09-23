// UI 编译失败即构建失败,panic 是 build script 的标准失败方式
#![allow(clippy::unwrap_used, clippy::panic)]

fn main() {
    let target = std::env::var("TARGET").unwrap_or_default();
    let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());

    // Windows:嵌入图标资源(交叉编译 windres;文件管理器/任务栏显示)。
    // 本地无 windres(纯 cargo check 场景)跳过并告警,CI 打包机恒有 mingw 工具链
    if target.contains("windows")
        && let Err(e) = winresource::WindowsResource::new()
            .set_icon("assets/icons/x-notify-service.ico")
            .compile()
    {
        if e.kind() == std::io::ErrorKind::NotFound {
            println!("cargo:warning=未找到 windres,跳过图标嵌入(本地开发环境)");
        } else {
            panic!("图标资源编译失败: {e}");
        }
    }

    // 内嵌演示页用的 SDK:dist 存在则复制,否则写占位(纯 cargo build 不依赖 pnpm;
    // 正式打包流程 pack-linux/pack-windows 均先构建 SDK 再 cargo build,嵌入的是真产物)
    let sdk_dist = std::path::Path::new("sdk/js/packages/sdk/dist/x-notify-service-sdk.js");
    let content = std::fs::read_to_string(sdk_dist).unwrap_or_else(|_| {
        "// sdk.js 占位:先在 sdk/js 下 pnpm -F @hexinfo/x-notify-service-sdk build".to_string()
    });
    std::fs::write(out_dir.join("embedded-sdk.js"), content).unwrap();
    println!("cargo:rerun-if-changed={}", sdk_dist.display());
}
