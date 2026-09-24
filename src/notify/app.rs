//! iced 程序体:daemon 常驻(默认无窗口),通知到达后 `window::open` 弹窗。
//!
//! 跨线程投递经 bridge(HTTP/调度线程 → 订阅流);定时(入场动画/位置复校)
//! 由短生命周期 std 线程驱动——thread-pool 执行器后端不带 Timer,
//! 不为此引入 tokio/smol。窗口生命周期:关闭=销毁窗口,下一条通知重开
//! (iced 无窗口隐藏 API);单窗 latest-only 语义。

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::time::{Duration, Instant};

use iced::font::{Family, Weight};
use iced::widget::container::Style as ContainerStyle;
use iced::widget::{Column, container, markdown, mouse_area, row, text};
use iced::{Color, Element, Font, Length, Padding, Subscription, Task, Theme, daemon, window};

use crate::markdown_body;
use crate::notify::popup;

const CARD_BORDER: Color = Color::from_rgb8(0x1c, 0x27, 0x38);
/// 关闭钮 × 静止色:标题栏内保持可见但低于标题层级
const CLOSE_GLYPH: Color = Color::from_rgb8(0xc7, 0xd0, 0xdd);
const CLOSE_GLYPH_HOVER: Color = Color::WHITE;
const CLOSE_HOVER_BG: Color = Color::from_rgb8(0x3d, 0x4c, 0x62);
const HEADER_H: f32 = 38.0;
const ACK_TIMEOUT: Duration = Duration::from_secs(3);
const ACK_PENDING: u8 = 0;
const ACK_CANCELLED: u8 = 1;
const ACK_SHOWN: u8 = 2;
const ACK_FAILED: u8 = 3;

#[derive(Debug, Clone)]
pub struct PopupAck {
    sender: SyncSender<bool>,
    status: Arc<AtomicU8>,
}

pub struct AckWaiter {
    receiver: Receiver<bool>,
    status: Arc<AtomicU8>,
}

impl PopupAck {
    pub fn new() -> (Self, AckWaiter) {
        let (sender, receiver) = sync_channel(1);
        let status = Arc::new(AtomicU8::new(ACK_PENDING));
        (
            Self {
                sender,
                status: Arc::clone(&status),
            },
            AckWaiter { receiver, status },
        )
    }

    fn is_cancelled(&self) -> bool {
        self.status.load(Ordering::Acquire) == ACK_CANCELLED
    }

    fn complete(&self, shown: bool) -> bool {
        let completed = if shown { ACK_SHOWN } else { ACK_FAILED };
        if self
            .status
            .compare_exchange(ACK_PENDING, completed, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return false;
        }
        let _ = self.sender.try_send(shown);
        true
    }
}

impl AckWaiter {
    pub fn wait(self) -> bool {
        self.wait_for(ACK_TIMEOUT)
    }

    fn wait_for(self, timeout: Duration) -> bool {
        if let Ok(shown) = self.receiver.recv_timeout(timeout) {
            return shown;
        }
        matches!(
            self.status.compare_exchange(
                ACK_PENDING,
                ACK_CANCELLED,
                Ordering::AcqRel,
                Ordering::Acquire,
            ),
            Err(ACK_SHOWN)
        )
    }
}

/// 平台标准 UI 字体族:钉死族名让 CJK 与拉丁同族——iced 默认 SansSerif
/// 解析为 "Open Sans"(各平台普遍缺失),按脚本回退后拉丁落到 Helvetica 系,
/// 与中文 PingFang/Noto/YaHei 度量不合,行内基线与字面大小观感不一致
#[cfg(target_os = "macos")]
const UI_FONT_FAMILY: &str = "PingFang SC";
#[cfg(windows)]
const UI_FONT_FAMILY: &str = "Microsoft YaHei UI";
#[cfg(all(unix, not(target_os = "macos")))]
const UI_FONT_FAMILY: &str = "Noto Sans CJK SC";

const UI_FONT: Font = Font {
    family: Family::Name(UI_FONT_FAMILY),
    ..Font::DEFAULT
};

