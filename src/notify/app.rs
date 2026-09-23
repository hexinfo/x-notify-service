//! iced 程序体:daemon 常驻(默认无窗口),通知到达后 `window::open` 弹窗。
//!
//! 跨线程投递经 bridge(HTTP/调度线程 → 订阅流);定时(入场动画/位置复校)
//! 由短生命周期 std 线程驱动——thread-pool 执行器后端不带 Timer,
//! 不为此引入 tokio/smol。窗口生命周期:关闭=销毁窗口,下一条通知重开
//! (iced 无窗口隐藏 API);单窗 latest-only 语义。

use std::time::{Duration, Instant};

use iced::font::{Family, Weight};
use iced::widget::container::Style as ContainerStyle;
use iced::widget::text::Span;
use iced::widget::{Column, container, mouse_area, rich_text, row, span, text};
use iced::{Color, Element, Font, Length, Padding, Subscription, Task, Theme, daemon, window};

use crate::html;
use crate::notify::popup;

const TITLE_COLOR: Color = Color::from_rgb8(0x1f, 0x23, 0x29);
const BODY_COLOR: Color = Color::from_rgb8(0x5f, 0x66, 0x72);
const CLOSE_GLYPH: Color = Color::from_rgb8(0x9a, 0xa2, 0xad);
const CLOSE_HOVER_BG: Color = Color::from_rgb8(0xee, 0xf0, 0xf3);
/// 白底方角卡片黑色描边:紧凑尺寸下靠深色边界与桌面分离
const CARD_BORDER: Color = Color::BLACK;

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
        body_html: String,
        quit_on_close: bool,
        /// 本条通知的弹窗尺寸(请求/配置/默认解析后的生效值)
        size: popup::Size,
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
}

/// 程序状态(全部在事件循环线程内访问)
struct State {
    title: String,
    body: Vec<html::Line>,
    window: Option<window::Id>,
    /// 弹窗关闭后退出事件循环(仅单发进程;服务模式恒 false)
    quit_on_close: bool,
    area: crate::screen::WorkArea,
    /// 当前弹窗尺寸(随每条通知更新,复用窗口时据此 resize)
    size: popup::Size,
    /// 当前滑入剩余偏移(px),0 表示就位
    slide: f32,
    hover_close: bool,
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
            slide: popup::SLIDE_PX,
            hover_close: false,
        }
    }
}

/// 服务模式:daemon 常驻,窗口关完不退出(进程由 stop/kill 终止)
pub fn run_service() -> iced::Result {
    build_daemon(|| (State::new(), Task::none()))
}

