// UI 编译失败即构建失败,panic 是 build script 的标准失败方式
#![allow(clippy::unwrap_used, clippy::panic)]

/// 0-255 的十进制字面量(避免热循环里 format!)
const fn itoa_octet(octet: u8) -> &'static str {
    const TABLE: [&str; 256] = [
        "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "14", "15", "16",
        "17", "18", "19", "20", "21", "22", "23", "24", "25", "26", "27", "28", "29", "30", "31",
        "32", "33", "34", "35", "36", "37", "38", "39", "40", "41", "42", "43", "44", "45", "46",
        "47", "48", "49", "50", "51", "52", "53", "54", "55", "56", "57", "58", "59", "60", "61",
        "62", "63", "64", "65", "66", "67", "68", "69", "70", "71", "72", "73", "74", "75", "76",
        "77", "78", "79", "80", "81", "82", "83", "84", "85", "86", "87", "88", "89", "90", "91",
        "92", "93", "94", "95", "96", "97", "98", "99", "100", "101", "102", "103", "104", "105",
        "106", "107", "108", "109", "110", "111", "112", "113", "114", "115", "116", "117", "118",
        "119", "120", "121", "122", "123", "124", "125", "126", "127", "128", "129", "130", "131",
        "132", "133", "134", "135", "136", "137", "138", "139", "140", "141", "142", "143", "144",
        "145", "146", "147", "148", "149", "150", "151", "152", "153", "154", "155", "156", "157",
        "158", "159", "160", "161", "162", "163", "164", "165", "166", "167", "168", "169", "170",
        "171", "172", "173", "174", "175", "176", "177", "178", "179", "180", "181", "182", "183",
        "184", "185", "186", "187", "188", "189", "190", "191", "192", "193", "194", "195", "196",
        "197", "198", "199", "200", "201", "202", "203", "204", "205", "206", "207", "208", "209",
        "210", "211", "212", "213", "214", "215", "216", "217", "218", "219", "220", "221", "222",
        "223", "224", "225", "226", "227", "228", "229", "230", "231", "232", "233", "234", "235",
        "236", "237", "238", "239", "240", "241", "242", "243", "244", "245", "246", "247", "248",
        "249", "250", "251", "252", "253", "254", "255",
    ];
    TABLE[octet as usize]
}

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

    // Linux 多档窗口图标:构建期把 PNG 解码并直接产出 X11 线序字节流,
    // 运行时零解码零拷贝(依赖树的 png 仅存在于构建图);仅 Linux 目标生成
    if target.contains("linux") {
        let mut out = String::from(
            "/// 构建期生成:_NET_WM_ICON 线序负载(小端 u32 流,含各档宽高)\npub(super) const ICON_BLOB: &[u8] = &[\n",
        );
        for size in [128u32, 48, 32, 16] {
            let path = format!("assets/icons/hicolor/x-notify-service-{size}.png");
            let data = std::fs::read(&path).unwrap();
            let decoder = png::Decoder::new(data.as_slice());
            let mut reader = decoder.read_info().unwrap();
            let mut buf = vec![0u8; reader.output_buffer_size()];
            let info = reader.next_frame(&mut buf).unwrap();
            let push_le = |out: &mut String, value: u32| {
                for octet in value.to_le_bytes() {
                    out.push_str(itoa_octet(octet));
                    out.push(',');
                }
            };
            push_le(&mut out, info.width);
            push_le(&mut out, info.height);
            let (chunks, _) = buf.as_chunks::<4>();
            for px in chunks {
                let [red, green, blue, alpha] = *px;
                let argb = (u32::from(alpha) << 24)
                    | (u32::from(red) << 16)
                    | (u32::from(green) << 8)
                    | u32::from(blue);
                push_le(&mut out, argb);
            }
            out.push('\n');
            println!("cargo:rerun-if-changed={path}");
        }
        out.push_str("];\n");
        std::fs::write(out_dir.join("window_icons.rs"), out).unwrap();
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