const BOLD: Font = Font {
    family: Family::Name(UI_FONT_FAMILY),
    weight: Weight::Bold,
    ..Font::DEFAULT
};

/// 程序消息(跨线程投递,需 Clone/Send)
#[derive(Debug, Clone)]
pub enum Message {
    /// 新通知到达;`quit_on_close` 仅 notify 子命令单发进程为 true
    Notify {
        title: String,
        body_markdown: String,
        quit_on_close: bool,
        /// 本条通知的弹窗尺寸(请求/默认解析后的生效值)
        size: popup::Size,
        colors: popup::Colors,
        ack: Option<PopupAck>,
    },
    /// 请求关闭弹窗(点击窗口任意处/关闭钮/HTTP /close/系统关闭请求)
    Close,
    /// 窗口创建完成(原生窗已映射)
    Opened(window::Id),
    /// 窗口已销毁
    Closed(window::Id),
    /// 位置复校 tick(`delay_ms` 标识档位)
    Fixup(u64),
    /// 复校读回的当前窗口位置
    FixupPosition(Option<iced::Point>),
    /// 入场动画 tick(距开始毫秒)
    Animate(u64),
    /// 关闭钮 hover 态
    CloseHover(bool),
    /// 链接只显示样式，不触发外部导航。
    LinkClicked,
}

/// 程序状态(全部在事件循环线程内访问)
struct State {
    title: String,
    body: Vec<markdown::Item>,
    window: Option<window::Id>,
    /// 弹窗关闭后退出事件循环(仅单发进程;服务模式恒 false)
    quit_on_close: bool,
    area: crate::screen::WorkArea,
    /// 当前弹窗尺寸(随每条通知更新,复用窗口时据此 resize)
    size: popup::Size,
    colors: popup::Colors,
    /// 当前滑入剩余偏移(px),0 表示就位
    slide: f32,
    hover_close: bool,
    pending_ack: Option<PopupAck>,
}

impl State {
    const fn new() -> Self {
        Self {
            title: String::new(),
            body: Vec::new(),
            window: None,
            quit_on_close: false,
            area: crate::screen::WorkArea {
                x: 0.0,
                y: 0.0,
                w: 0.0,
                h: 0.0,
                scale: 1.0,
            },
            size: popup::Size::DEFAULT,
            colors: popup::Colors::DEFAULT,
            slide: popup::SLIDE_PX,
            hover_close: false,
            pending_ack: None,
        }
    }
}

/// 服务模式:daemon 常驻,窗口关完不退出(进程由 stop/kill 终止)
pub fn run_service() -> iced::Result {
    build_daemon(|| (State::new(), Task::none()))
}

/// 单发模式(notify 子命令):boot 即注入一条通知,弹窗关闭后退出
pub fn run_single(
    title: String,
    body_markdown: String,
    size: popup::Size,
    colors: popup::Colors,
) -> iced::Result {
    build_daemon(move || {
        (
            State::new(),
            Task::done(Message::Notify {
                title: title.clone(),
                body_markdown: body_markdown.clone(),
                quit_on_close: true,
                size,
                colors,
                ack: None,
            }),
        )
    })
}

fn build_daemon(boot: impl Fn() -> (State, Task<Message>) + 'static) -> iced::Result {
    daemon(boot, update, view)
        .title(popup::WINDOW_TITLE)
        .subscription(subscription)
        .theme(Theme::Light)
        .default_font(UI_FONT)
        .style(|_state, _theme| iced::theme::Style {
            background_color: Color::WHITE,
            text_color: color(popup::Colors::DEFAULT.header_text),
        })
        .run()
}

/// 跨线程投递一条消息;事件循环未就绪/已退出时返回 false(调用方降级)
pub fn post(message: Message) -> bool {
    bridge::post(message)
}

