//! 正文 HTML 子集(加粗/颜色/字号/换行/实体)的解析与排版输出。
//!
//! 流水线:`parse`(标签与实体 → 带样式逻辑行)→ `wrap`(估宽折行 + 行首禁则)
//! → 结构化物理行(`Line`/`Run`,渲染层直接映射富文本 span)。
//! 中间类型对本模块外不可见,调用方只依赖入口:`parse`、`to_lines`、`to_plain_text`。

mod attr;
mod entity;
mod parse;
mod wrap;

/// 正文默认字号(缺省与折行估宽的基准)
pub const BASE_FONT_SIZE: u16 = 14;

/// 正文排版限制:由弹窗尺寸推导,折行与截断共用
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// 14px 基准下每行估宽容量(单位数;实际容量随本行字号反比缩放)
    pub line_units: f64,
    /// 最大行数,超出截断加 …
    pub max_lines: usize,
}

impl Limits {
    /// 纯文本提取用:不折行不截断(系统通知兜底自管排版)
    pub const UNBOUNDED: Self = Self {
        line_units: f64::INFINITY,
        max_lines: usize::MAX,
    };
}

/// 解析结果:折行后的物理行(对外不透明)
pub struct Parsed {
    lines: wrap::WrappedLines,
}

/// 物理行:折行后的正文行,携带本行生效字号(字号按行,不进 span)
pub struct Line {
    pub runs: Vec<Run>,
    pub size: u16,
}

/// 富文本片段:样式只含加粗与颜色
pub struct Run {
    pub text: String,
    pub color: Option<(u8, u8, u8)>,
    pub bold: bool,
}

/// 解析 HTML 子集并按 `limits` 完成折行(行容量/行数上限随弹窗尺寸)
pub fn parse(html: &str, limits: Limits) -> Parsed {
    Parsed {
        lines: wrap::wrap_lines(parse::parse_logical_lines(html), limits),
    }
}

/// 逐行输出结构化物理行(渲染层按 span 映射)。
// Vec 装配无法 const 化;nursery 误报,豁免
#[allow(clippy::missing_const_for_fn)]
pub fn to_lines(parsed: &Parsed) -> Vec<Line> {
    parsed
        .lines
        .0
        .iter()
        .map(|line| Line {
            runs: line
                .runs
                .iter()
                .map(|r| Run {
                    text: r.text.clone(),
                    color: r.style.color.map(|c| (c.0, c.1, c.2)),
                    bold: r.style.bold,
                })
                .collect(),
            // 门面边界:内部 FontSize 收口为裸 u16,调用方无需感知该类型
            size: line.size.map_or(BASE_FONT_SIZE, |f| f.0),
        })
        .collect()
}

/// 提取纯文本(供系统通知兜底;不折行)
pub fn to_plain_text(html: &str) -> String {
    parse(html, Limits::UNBOUNDED)
        .lines
        .0
        .iter()
        .map(|l| l.runs.iter().map(|r| r.text.as_str()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

/// 估宽系数(CJK 记 1 单位、其余 0.55):标题等外置截断与折行共用同一套估计。
// 薄转发;nursery 误报,豁免
#[allow(clippy::missing_const_for_fn)]
pub fn char_units(c: char) -> f64 {
    wrap::char_units(c)
}
