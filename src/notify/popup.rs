//! 弹窗尺寸、颜色、正文容量和落点计算。

/// 弹窗逻辑尺寸(方角白卡,不依赖窗口透明)。
/// 生效优先级:/notify 请求字段 > `Size::DEFAULT`
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Size {
    pub width: f64,
    pub height: f64,
}

impl Size {
    /// 默认紧凑横幅:常规通知为标题 + 一两行正文(时间戳折到第二行不被裁)
    pub const DEFAULT: Self = Self {
        width: 220.0,
        height: 100.0,
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Colors {
    pub header_background: [u8; 3],
    pub header_text: [u8; 3],
    pub body_background: [u8; 3],
    pub body_text: [u8; 3],
}

impl Colors {
    pub const DEFAULT: Self = Self {
        header_background: [0x27, 0x34, 0x49],
        header_text: [0xff, 0xff, 0xff],
        body_background: [0xf7, 0xf9, 0xfc],
        body_text: [0x3f, 0x47, 0x54],
    };
}

pub fn parse_hex_color(value: &str) -> Option<[u8; 3]> {
    let hex = value.strip_prefix('#')?;
    if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    Some([
        u8::from_str_radix(&hex[0..2], 16).ok()?,
        u8::from_str_radix(&hex[2..4], 16).ok()?,
        u8::from_str_radix(&hex[4..6], 16).ok()?,
    ])
}

pub fn resolve_colors(
    header_background: Option<&str>,
    header_text: Option<&str>,
    body_background: Option<&str>,
    body_text: Option<&str>,
) -> Colors {
    let defaults = Colors::DEFAULT;
    Colors {
        header_background: header_background
            .and_then(parse_hex_color)
            .unwrap_or(defaults.header_background),
        header_text: header_text
            .and_then(parse_hex_color)
            .unwrap_or(defaults.header_text),
        body_background: body_background
            .and_then(parse_hex_color)
            .unwrap_or(defaults.body_background),
        body_text: body_text
            .and_then(parse_hex_color)
            .unwrap_or(defaults.body_text),
    }
}

/// 尺寸取值范围(逻辑像素):请求越界由 API 层 422 拒绝
pub const MIN_W: u16 = 220;
pub const MAX_W: u16 = 800;
pub const MIN_H: u16 = 80;
pub const MAX_H: u16 = 600;

/// GPUI 视图使用 38px 标题行、上下各 8px 正文内边距和 14px/1.45 行高。
const HEADER_HEIGHT: f64 = 38.0;
const BODY_PAD_Y: f64 = 8.0;
const BODY_FONT_PX: f64 = 14.0;
const BODY_LINE_HEIGHT: f64 = 1.45;

/// 弹窗与屏幕右下角边距(物理像素,随 scale 缩放)
const MARGIN: f64 = 14.0;
/// 入场滑入距离(px)与时长(ms)
pub const SLIDE_PX: f32 = 26.0;
pub const SLIDE_MS: u64 = 220;

/// 逐轴取生效尺寸:请求值(API/CLI 层已校验范围)> 默认值。宽高独立,允许只传其一
pub fn resolve_size(req_width: Option<u16>, req_height: Option<u16>) -> Size {
    Size {
        width: req_width.map_or(Size::DEFAULT.width, f64::from),
        height: req_height.map_or(Size::DEFAULT.height, f64::from),
    }
}

/// 正文行数限制，超出部分由 GPUI TextView 省略。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BodyLimits {
    pub max_lines: usize,
}

/// 由窗口高度推导正文最大行数，范围为 1 至 5 行。
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub fn body_limits(size: Size) -> BodyLimits {
    let body_h = BODY_PAD_Y.mul_add(-2.0, size.height - HEADER_HEIGHT);
    let line_px = BODY_FONT_PX * BODY_LINE_HEIGHT;
    BodyLimits {
        max_lines: (body_h / line_px).floor().clamp(1.0, 5.0) as usize,
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

/// ease-out 三次曲线(t∈[0,1] → 进度),入场滑入用
pub const fn ease_out_cubic(t: f32) -> f32 {
    let inv = 1.0 - t;
    1.0 - inv * inv * inv
}

#[cfg(test)]
mod tests {
    use crate::screen::WorkArea;

    use super::{
        MAX_H, MAX_W, MIN_H, MIN_W, Size, body_limits, ease_out_cubic, landing, resolve_size,
    };

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
        let d = Size::DEFAULT;
        assert!((d.width - 220.0).abs() < f64::EPSILON);
        assert!((d.height - 100.0).abs() < f64::EPSILON);
        // 请求 > 默认(逐轴独立)
        assert_eq!(
            resolve_size(Some(400), None),
            Size {
                width: 400.0,
                height: d.height,
            }
        );
        assert_eq!(resolve_size(None, None), d);
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
}