fn update(state: &mut State, message: Message) -> Task<Message> {
    match message {
        Message::Notify {
            title,
            body_markdown,
            quit_on_close,
            size,
            colors,
            ack,
        } => notify(
            state,
            title,
            &body_markdown,
            quit_on_close,
            size,
            colors,
            ack,
        ),
        Message::Opened(id) => {
            if state.window != Some(id) {
                return Task::none();
            }
            if let Some(ack) = state.pending_ack.take()
                && !ack.complete(true)
            {
                return window::close::<Message>(id);
            }
            // 原生窗口已映射:补设 X11 属性(窗口类型/状态/图标);
            // 置顶再走一次 ClientMessage(映射前发送会被 WM 丢弃,EWMH 语义)
            #[cfg(target_os = "linux")]
            crate::notify::window_icon::set();
            schedule_animation();
            schedule_fixups();
            if let Some(id) = state.window {
                window::set_level(id, window::Level::AlwaysOnTop)
            } else {
                Task::none()
            }
        }
        Message::Close => {
            let Some(id) = state.window else {
                return Task::none();
            };
            if let Some(ack) = state.pending_ack.take() {
                ack.complete(false);
            }
            log::debug!("弹窗被关闭");
            window::close::<Message>(id)
        }
        Message::Closed(id) => {
            if state.window == Some(id) {
                state.window = None;
                if let Some(ack) = state.pending_ack.take() {
                    ack.complete(false);
                }
                // 下次开窗重新滑入
                state.slide = popup::SLIDE_PX;
                // 窗口在悬停状态下被点击销毁时不会再收到 on_exit；
                // 清掉瞬时交互态，避免下次开窗沿用悬停背景。
                state.hover_close = false;
                if state.quit_on_close {
                    return iced::exit();
                }
            }
            Task::none()
        }
        Message::Fixup(delay_ms) => {
            let Some(id) = state.window else {
                return Task::none();
            };
            if popup::should_retry_x11_init(delay_ms) {
                // 首 tick 兜底:窗口新开时原生窗可能尚未进 _NET_CLIENT_LIST
                #[cfg(target_os = "linux")]
                crate::notify::window_icon::set();
            }
            window::position(id).map(Message::FixupPosition)
        }
        Message::FixupPosition(current) => {
            let expected = popup::logical_landing(&state.area, state.size);
            if let Some(pos) = current {
                let drifted = (pos.x - expected.x).abs() > 2.0 || (pos.y - expected.y).abs() > 2.0;
                if drifted {
                    log::warn!("WM 重摆了弹窗(现 {pos:?}),复校回 {expected:?}");
                }
            }
            // 无条件复校(幂等 move_to,不依赖 WM 是否已摆正)
            if let Some(id) = state.window {
                window::move_to(id, expected)
            } else {
                Task::none()
            }
        }
        Message::Animate(elapsed_ms) => {
            // u64→f32:毫秒时长的失真远小于一帧
            #[allow(clippy::cast_precision_loss)]
            let t = (elapsed_ms as f32 / popup::SLIDE_MS as f32).min(1.0);
            state.slide = popup::SLIDE_PX * (1.0 - popup::ease_out_cubic(t));
            Task::none()
        }
        Message::CloseHover(on) => {
            state.hover_close = on;
            Task::none()
        }
        Message::LinkClicked => Task::none(),
    }
}

