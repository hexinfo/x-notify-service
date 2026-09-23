//! GPUI 通知卡片。窗口生命周期由 `app` 持有，视图只发出关闭请求。

#[cfg(test)]
mod tests {
    use super::{CloseRequested, PopupPayload, PopupView, image_alt_node, popup_border_color};
    use crate::notify::popup::{Colors, Size};
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{AppContext as _, TestAppContext, point, px, rgb, size};
    use std::{cell::Cell, rc::Rc};

    fn payload(body_markdown: &str) -> PopupPayload {
        PopupPayload {
            title: "消息提醒".into(),
            body_markdown: body_markdown.into(),
            size: Size::DEFAULT,
            colors: Colors::DEFAULT,
            quit_on_close: false,
        }
    }

    #[test]
    fn popup_border_is_opaque_black() {
        assert_eq!(popup_border_color(), rgb(0).into());
    }

    #[gpui_kit::test]
    fn popup_layout_keeps_request_size_and_close_cell_geometry(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let handle = cx.open_window(size(px(220.), px(100.)), |_, _| {
            PopupView::new(payload("待办 **1** 条"))
        });
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(
                window.find("popup-root").bounds().size,
                size(px(220.), px(100.))
            );
            assert_eq!(window.find("popup-header").bounds().size.height, px(38.));
            assert_eq!(
                window.find("popup-close").bounds().size,
                size(px(38.), px(38.))
            );
            assert_eq!(window.find("popup-body").bounds().size.height, px(60.));
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn reset_interaction_clears_close_hover(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        cx.update(|cx| cx.set_reduce_motion(true));
        let handle = cx.open_window(size(px(220.), px(100.)), |_, _| {
            PopupView::new(payload("正文"))
        });
        cx.update_window(handle.into(), |_, window, cx| {
            window.hover("popup-close", cx);
        })
        .unwrap();
        let view = cx.read_window(&handle, |view, _| view).unwrap();
        cx.update_entity(&view, |view, cx| {
            assert!(view.hover_close);
            view.reset_interaction(cx);
            assert!(!view.hover_close);
        });
    }

    #[gpui_kit::test]
    fn markdown_link_click_does_not_open_url(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        cx.update(|cx| cx.set_reduce_motion(true));
        let handle = cx.open_window(size(px(220.), px(100.)), |_, _| {
            PopupView::new(payload("[链接](https://example.invalid/a)"))
        });
        let view = cx.read_window(&handle, |view, _| view).unwrap();
        let closes = Rc::new(Cell::new(0));
        let observed = Rc::clone(&closes);
        cx.update(|cx| {
            cx.subscribe(&view, move |_, _: &CloseRequested, _| {
                observed.set(observed.get() + 1);
            })
            .detach();
        });
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            window.click_at("popup-body", point(px(28.), px(18.)), cx);
        })
        .unwrap();
        assert_eq!(cx.opened_url(), None);
        assert_eq!(closes.get(), 1);
    }

    #[gpui_kit::test]
    fn close_cell_click_emits_one_request(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let handle = cx.open_window(size(px(220.), px(100.)), |_, _| {
            PopupView::new(payload("正文"))
        });
        let view = cx.read_window(&handle, |view, _| view).unwrap();
        let closes = Rc::new(Cell::new(0));
        let observed = Rc::clone(&closes);
        cx.update(|cx| {
            cx.subscribe(&view, move |_, _: &CloseRequested, _| {
                observed.set(observed.get() + 1);
            })
            .detach();
        });
        cx.update_window(handle.into(), |_, window, cx| {
            window.click("popup-close", cx);
        })
        .unwrap();
        assert_eq!(closes.get(), 1);
    }

    #[test]
    fn markdown_image_becomes_alt_text_without_resource_url() {
        let root = markdown::to_mdast(
            "![图](https://example.invalid/a.png)",
            &markdown::ParseOptions::gfm(),
        )
        .unwrap();
        let image = root.children().unwrap()[0].children().unwrap()[0].clone();
        let node = image_alt_node(&image).unwrap();
        assert_eq!(node.as_text(), "图");
        assert!(!node.as_markdown().contains("example.invalid"));
    }
}
use std::time::Duration;

use gpui_kit::base::TestSupportExt as _;
use gpui_kit::base::text::{
    MarkdownNode, MarkdownParseContext, MarkdownPlugin, TextView, TextViewStyle,
};
use gpui_kit::{
    Animation, AnimationExt as _, Context, EventEmitter, FontWeight, HighlightStyle, Hsla,
    IntoElement, PathBuilder, Render, Window, canvas, div, point, prelude::*, px, rems, rgb,
};
use markdown::mdast::Node;

use crate::markdown_body::sanitize_for_text_view;
use crate::notify::popup::{self, Colors, Size};

const HEADER_HEIGHT: f32 = 38.0;
const CLOSE_SIZE: f32 = 38.0;

#[derive(Clone)]
pub struct PopupPayload {
    pub title: String,
    pub body_markdown: String,
    pub size: Size,
    pub colors: Colors,
    pub quit_on_close: bool,
}

/// Clicking anywhere on the popup asks its controller to close the window.
pub struct CloseRequested;

#[allow(clippy::module_name_repetitions)]
pub struct PopupView {
    payload: PopupPayload,
    hover_close: bool,
}

impl EventEmitter<CloseRequested> for PopupView {}

impl PopupView {
    pub const fn new(payload: PopupPayload) -> Self {
        Self {
            payload,
            hover_close: false,
        }
    }

    pub fn set_payload(&mut self, payload: PopupPayload, cx: &mut Context<Self>) {
        self.payload = payload;
        self.reset_interaction(cx);
    }

    pub fn reset_interaction(&mut self, cx: &mut Context<Self>) {
        self.hover_close = false;
        cx.notify();
    }

