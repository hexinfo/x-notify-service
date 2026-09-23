use std::borrow::Cow;

use markdown::mdast::Node;

/// Escape raw HTML nodes so Markdown TextView displays their source literally.
pub fn sanitize_for_text_view(source: &str) -> Cow<'_, str> {
    let Ok(root) = markdown::to_mdast(source, &markdown::ParseOptions::gfm()) else {
        return Cow::Borrowed(source);
    };
    let mut ranges = Vec::new();
    collect_html_ranges(&root, &mut ranges);
    if ranges.is_empty() {
        return Cow::Borrowed(source);
    }

    // mdast positions use source byte offsets; reverse order keeps earlier ranges stable.
    ranges.sort_unstable_by_key(|(start, _)| *start);
    let mut sanitized = source.to_owned();
    for (start, end) in ranges.into_iter().rev() {
        if let Some(raw) = source.get(start..end) {
            let escaped = raw
                .replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;");
            sanitized.replace_range(start..end, &escaped);
        }
    }
    Cow::Owned(sanitized)
}

fn collect_html_ranges(node: &Node, ranges: &mut Vec<(usize, usize)>) {
    if let Node::Html(html) = node
        && let Some(position) = &html.position
    {
        ranges.push((position.start.offset, position.end.offset));
    }
    if let Some(children) = node.children() {
        for child in children {
            collect_html_ranges(child, ranges);
        }
    }
}

/// Project Markdown into text for the system notification fallback.
pub fn to_plain_text(source: &str) -> String {
    let Ok(root) = markdown::to_mdast(source, &markdown::ParseOptions::gfm()) else {
        return source.to_owned();
    };
    let mut out = String::new();
    collect_text(&root, &mut out);
    out.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Escape a plain body before handing it to a notification server that parses markup.
fn escape_notification_markup(plain_text: &str) -> String {
    plain_text
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Escape only when the notification server advertises body markup support.
pub fn notification_body_for_capabilities(plain_text: &str, supports_markup: bool) -> Cow<'_, str> {
    if supports_markup {
        Cow::Owned(escape_notification_markup(plain_text))
    } else {
        Cow::Borrowed(plain_text)
    }
}

fn collect_text(node: &Node, out: &mut String) {
    if let Some(children) = node.children() {
        for child in children {
            collect_text(child, out);
        }
    }
    if let Node::Text(value) = node {
        out.push_str(&value.value);
    }
    if let Node::Html(value) = node {
        out.push_str(&value.value);
    }
    if let Node::Code(value) = node {
        out.push_str(&value.value);
        out.push('\n');
    }
    if let Node::InlineCode(value) = node {
        out.push_str(&value.value);
    }
    if let Node::Image(value) = node {
        out.push_str(&value.alt);
    }
    if let Node::ImageReference(value) = node {
        out.push_str(&value.alt);
    }
    if matches!(
        node,
        Node::Break(_) | Node::Paragraph(_) | Node::Heading(_) | Node::TableRow(_)
    ) {
        out.push('\n');
    }
    if matches!(node, Node::TableCell(_)) {
        out.push(' ');
    }
}

#[cfg(test)]
mod tests {
    use super::{notification_body_for_capabilities, sanitize_for_text_view, to_plain_text};

    #[test]
    fn raw_html_is_literal_markdown_input() {
        assert_eq!(
            sanitize_for_text_view("待办 <b>1</b> 条"),
            "待办 &lt;b&gt;1&lt;/b&gt; 条"
        );
    }

    #[test]
    fn raw_html_entity_keeps_its_original_spelling_in_text_view() {
        assert_eq!(
            sanitize_for_text_view("<b title=\"&amp;\">1</b>"),
            "&lt;b title=\"&amp;amp;\"&gt;1&lt;/b&gt;"
        );
    }

    #[test]
    fn block_quote_marker_is_not_escaped() {
        assert_eq!(sanitize_for_text_view("> 引用"), "> 引用");
    }

    #[test]
    fn plain_text_drops_syntax_and_link_targets() {
        assert_eq!(
            to_plain_text("**紧急** [工单](https://example.invalid/1)\n\n- 第一项\n- `code`"),
            "紧急 工单\n第一项\ncode"
        );
    }

    #[test]
    fn image_uses_alt_text_without_url() {
        assert_eq!(
            to_plain_text("![流程图](https://example.invalid/a.png)"),
            "流程图"
        );
    }

    #[test]
    fn raw_html_remains_visible_in_plain_text() {
        assert_eq!(to_plain_text("待办 <b>1</b> 条"), "待办 <b>1</b> 条");
    }

    #[test]
    fn markup_capable_server_receives_escaped_literal_html() {
        let plain = to_plain_text("**待办** <b>1</b> &amp;");
        assert_eq!(
            notification_body_for_capabilities(&plain, true),
            "待办 &lt;b&gt;1&lt;/b&gt; &amp;"
        );
    }

    #[test]
    fn server_without_markup_receives_literal_plain_text() {
        assert_eq!(
            notification_body_for_capabilities("待办 <b>1</b> &", false),
            "待办 <b>1</b> &"
        );
    }
}