/// 新通知:更新内容与尺寸;窗口在则复用(重摆尺寸/位置/置顶),不在则创建期定位开窗
fn notify(
    state: &mut State,
    title: String,
    body_markdown: &str,
    quit_on_close: bool,
    size: popup::Size,
    colors: popup::Colors,
    ack: Option<PopupAck>,
) -> Task<Message> {
    if ack.as_ref().is_some_and(PopupAck::is_cancelled) {
        return Task::none();
    }
    let Some(area) = crate::screen::work_area() else {
        log::warn!("无法获取屏幕工作区,本条通知走系统通知");
        if let Some(ack) = ack {
            ack.complete(false);
        } else {
            crate::notify::fallback::show_raw(&title, &markdown_body::to_plain_text(body_markdown));
        }
        return Task::none();
    };
    state.area = area;
    state.quit_on_close = quit_on_close;
    state.title = title;
    state.size = size;
    state.colors = colors;
    let sanitized = markdown_body::sanitize_for_text_view(body_markdown);
    state.body = markdown::parse(&sanitized).collect();
    let (px, py) = popup::landing(&area, size);
    log::info!(
        "弹窗定位: 工作区({},{},{}x{}) → ({px},{py}),尺寸 {}x{}",
        area.x,
        area.y,
        area.w,
        area.h,
        size.width,
        size.height
    );
    if let Some(id) = state.window {
        match (state.pending_ack.take(), ack) {
            (Some(previous), next) => {
                previous.complete(false);
                state.pending_ack = next;
            }
            (None, Some(ack)) => {
                ack.complete(true);
            }
            (None, None) => {}
        }
        // 窗口复用:内容与尺寸已更新,重跑一轮 resize/位置复校/置顶(resize 幂等)
        schedule_fixups();
        // f64→f32:窗口逻辑尺寸为整数级数值,无精度损失
        #[allow(clippy::cast_possible_truncation)]
        let logical = iced::Size::new(size.width as f32, size.height as f32);
        Task::batch([
            window::resize(id, logical),
            window::move_to(id, popup::logical_landing(&state.area, size)),
            window::set_level(id, window::Level::AlwaysOnTop),
        ])
    } else {
        let (id, opened) = window::open(popup::window_settings(&area, size));
        state.window = Some(id);
        state.pending_ack = ack;
        opened.map(Message::Opened)
    }
}

fn subscription(_state: &State) -> Subscription<Message> {
    Subscription::batch([
        window::close_events().map(Message::Closed),
        // 无框窗口的关闭请求(如 WM 快捷键)与点击关闭同路
        window::close_requests().map(|_| Message::Close),
        bridge::subscription(),
    ])
}

fn view(state: &State, _window: window::Id) -> Element<'_, Message> {
    // 深色标题栏提供稳定轮廓:白色网页全屏时不再只靠一圈细边辨认通知。
    // 滑入仍只作用于内容，窗口与外框保持落定不动。
    let header = container(title_row(state))
        .height(HEADER_H)
        .width(Length::Fill)
        .style(move |_theme| header_style(color(state.colors.header_background)));
    let body = container(body_content(state))
        .padding(Padding {
            top: 8.0,
            bottom: 8.0,
            left: popup::PAD_LEFT + state.slide,
            right: popup::PAD_RIGHT,
        })
        .width(Length::Fill)
        .height(Length::Fill)
        .clip(true)
        .style(move |_theme| {
            let mut style = body_panel_style(color(state.colors.body_background));
            style.text_color = Some(color(state.colors.body_text));
            style
        });
    let card = container(Column::with_capacity(2).push(header).push(body))
        .width(Length::Fill)
        .height(Length::Fill)
        .style(|_theme| card_style());

    // 整窗点击关闭(与关闭钮同为 Close,重复消息幂等)
    mouse_area(card).on_press(Message::Close).into()
}

/// 深色外框把双色卡片收成一个整体
fn card_style() -> ContainerStyle {
    ContainerStyle {
        border: iced::Border {
            color: CARD_BORDER,
            width: 1.0,
            ..iced::Border::default()
        },
        ..ContainerStyle::default()
    }
}

fn header_style(background: Color) -> ContainerStyle {
    ContainerStyle {
        background: Some(background.into()),
        ..ContainerStyle::default()
    }
}

fn body_panel_style(background: Color) -> ContainerStyle {
    ContainerStyle {
        background: Some(background.into()),
        ..ContainerStyle::default()
    }
}

/// 标题行:加粗标题(单行截断)+ 关闭钮,垂直居中
fn title_row(state: &State) -> Element<'_, Message> {
    row![
        container(
            text(popup::elide_title(&state.title, state.size.width))
                .size(16.0)
                .font(BOLD)
                .color(color(state.colors.header_text))
                .wrapping(iced::widget::text::Wrapping::None)
                .width(Length::Fill),
        )
        .padding(Padding {
            top: 0.0,
            bottom: 0.0,
            left: popup::PAD_LEFT + state.slide,
            right: popup::PAD_RIGHT,
        })
        .height(Length::Fill)
        .align_y(iced::alignment::Vertical::Center)
        .width(Length::Fill),
        close_button(state.hover_close),
    ]
    .height(Length::Fill)
    .align_y(iced::Alignment::Center)
    .into()
}

