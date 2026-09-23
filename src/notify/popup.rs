//! 弹窗几何、窗口设置与 GUI 探测(纯逻辑,iced 程序体在 `app.rs`)。

/// 弹窗逻辑尺寸(方角白卡,不依赖窗口透明)。
/// 生效优先级:/notify 请求字段 > config.toml(`popup_width`/`popup_height`) > `Size::DEFAULT`
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Size {
    pub width: f64,
    pub height: f64,
}

impl Size {
    /// 默认紧凑横幅:常规通知为标题 + 一两行正文(时间戳折到第二行不被裁)
    pub const DEFAULT: Self = Self {
        width: 327.0,
        height: 106.0,
    };
}

/// 尺寸取值范围(逻辑像素):请求越界由 API 层 422 拒绝,配置越界钳到边界
pub const MIN_W: u16 = 220;
pub const MAX_W: u16 = 800;
pub const MIN_H: u16 = 80;
pub const MAX_H: u16 = 600;

/// 卡片内边距与标题行高(iced 视图为 f32 口径):视图布局与折行容量共用同一组数,防两处漂移
pub const PAD_LEFT: f32 = 20.0;
pub const PAD_RIGHT: f32 = 12.0;
pub const PAD_TOP: f32 = 12.0;
pub const PAD_BOTTOM: f32 = 12.0;
pub const TITLE_ROW_H: f32 = 24.0;
/// 标题行与正文行的间距
pub const ROW_GAP: f32 = 4.0;
/// 正文行高倍数(与视图 rich_text().line_height 一致)
pub const BODY_LINE_HEIGHT: f32 = 1.6;
/// 标题行右侧关闭钮占位(含边距)
const TITLE_TAIL: f64 = 28.0;

/// 弹窗与屏幕右下角边距(物理像素,随 scale 缩放)
const MARGIN: f64 = 14.0;
/// 窗口标题:仅作 WM/任务栏/测试识别用,无框窗口不显示
pub const WINDOW_TITLE: &str = "x-notify-service";

/// 入场滑入距离(px)与时长(ms)
pub const SLIDE_PX: f32 = 26.0;
pub const SLIDE_MS: u64 = 220;

/// 显示后位置复校节奏(对抗 WM 把新映射窗口重摆到默认位)
pub const FIXUP_DELAYS_MS: [u64; 4] = [50, 120, 250, 500];

const TITLE_FONT_PX: f64 = 16.0;

/// 首个 fixup tick 承担 X11 属性兜底重试:窗口新开时原生窗可能尚未可查
/// (每次通知都是新开窗口,不存在旧代码"仅首次窗口"的豁免)
pub const fn should_retry_x11_init(delay_ms: u64) -> bool {
    delay_ms == 50
}

/// 逐轴取生效尺寸:请求值(API 层已校验范围)> 配置值(越界钳到边界并告警)> 默认值。
/// 宽高独立,允许只传其一
pub fn resolve_size(
    req_width: Option<u16>,
    req_height: Option<u16>,
    cfg_width: Option<u16>,
    cfg_height: Option<u16>,
) -> Size {
    Size {
        width: pick_axis(
            req_width,
            cfg_width,
            (MIN_W, MAX_W),
            Size::DEFAULT.width,
            "popup_width",
        ),
        height: pick_axis(
            req_height,
            cfg_height,
            (MIN_H, MAX_H),
            Size::DEFAULT.height,
            "popup_height",
        ),
    }
}

fn pick_axis(
    req: Option<u16>,
    cfg: Option<u16>,
    (min, max): (u16, u16),
    default: f64,
    name: &str,
) -> f64 {
    if let Some(v) = req {
        return f64::from(v);
    }
    if let Some(v) = cfg {
        if !(min..=max).contains(&v) {
            log::warn!("配置项 {name}={v} 越界([{min},{max}]),已按边界生效");
            return f64::from(v.clamp(min, max));
        }
        return f64::from(v);
    }
    default
}

/// 由窗口尺寸推导正文排版限制:每行估宽容量(14px 基准)与最大行数。
/// 行数另受 5 行设计上限约束;行盒高度按基准字号近似(字号大于基准时略紧)
pub fn body_limits(size: Size) -> crate::html::Limits {
    let text_w = size.width - f64::from(PAD_LEFT + PAD_RIGHT);
    let body_h = size.height - f64::from(PAD_TOP + PAD_BOTTOM + TITLE_ROW_H + ROW_GAP);
    let line_px = f64::from(crate::html::BASE_FONT_SIZE) * f64::from(BODY_LINE_HEIGHT);
    crate::html::Limits {
        line_units: text_w / f64::from(crate::html::BASE_FONT_SIZE),
        max_lines: {
            // 钳制后值域 [1,5]:截断与符号位不可能触发
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            {
                (body_h / line_px).floor().clamp(1.0, 5.0) as usize
            }
        },
    }
}