    pub const fn payload(&self) -> &PopupPayload {
        &self.payload
    }
}

fn color(channels: [u8; 3]) -> Hsla {
    rgb((u32::from(channels[0]) << 16) | (u32::from(channels[1]) << 8) | u32::from(channels[2]))
        .into()
}

fn markdown_style(colors: Colors) -> TextViewStyle {
    let foreground = color(colors.body_text);
    let code_background = color(colors.body_background).blend(foreground.opacity(0.08));
    TextViewStyle::default()
        .with_foreground(foreground)
        .with_muted_foreground(foreground.opacity(0.75))
        .with_link(foreground)
        .with_code_background(code_background)
        .with_border(foreground.opacity(0.25))
        .with_inline_code(HighlightStyle {
            color: Some(foreground),
            background_color: Some(code_background),
            ..Default::default()
        })
        .with_paragraph_gap(rems(0.))
        .with_heading_base_font_size(px(14.))
        .with_heading_font_size(|_, _| px(14.))
}

fn image_alt_node(node: &Node) -> Option<MarkdownNode> {
    let Node::Image(image) = node else {
        return None;
    };
    Some(
        MarkdownNode::new("popup-image-alt", ())
            .text(image.alt.clone())
            .markdown(image.alt.clone()),
    )
}

struct AltTextImages;

impl MarkdownPlugin for AltTextImages {
    fn name(&self) -> &'static str {
        "popup-image-alt"
    }

    fn parse(&self, node: &Node, _: &MarkdownParseContext<'_>) -> Option<MarkdownNode> {
        image_alt_node(node)
    }
}

fn close_mark(color: Hsla) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds, (), window, _| {
            let x = bounds.origin.x;
            let y = bounds.origin.y;
            let mut first = PathBuilder::stroke(px(1.5));
            first.move_to(point(x + px(3.), y + px(3.)));
            first.line_to(point(x + px(13.), y + px(13.)));
            if let Ok(path) = first.build() {
                window.paint_path(path, color);
            }
            let mut second = PathBuilder::stroke(px(1.5));
            second.move_to(point(x + px(13.), y + px(3.)));
            second.line_to(point(x + px(3.), y + px(13.)));
            if let Ok(path) = second.build() {
                window.paint_path(path, color);
            }
        },
    )
    .size(px(16.))
}

#[cfg(target_os = "macos")]
const UI_FONT: &str = "PingFang SC";
#[cfg(windows)]
const UI_FONT: &str = "Microsoft YaHei UI";
#[cfg(target_os = "linux")]
const UI_FONT: &str = "Noto Sans CJK SC";

fn popup_border_color() -> Hsla {
    rgb(0).into()
}

impl Render for PopupView {
    #[allow(clippy::too_many_lines, clippy::cast_possible_truncation)]
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = self.payload.colors;
        let header_text = color(colors.header_text);
        let body_text = color(colors.body_text);
        let markdown = sanitize_for_text_view(&self.payload.body_markdown).into_owned();
        let view = cx.weak_entity();
        let body = TextView::markdown("popup-markdown", markdown)
            .selectable(false)
            .scrollable(false)
            .max_lines(popup::body_limits(self.payload.size).max_lines)
            .style(markdown_style(colors))
            .on_link_click(move |_, _, _, cx| {
                if let Some(view) = view.upgrade() {
                    view.update(cx, |_, cx| cx.emit(CloseRequested));
                }
            })
            .plugin(AltTextImages);

        let content = div()
            .w_full()
            .h_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .id("popup-header")
                    .test_support()
                    .w_full()
                    .h(px(HEADER_HEIGHT))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .bg(color(colors.header_background))
                    .text_color(header_text)
                    .child(
                        div()
                            .id("popup-title")
                            .test_support()
                            .relative()
                            .min_w_0()
                            .flex_1()
                            .pl(px(14.))
                            .text_size(px(15.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .font_family(UI_FONT)
                            .truncate()
                            .child(self.payload.title.clone())
                            .with_animation(
                                "popup-title-slide",
                                Animation::new(Duration::from_millis(popup::SLIDE_MS))
                                    .with_easing(popup::ease_out_cubic),
                                |element, progress| {
                                    element.left(px(popup::SLIDE_PX * (1. - progress)))
                                },
                            ),
                    )
                    .child(
                        div()
                            .id("popup-close")
                            .test_support()
                            .w(px(CLOSE_SIZE))
                            .h(px(CLOSE_SIZE))
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .when(self.hover_close, |this| this.bg(header_text.opacity(0.12)))
                            .on_hover(cx.listener(|view, hovering, _, cx| {
                                view.hover_close = *hovering;
                                cx.notify();
                            }))
                            .child(close_mark(header_text)),
                    ),
            )
            .child(
                div()
                    .id("popup-body")
                    .test_support()
                    .w_full()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .bg(color(colors.body_background))
                    .text_color(body_text)
                    .text_size(px(14.))
                    .line_height(gpui_kit::relative(1.45))
                    .font_family(UI_FONT)
                    .px(px(14.))
                    .py(px(8.))
                    .child(
                        div().relative().w_full().child(body).with_animation(
                            "popup-body-slide",
                            Animation::new(Duration::from_millis(popup::SLIDE_MS))
                                .with_easing(popup::ease_out_cubic),
                            |element, progress| element.left(px(popup::SLIDE_PX * (1. - progress))),
                        ),
                    ),
            );

        div()
            .id("popup-root")
            .test_support()
            .w(px(self.payload.size.width as f32))
            .h(px(self.payload.size.height as f32))
            .overflow_hidden()
            .border_1()
            .border_color(popup_border_color())
            .on_click(cx.listener(|_, _, _, cx| cx.emit(CloseRequested)))
            .child(content)
    }
}
