#![cfg_attr(windows, windows_subsystem = "windows")]

use iced::font::Family;
use iced::widget::{column, container, text};
use iced::{Color, Element, Font, Length, Padding, Theme};

const TITLE_FONT: Font = Font {
    family: Family::Name("Microsoft YaHei UI Bold"),
    ..Font::DEFAULT
};
const BODY_FONT: Font = Font {
    family: Family::Name("Microsoft YaHei UI"),
    ..Font::DEFAULT
};

#[derive(Clone, Copy)]
enum Alignment {
    TextBottom,
    ContainerBottom,
    TextCenter,
}

fn main() -> iced::Result {
    iced::application(|| (), update, view)
        .title("Windows 字体对齐预览")
        .theme(theme)
        .window_size((280.0, 440.0))
        .centered()
        .run()
}

const fn update(_: &mut (), _: ()) {}

#[allow(clippy::trivially_copy_pass_by_ref)]
const fn theme(_: &()) -> Theme {
    Theme::Light
}

fn view(_: &()) -> Element<'_, ()> {
    column![
        sample("1  文本填满后靠下", Alignment::TextBottom),
        sample("3  容器靠下", Alignment::ContainerBottom),
        sample("示例  文本填满后居中", Alignment::TextCenter),
    ]
    .spacing(18)
    .padding(16)
    .into()
}

fn sample(label: &'static str, alignment: Alignment) -> Element<'static, ()> {
    let title = text("消息提醒")
        .font(TITLE_FONT)
        .size(16)
        .color(Color::WHITE)
        .width(Length::Fill);
    let title = match alignment {
        Alignment::TextBottom => title
            .height(Length::Fill)
            .align_y(iced::alignment::Vertical::Bottom),
        Alignment::TextCenter => title
            .height(Length::Fill)
            .center()
            .align_x(iced::alignment::Horizontal::Left),
        Alignment::ContainerBottom => title,
    };
    let header = container(title)
        .padding(Padding {
            top: 0.0,
            right: 12.0,
            bottom: 0.0,
            left: 20.0,
        })
        .width(Length::Fill)
        .height(38)
        .align_y(match alignment {
            Alignment::ContainerBottom => iced::alignment::Vertical::Bottom,
            Alignment::TextBottom | Alignment::TextCenter => iced::alignment::Vertical::Center,
        })
        .style(|_| iced::widget::container::Style {
            background: Some(Color::from_rgb8(0x27, 0x34, 0x49).into()),
            ..Default::default()
        });
    let body = container(
        column![
            text("待办通知 1 条").font(BODY_FONT).size(14),
            text("15:21:05").font(BODY_FONT).size(14),
        ]
        .spacing(0),
    )
    .padding(Padding {
        top: 8.0,
        right: 12.0,
        bottom: 8.0,
        left: 20.0,
    })
    .width(Length::Fill)
    .height(Length::Fill)
    .style(|_| iced::widget::container::Style {
        background: Some(Color::from_rgb8(0xf7, 0xf9, 0xfc).into()),
        text_color: Some(Color::from_rgb8(0x3f, 0x47, 0x54)),
        ..Default::default()
    });
    column![
        text(label).size(12),
        column![header, body].width(220).height(100),
    ]
    .spacing(4)
    .into()
}