/// 关闭钮占满标题栏右端方格:与上/右外框贴齐,静止时不显示格子边界
fn close_button(hover: bool) -> Element<'static, Message> {
    let glyph = if hover {
        CLOSE_GLYPH_HOVER
    } else {
        CLOSE_GLYPH
    };
    let glyph = text("×")
        .size(16.0)
        .line_height(1.0)
        .font(UI_FONT)
        .color(glyph)
        .width(Length::Fill)
        .height(Length::Fill)
        .center();
    let circle = container(glyph)
        // 字体的 × 字面重心略低；底部多留 2px，使视觉中心上移 1px。
        // 点击区域仍保持完整的 38×38。
        .padding(Padding {
            top: 0.0,
            bottom: 2.0,
            left: 0.0,
            right: 0.0,
        })
        .width(HEADER_H)
        .height(HEADER_H)
        // iced Container 默认 Left/Top 对齐,必须显式居中
        .align_x(iced::alignment::Horizontal::Center)
        .align_y(iced::alignment::Vertical::Center)
        .style(move |_theme| ContainerStyle {
            background: hover.then_some(CLOSE_HOVER_BG.into()),
            ..ContainerStyle::default()
        });

    mouse_area(circle)
        .on_press(Message::Close)
        .on_enter(Message::CloseHover(true))
        .on_exit(Message::CloseHover(false))
        .into()
}

/// 正文交给 iced 的 Markdown 渲染器；图片区块只显示替代文本，链接不导航。
fn body_content(state: &State) -> Element<'_, Message> {
    let body_text = color(state.colors.body_text);
    let mut style = markdown::Style::from_palette(Theme::Light.palette());
    style.font = UI_FONT;
    style.link_color = body_text;
    style.inline_code_color = body_text;
    style.inline_code_font = UI_FONT;
    style.code_block_font = UI_FONT;
    style.inline_code_highlight = markdown::Highlight {
        background: Color {
            a: 0.08,
            ..body_text
        }
        .into(),
        border: iced::Border::default(),
    };
    let mut settings = markdown::Settings::with_text_size(14, style);
    settings.h1_size = 14.into();
    settings.h2_size = 14.into();
    settings.h3_size = 14.into();
    settings.h4_size = 14.into();
    settings.h5_size = 14.into();
    settings.h6_size = 14.into();
    settings.code_size = 14.into();
    settings.spacing = iced::Pixels(0.0);
    markdown::view(&state.body, settings).map(|_| Message::LinkClicked)
}

const fn color([red, green, blue]: [u8; 3]) -> Color {
    Color::from_rgb8(red, green, blue)
}

/// 延时投递一条消息(thread-pool 后端无 Timer;短生命周期线程驱动)
fn post_after(delay: Duration, make_message: impl FnOnce() -> Message + Send + 'static) {
    std::thread::spawn(move || {
        std::thread::sleep(delay);
        let _ = bridge::post(make_message());
    });
}

/// 显示后多档位置复校,对抗 WM 重摆
fn schedule_fixups() {
    for delay_ms in popup::FIXUP_DELAYS_MS {
        post_after(Duration::from_millis(delay_ms), move || {
            Message::Fixup(delay_ms)
        });
    }
}

/// 入场动画:~60Hz 驱动 220ms,收尾补一帧确保终态精确就位
fn schedule_animation() {
    std::thread::spawn(|| {
        let start = Instant::now();
        let total = Duration::from_millis(popup::SLIDE_MS);
        while start.elapsed() < total {
            std::thread::sleep(Duration::from_millis(16));
            let elapsed = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
            let _ = bridge::post(Message::Animate(elapsed));
        }
        let _ = bridge::post(Message::Animate(popup::SLIDE_MS));
    });
}

