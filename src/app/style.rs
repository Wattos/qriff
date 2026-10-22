use iced::widget::{button, container};
use iced::{Background, Border, Color, Theme};

pub fn chrome(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    container::Style {
        background: Some(
            if theme.palette().background.r < 0.5 {
                iced::color!(0x31363b)
            } else {
                iced::color!(0xeff0f1)
            }
            .into(),
        ),
        text_color: Some(p.background.base.text),
        ..Default::default()
    }
}
pub fn card(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    container::Style {
        background: Some(p.background.base.color.into()),
        text_color: Some(p.background.base.text),
        border: Border {
            color: border(theme),
            width: 1.0,
            radius: 5.0.into(),
        },
        ..Default::default()
    }
}
pub fn secondary(theme: &Theme, status: button::Status) -> button::Style {
    let p = theme.extended_palette();
    let mut style = button::secondary(theme, status);
    style.background = Some(Background::Color(
        if matches!(status, button::Status::Hovered) {
            p.primary.weak.color
        } else {
            p.background.base.color
        },
    ));
    style.border = Border {
        color: if matches!(status, button::Status::Hovered | button::Status::Pressed) {
            p.primary.base.color
        } else {
            border(theme)
        },
        width: 1.0,
        radius: 4.0.into(),
    };
    style.text_color = p.background.base.text;
    style
}
pub const BLUE: Color = Color::from_rgb(0.239216, 0.682353, 0.913725);

pub fn theme(dark: bool, accent: Color) -> Theme {
    use iced::theme::Palette;
    Theme::custom(
        if dark { "Breeze Dark" } else { "Breeze Light" },
        Palette {
            background: if dark {
                iced::color!(0x232629)
            } else {
                iced::color!(0xfcfcfc)
            },
            text: if dark {
                iced::color!(0xeff0f1)
            } else {
                iced::color!(0x232629)
            },
            primary: accent,
            success: iced::color!(0x27ae60),
            warning: iced::color!(0xf67400),
            danger: iced::color!(0xda4453),
        },
    )
}

pub fn border(theme: &Theme) -> Color {
    if theme.palette().background.r < 0.5 {
        iced::color!(0x51575d)
    } else {
        iced::color!(0xc8cdd1)
    }
}
pub fn muted(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(if theme.palette().background.r < 0.5 {
            iced::color!(0xb1bac3)
        } else {
            iced::color!(0x64717c)
        }),
    }
}
pub fn accent(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(if theme.palette().background.r < 0.5 {
            theme.palette().primary
        } else {
            iced::color!(0x2980b9)
        }),
    }
}

pub fn icon_tile(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(
            if theme.palette().background.r < 0.5 {
                iced::color!(0x25465a)
            } else {
                iced::color!(0xe2f1fa)
            }
            .into(),
        ),
        border: Border {
            radius: 5.0.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

pub fn focused(
    theme: &Theme,
    status: button::Status,
    primary: bool,
    focused: bool,
) -> button::Style {
    let mut style = if primary {
        button::primary(theme, status)
    } else {
        secondary(theme, status)
    };
    if focused {
        style.border = Border {
            color: if primary {
                theme.palette().text
            } else {
                theme.palette().primary
            },
            width: 3.0,
            radius: 4.0.into(),
        };
    }
    style
}
pub fn focus_ring(theme: &Theme, focused: bool) -> container::Style {
    container::Style {
        border: Border {
            color: if focused {
                theme.palette().primary
            } else {
                Color::TRANSPARENT
            },
            width: 2.0,
            radius: 4.0.into(),
        },
        ..Default::default()
    }
}