/// 工作区右下角落点(物理像素,已扣除边距)。
/// pub 供 info 子命令展示:诊断值与弹窗实际定位共用同一计算
// round 后截断为整型像素,值域受屏幕尺寸约束,截断即取整语义
#[allow(clippy::cast_possible_truncation)]
pub fn landing(area: &crate::screen::WorkArea, size: Size) -> (i32, i32) {
    let scale = area.scale.max(1.0);
    let w = size.width * scale;
    let h = size.height * scale;
    (
        MARGIN.mul_add(-scale, area.x + area.w - w).round() as i32,
        MARGIN.mul_add(-scale, area.y + area.h - h).round() as i32,
    )
}

/// 落点的逻辑坐标(iced 窗口 API 以逻辑像素计)。
// f64→f32/i32→f32:屏幕 scale 与像素坐标的精度远低于 f32 可表,失真可忽略
#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
pub fn logical_landing(area: &crate::screen::WorkArea, size: Size) -> iced::Point {
    let (x, y) = landing(area, size);
    let scale = area.scale.max(1.0) as f32;
    iced::Point::new(x as f32 / scale, y as f32 / scale)
}

/// ease-out 三次曲线(t∈[0,1] → 进度),入场滑入用
pub const fn ease_out_cubic(t: f32) -> f32 {
    let inv = 1.0 - t;
    1.0 - inv * inv * inv
}

/// 标题单行截断:超宽加省略号(与正文折行共用同一套估宽系数;
/// iced 0.14 的 Text 无 elide,截断在数据层完成)
pub fn elide_title(title: &str, width: f64) -> String {
    let max_units = (width - TITLE_TAIL - f64::from(PAD_LEFT + PAD_RIGHT)) / TITLE_FONT_PX;
    let mut units = 0f64;
    for (idx, ch) in title.char_indices() {
        // 省略号预占 1 单位,避免截断后仍超宽
        if units + crate::html::char_units(ch) > max_units - 1.0 {
            let mut out = title[..idx].to_string();
            out.push('…');
            return out;
        }
        units += crate::html::char_units(ch);
    }
    title.to_owned()
}

/// 启动时探测弹窗 GUI 是否可用:能连上窗口系统并取到工作区即认为可用。
/// 只做无副作用的工作区查询——iced daemon 启动即占据主线程,不能试建窗口
pub fn gui_probe() -> bool {
    #[cfg(target_os = "linux")]
    {
        if std::env::var_os("DISPLAY").is_none() && std::env::var_os("WAYLAND_DISPLAY").is_none() {
            log::warn!("无 DISPLAY/WAYLAND_DISPLAY,弹窗不可用");
            return false;
        }
    }
    let area = crate::screen::work_area();
    if area.is_none() {
        log::warn!("无法连接窗口系统/获取工作区,弹窗不可用");
    }
    area.is_some()
}

/// 构造弹窗窗口设置:置顶/无框/固定尺寸/右下角创建期定位(避免先默认位再跳)。
/// iced 不暴露 winit attributes hook,创建期能力收口到这里:
/// Windows 经 `skip_taskbar` 隐藏任务栏条目;Linux 经 `application_id` 设 WM_CLASS
/// (StartupWMClass 匹配 desktop 条目),_NET_WM_WINDOW_TYPE 等仍由 x11rb 事后补齐。
pub fn window_settings(area: &crate::screen::WorkArea, size: Size) -> iced::window::Settings {
    // f64→f32:窗口逻辑尺寸为整数级数值,无精度损失
    #[allow(clippy::cast_possible_truncation)]
    let logical = iced::Size::new(size.width as f32, size.height as f32);
    iced::window::Settings {
        size: logical,
        position: iced::window::Position::Specific(logical_landing(area, size)),
        resizable: false,
        decorations: false,
        transparent: false,
        level: iced::window::Level::AlwaysOnTop,
        // 关闭请求经 close_requests 订阅处理(关闭语义=销毁窗口,daemon 不退出)
        exit_on_close_request: false,
        platform_specific: platform_specific(),
        // 创建期窗口图标(winit 写 _NET_WM_ICON);x11rb 多档直写仍是兜底主链
        icon: settings_icon(),
        ..iced::window::Settings::default()
    }
}

