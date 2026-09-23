//! 估宽折行:CJK 记 1 单位、其余 0.55,行容量随该行字号缩放(`Limits` 由弹窗尺寸推导);
//! 行首禁则(`kinsoku`):折行点若使标点成为下一行行首,把前一字符一并带下去;
//! 超过 `Limits::max_lines` 截断,末尾追加 …。

use super::Limits;
use super::attr::FontSize;
use super::parse::{Line, LogicalLines, Run, RunStyle};

/// 物理行(折行后,带本行生效字号)
pub(super) struct LineOut {
    pub runs: Vec<Run>,
    pub size: Option<FontSize>,
}

pub(super) struct WrappedLines(pub Vec<LineOut>);

pub(super) fn wrap_lines(lines: LogicalLines, limits: Limits) -> WrappedLines {
    let LogicalLines(logical) = lines;
    let mut out: Vec<LineOut> = Vec::new();
    let mut truncated = false;
    'outer: for (idx, Line(runs)) in logical.iter().enumerate() {
        let line_size = runs.iter().find_map(|r| r.style.size);
        let font = f64::from(line_size.unwrap_or(FontSize(super::BASE_FONT_SIZE)).0);
        let max_units = limits.line_units * f64::from(super::BASE_FONT_SIZE) / font;
        let mut cur: Vec<Run> = Vec::new();
        let mut units = 0f64;
        for run in runs {
            let mut seg = String::new();
            for ch in run.text.chars() {
                if units + char_units(ch) > max_units {
                    let (head, rest) = split_with_kinsoku(seg, ch);
                    seg = rest;
                    if !seg.is_empty() {
                        cur.push(Run {
                            text: std::mem::take(&mut seg),
                            style: run.style,
                        });
                    }
                    out.push(LineOut {
                        runs: std::mem::take(&mut cur),
                        size: line_size,
                    });
                    units = head.chars().map(char_units).sum();
                    seg = head;
                    // 折行点即预算耗尽:当前字符及其后必被丢弃,是真截断
                    if out.len() >= limits.max_lines {
                        truncated = true;
                        break 'outer;
                    }
                }
                seg.push(ch);
                units += char_units(ch);
            }
            if !seg.is_empty() {
                cur.push(Run {
                    text: seg,
                    style: run.style,
                });
            }
        }
        out.push(LineOut {
            runs: cur,
            size: line_size,
        });
        if out.len() >= limits.max_lines {
            // 恰好填满且其后无非空内容:不是截断,不加 …
            let remains = logical[idx + 1..]
                .iter()
                .any(|l| l.0.iter().any(|r| !r.text.is_empty()));
            if remains {
                truncated = true;
            }
            break;
        }
    }
    if truncated && let Some(last) = out.last_mut() {
        let st = last.runs.last().map_or(
            RunStyle {
                bold: false,
                color: None,
                size: None,
            },
            |r| r.style,
        );
        last.runs.push(Run {
            text: "…".into(),
            style: st,
        });
    }
    WrappedLines(out)
}

pub(super) const fn char_units(c: char) -> f64 {
    if c.is_ascii() { 0.55 } else { 1.0 }
}

/// 行首禁则(`kinsoku`):这些标点不能出现在折行后下一行的开头
const fn is_no_line_start(c: char) -> bool {
    matches!(
        c,
        '，' | '。'
            | '、'
            | '；'
            | '：'
            | '！'
            | '？'
            | '…'
            | '·'
            | ','
            | '.'
            | ';'
            | ':'
            | '!'
            | '?'
            | ')'
            | ']'
            | '}'
            | '%'
            | '）'
            | '》'
            | '〉'
            | '」'
            | '』'
            | '】'
            | '〕'
            | '"'
            | '”'
            | '\''
            | '’'
    )
}

/// 折行点落在禁则标点 `next` 上时,返回 (带下去的行首字符, 留在本行的剩余);
/// 整段都是禁则字符时放弃处理(原样保留)
fn split_with_kinsoku(seg: String, next: char) -> (String, String) {
    if !is_no_line_start(next) {
        return (String::new(), seg);
    }
    let mut rest = seg;
    let mut head = String::new();
    let mut first = next;
    while is_no_line_start(first) {
        match rest.pop() {
            Some(prev) => {
                head.insert(0, prev);
                first = prev;
            }
            None => return (String::new(), head), // 整行禁则:不强行拆
        }
    }
    (head, rest)
}

#[cfg(test)]
mod tests {
    use super::super::Limits;
    use super::super::parse;

    /// 测试基准排版限制(相当于旧版 367px 宽/5 行上限的口径)
    const LIMITS: Limits = Limits {
        line_units: 24.0,
        max_lines: 5,
    };