/// 单发模式(notify 子命令):boot 即注入一条通知,弹窗关闭后退出
pub fn run_single(title: String, body_html: String, size: popup::Size) -> iced::Result {
    build_daemon(move || {
        (
            State::new(),
            Task::done(Message::Notify {
                title: title.clone(),
                body_html: body_html.clone(),
                quit_on_close: true,
                size,
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
            text_color: TITLE_COLOR,
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
            body_html,
            quit_on_close,
            size,
        } => notify(state, title, &body_html, quit_on_close, size),
        Message::Opened(_id) => {
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
            log::debug!("弹窗被关闭");
            window::close::<Message>(id)
        }
        Message::Closed(id) => {
            if state.window == Some(id) {
                state.window = None;
                // 下次开窗重新滑入
                state.slide = popup::SLIDE_PX;
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
    }
}

/// 新通知:更新内容与尺寸;窗口在则复用(重摆尺寸/位置/置顶),不在则创建期定位开窗
fn notify(
    state: &mut State,
    title: String,
    body_html: &str,
    quit_on_close: bool,
    size: popup::Size,
) -> Task<Message> {
    let Some(area) = crate::screen::work_area() else {
        log::warn!("无法获取屏幕工作区,本条通知走系统通知");
        crate::notify::fallback::show_raw(&title, &html::to_plain_text(body_html));
        return Task::none();
    };
    state.area = area;
    state.quit_on_close = quit_on_close;
    state.title = title;
    state.size = size;
    state.body = html::to_lines(&html::parse(body_html, popup::body_limits(size)));
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
    let card = container(
        Column::with_capacity(2)
            .push(title_row(state))
            .push(body_column(state))
            .spacing(popup::ROW_GAP),
    )
    .padding(Padding {
        top: popup::PAD_TOP,
        bottom: popup::PAD_BOTTOM,
        left: popup::PAD_LEFT,
        right: popup::PAD_RIGHT,
    })
    .width(Length::Fill)
    .height(Length::Fill)
    .style(|_theme| card_style());

    // 滑入偏移:入场动画期间卡片自右向左就位(以左内边距驱动)
    let sliding = container(card)
        .padding(Padding {
            top: 0.0,
            bottom: 0.0,
            left: state.slide,
            right: 0.0,
        })
        .width(Length::Fill)
        .height(Length::Fill)
        .style(|_theme| ContainerStyle {
            background: Some(Color::WHITE.into()),
            ..ContainerStyle::default()
        });

    // 整窗点击关闭(与关闭钮同为 Close,重复消息幂等)
    mouse_area(sliding).on_press(Message::Close).into()
}

/// 白底方角卡片描边
fn card_style() -> ContainerStyle {
    ContainerStyle {
        background: Some(Color::WHITE.into()),
        border: iced::Border {
            color: CARD_BORDER,
            width: 1.0,
            ..iced::Border::default()
        },
        ..ContainerStyle::default()
    }
}

/// 标题行:加粗标题(单行截断)+ 关闭钮,垂直居中
fn title_row(state: &State) -> Element<'_, Message> {
    row![
        text(popup::elide_title(&state.title, state.size.width))
            .size(16.0)
            .font(BOLD)
            .color(TITLE_COLOR)
            .wrapping(iced::widget::text::Wrapping::None)
            .width(Length::Fill),
        close_button(state.hover_close),
    ]
    .height(popup::TITLE_ROW_H)
    .align_y(iced::Alignment::Center)
    .into()
}

/// 关闭钮:20×20 圆形 hover 底色,字形垂直水平居中
fn close_button(hover: bool) -> Element<'static, Message> {
    let circle = container(text("×").size(16.0).font(UI_FONT).color(CLOSE_GLYPH))
        .width(20.0)
        .height(20.0)
        // iced Container 默认 Left/Top 对齐,必须显式居中
        .align_x(iced::alignment::Horizontal::Center)
        .align_y(iced::alignment::Vertical::Center)
        .style(move |_theme| ContainerStyle {
            background: hover.then_some(CLOSE_HOVER_BG.into()),
            border: iced::Border {
                radius: iced::border::Radius::from(10.0),
                ..iced::Border::default()
            },
            ..ContainerStyle::default()
        });

    let slot = container(circle).center(popup::TITLE_ROW_H);

    mouse_area(slot)
        .on_press(Message::Close)
        .on_enter(Message::CloseHover(true))
        .on_exit(Message::CloseHover(false))
        .into()
}

/// 正文:逐行富文本(加粗/颜色按 span,字号/行高按行)
fn body_column(state: &State) -> Element<'_, Message> {
    let mut lines = Column::with_capacity(state.body.len());
    for line in &state.body {
        let spans: Vec<Span<'_, (), Font>> = line
            .runs
            .iter()
            .map(|run| {
                span(run.text.as_str())
                    .color_maybe(
                        run.color
                            .map(|(red, green, blue)| Color::from_rgb8(red, green, blue)),
                    )
                    .font_maybe(run.bold.then_some(BOLD))
            })
            .collect();
        lines = lines.push(
            rich_text(spans)
                .font(UI_FONT)
                .size(f32::from(line.size))
                .line_height(popup::BODY_LINE_HEIGHT)
                // 行由 Rust 侧预折,禁二次换行:估宽偏差只裁切,不产生额外行(保住 5 行上限)
                .wrapping(iced::widget::text::Wrapping::None)
                .color(BODY_COLOR),
        );
    }
    lines.into()
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