/// 窗口图标仅 Linux 有构建期烘焙的 ICON_BLOB;其余平台用默认(Windows 走 exe 资源)。
// cfg 分支薄封装;nursery 误报,豁免
#[allow(clippy::missing_const_for_fn)]
fn settings_icon() -> Option<iced::window::Icon> {
    #[cfg(target_os = "linux")]
    {
        crate::notify::window_icon::settings_icon()
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

/// 平台专属设置:Windows 经 `skip_taskbar` 隐藏任务栏条目;
/// Linux 经 `application_id` 设 WM_CLASS(StartupWMClass 匹配 desktop 条目)
fn platform_specific() -> iced::window::settings::PlatformSpecific {
    #[cfg(windows)]
    {
        iced::window::settings::PlatformSpecific {
            skip_taskbar: true,
            ..Default::default()
        }
    }
    #[cfg(target_os = "linux")]
    {
        iced::window::settings::PlatformSpecific {
            application_id: WINDOW_TITLE.to_owned(),
            ..Default::default()
        }
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        iced::window::settings::PlatformSpecific::default()
    }
}

#[cfg(test)]
mod tests {
    use crate::screen::WorkArea;

    use super::{
        MAX_H, MAX_W, MIN_H, MIN_W, Size, body_limits, ease_out_cubic, elide_title, landing,
        resolve_size, should_retry_x11_init,
    };

    #[test]
    fn x11_init_retry_only_runs_at_first_tick() {
        assert!(should_retry_x11_init(50));
        assert!(!should_retry_x11_init(120));
        assert!(!should_retry_x11_init(500));
    }

    #[test]
    fn landing_pins_bottom_right_with_margin() {
        let area = WorkArea {
            x: 0.0,
            y: 0.0,
            w: 1920.0,
            h: 1040.0,
            scale: 1.0,
        };
        let (x, y) = landing(
            &area,
            Size {
                width: 367.0,
                height: 206.0,
            },
        );
        assert_eq!(x, 1920 - 367 - 14);
        assert_eq!(y, 1040 - 206 - 14);
    }

    #[test]
    fn size_resolution_priority() {
        let near = |a: f64, b: f64| (a - b).abs() < 1e-9;
        let d = Size::DEFAULT;
        // 请求 > 配置 > 默认(逐轴独立)
        assert_eq!(
            resolve_size(Some(400), None, Some(500), Some(120)),
            Size {
                width: 400.0,
                height: 120.0,
            }
        );
        assert!(near(resolve_size(None, None, Some(500), None).width, 500.0));
        assert_eq!(resolve_size(None, None, None, None), d);
        // 配置越界:钳到边界
        let clamped = resolve_size(None, None, Some(10), Some(9999));
        assert!(near(clamped.width, f64::from(MIN_W)));
        assert!(near(clamped.height, f64::from(MAX_H)));
        // 默认自身必须在范围内
        assert!((f64::from(MIN_W)..=f64::from(MAX_W)).contains(&d.width));
        assert!((f64::from(MIN_H)..=f64::from(MAX_H)).contains(&d.height));
    }

    #[test]
    fn body_limits_follow_size() {
        // 默认横幅:两行正文容量(时间戳折到第二行不被裁)
        assert_eq!(body_limits(Size::DEFAULT).max_lines, 2);
        // 高窗受 5 行设计上限约束;矮窗至少 1 行
        let tall = body_limits(Size {
            width: 327.0,
            height: 600.0,
        });
        assert_eq!(tall.max_lines, 5);
        let short = body_limits(Size {
            width: 327.0,
            height: f64::from(MIN_H),
        });
        assert_eq!(short.max_lines, 1);
    }

    #[test]
    fn ease_out_cubic_bounds() {
        assert!((ease_out_cubic(0.0) - 0.0).abs() < f32::EPSILON);
        assert!((ease_out_cubic(1.0) - 1.0).abs() < f32::EPSILON);
        assert!(ease_out_cubic(0.5) > 0.5, "ease-out 前半程应过半");
    }

    #[test]
    fn title_elide_truncates_long_and_keeps_short() {
        assert_eq!(elide_title("短标题", 327.0), "短标题");
        let long = "标题".repeat(60);
        let elided = elide_title(&long, 327.0);
        assert!(elided.ends_with('…'));
        assert!(elided.chars().count() < long.chars().count());
        // 窗口越宽可容纳越长标题
        let wider = elide_title(&long, 800.0);
        assert!(wider.chars().count() > elided.chars().count());
    }
}