    /// 行首禁则:折行后任何一行的行首不能是标点
    #[test]
    fn kinsoku_no_punctuation_at_line_start() {
        // 24 个汉字恰好填满一行,逗号将被禁则处理:前一字符带到第二行行首
        let text = format!("{},后续内容继续排列显示", "一".repeat(24));
        let wrapped = super::wrap_lines(parse::parse_logical_lines(&text), LIMITS);
        assert!(wrapped.0.len() >= 2, "应当折行");
        for line in &wrapped.0 {
            if let Some(c) = line.runs.first().and_then(|r| r.text.chars().next()) {
                assert!(!super::is_no_line_start(c), "行首出现禁则标点: {c}");
            }
        }
    }

    /// 恰好填满行数上限不是截断,不加省略号(默认弹窗两行装"待办+时间"场景)
    #[test]
    fn exact_fill_lines_no_ellipsis() {
        let limits = Limits {
            line_units: 24.0,
            max_lines: 2,
        };
        let wrapped = super::wrap_lines(
            parse::parse_logical_lines("待办通知 <b>1</b> 条<br/>15:21:05"),
            limits,
        );
        assert_eq!(wrapped.0.len(), 2);
        let last: String = wrapped.0[1].runs.iter().map(|r| r.text.as_str()).collect();
        assert_eq!(last, "15:21:05", "恰好两行不应追加省略号");
        // 真超出仍截断加 …
        let over = super::wrap_lines(parse::parse_logical_lines("一<br>二<br>三"), limits);
        assert_eq!(over.0.len(), 2);
        let tail: String = over.0[1].runs.iter().map(|r| r.text.as_str()).collect();
        assert!(tail.ends_with('…'), "超出两行应截断加省略号: {tail}");
    }
}

#[cfg(test)]
mod more_tests {
    use super::super::{BASE_FONT_SIZE, Limits, parse, to_lines, to_plain_text};

    /// 测试基准排版限制(相当于旧版 367px 宽/5 行上限的口径)
    const LIMITS: Limits = Limits {
        line_units: 24.0,
        max_lines: 5,
    };

    /// 基础子集:加粗/颜色/字号/br/实体/未知标签剥除
    #[test]
    fn subset_parse_to_structured_lines() {
        let p = parse(
            "<b>紧急</b> 普通 <font color=\"#d93025\">红</font><br>第二行 <i>斜体剥除</i> &amp; 实体",
            LIMITS,
        );
        let lines = to_lines(&p);
        assert_eq!(lines.len(), 2, "br 应产生两行");
        assert_eq!(lines[0].size, BASE_FONT_SIZE);
        let bold = lines[0].runs.iter().find(|r| r.bold).unwrap();
        assert_eq!(bold.text, "紧急");
        let red = lines[0]
            .runs
            .iter()
            .find(|r| r.color == Some((0xd9, 0x30, 0x25)))
            .unwrap();
        assert_eq!(red.text, "红");
        assert!(
            !lines[1].runs.iter().any(|r| r.bold),
            "未知标签剥除不产生样式"
        );
        let second = lines[1]
            .runs
            .iter()
            .map(|r| r.text.as_str())
            .collect::<String>();
        assert!(second.contains("斜体剥除"), "剥除后内文保留");
        assert!(second.contains("& 实体"), "实体应解码: {second}");
    }

    /// 字号按行生效;超范围字号忽略
    #[test]
    fn font_size_per_line_and_clamp() {
        let p = parse(
            "<font size=\"17\">大字行</font><br><font size=\"99\">非法字号回落</font>",
            Limits::UNBOUNDED,
        );
        let lines = to_lines(&p);
        assert_eq!(lines[0].size, 17);
        assert_eq!(lines[1].size, BASE_FONT_SIZE, "超范围字号应回落默认");
    }

    /// 超过 `max_lines` 截断加省略号
    #[test]
    fn truncate_with_ellipsis() {
        let text = std::array::from_fn::<_, 8, _>(|_| "很长的一行内容呀".repeat(3)).join("<br>");
        let p = parse(&text, LIMITS);
        let lines = to_lines(&p);
        assert_eq!(lines.len(), LIMITS.max_lines);
        assert_eq!(lines.last().unwrap().runs.last().unwrap().text, "…");
    }

    /// 纯文本提取:标签全剥、行按 \n 连接
    #[test]
    fn plain_text_extraction() {
        let t = to_plain_text("<b>A</b><font color=\"red\">B</font><br>C &lt;D&gt;");
        assert_eq!(t, "AB\nC <D>");
    }
}