/// 跨线程桥:静态通道发件端,订阅流常驻消费
mod bridge {
    use std::sync::Mutex;

    use iced::Subscription;
    use iced::futures::Stream;
    use iced::futures::channel::mpsc::{UnboundedSender, unbounded};

    use super::Message;

    static SENDER: Mutex<Option<UnboundedSender<Message>>> = Mutex::new(None);

    fn locked() -> std::sync::MutexGuard<'static, Option<UnboundedSender<Message>>> {
        SENDER
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// 事件循环未就绪/已退出时返回 false(调用方降级)
    pub(super) fn post(message: Message) -> bool {
        match &*locked() {
            Some(sender) => sender.unbounded_send(message).is_ok(),
            None => false,
        }
    }

    pub(super) fn subscription() -> Subscription<Message> {
        Subscription::run(stream)
    }

    fn stream() -> impl Stream<Item = Message> {
        let (sender, receiver) = unbounded();
        *locked() = Some(sender);
        receiver
    }
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::{Message, PopupAck, State, body_panel_style, color, header_style, update};
    use crate::notify::popup::Colors;
    use std::time::Duration;

    #[test]
    fn popup_ack_confirms_only_opened_window() {
        let (ack, waiter) = PopupAck::new();
        assert!(ack.complete(true));
        assert!(waiter.wait());
        assert!(!ack.complete(false));
    }

    #[test]
    fn popup_ack_timeout_cancels_late_open() {
        let (ack, waiter) = PopupAck::new();
        assert!(!waiter.wait_for(Duration::ZERO));
        assert!(ack.is_cancelled());
        assert!(!ack.complete(true));
    }

    #[test]
    fn iced_markdown_keeps_bold_and_paragraph_break() {
        use iced::font::Weight;
        use iced::widget::markdown::{self, Item};

        let source = crate::markdown_body::sanitize_for_text_view("待办通知 **1** 条\n\n15:21:05");
        let items: Vec<_> = markdown::parse(&source).collect();
        assert_eq!(items.len(), 2);
        let Item::Paragraph(first) = &items[0] else {
            panic!("第一块应为正文");
        };
        let style = markdown::Style::from_palette(iced::Theme::Light.palette());
        assert!(first.spans(style).iter().any(|span| {
            span.text.as_ref() == "1" && span.font.is_some_and(|font| font.weight == Weight::Bold)
        }));
        let Item::Paragraph(second) = &items[1] else {
            panic!("第二块应为时间");
        };
        assert_eq!(second.spans(style)[0].text.as_ref(), "15:21:05");
    }

    #[test]
    fn iced_markdown_displays_raw_html_as_text() {
        use iced::widget::markdown::{self, Item};

        let source = crate::markdown_body::sanitize_for_text_view("待办 <b>1</b> 条");
        let items: Vec<_> = markdown::parse(&source).collect();
        let Item::Paragraph(body) = &items[0] else {
            panic!("正文应为段落");
        };
        let style = markdown::Style::from_palette(iced::Theme::Light.palette());
        let text = body
            .spans(style)
            .iter()
            .map(|span| span.text.as_ref())
            .collect::<String>();
        assert_eq!(text, "待办 <b>1</b> 条");
    }

    #[test]
    fn notification_sections_keep_strong_visual_contrast() {
        let colors = Colors::DEFAULT;
        assert_eq!(
            header_style(color(colors.header_background)).background,
            Some(color(colors.header_background).into())
        );
        assert_eq!(
            body_panel_style(color(colors.body_background)).background,
            Some(color(colors.body_background).into())
        );
        assert_ne!(colors.header_background, colors.body_background);
    }

    #[test]
    fn closing_window_clears_close_hover_state() {
        let mut state = State::new();
        let id = iced::window::Id::unique();
        state.window = Some(id);

        let _ = update(&mut state, Message::CloseHover(true));
        assert!(state.hover_close);
        let _ = update(&mut state, Message::Closed(id));

        assert!(!state.hover_close);
    }
}
