//! Solid Fluent-inspired surfaces. All fills are opaque and derive from user colors.
use iced::widget::{button, container, slider, text_input};
use iced::{Border, Color, Shadow, Theme, Vector};

pub(super) fn mix(a: Color, b: Color, amount: f32) -> Color {
    Color::from_rgb(
        a.r + (b.r - a.r) * amount,
        a.g + (b.g - a.g) * amount,
        a.b + (b.b - a.b) * amount,
    )
}
pub(crate) fn muted(theme: &Theme) -> Color {
    let p = theme.palette();
    mix(p.background, p.text, 0.74)
}
fn border(theme: &Theme) -> Color {
    let p = theme.palette();
    mix(p.background, p.text, 0.20)
}
fn surface(theme: &Theme, amount: f32) -> Color {
    let p = theme.palette();
    mix(p.background, p.text, amount)
}
pub(super) fn panel(theme: &Theme, shadows: bool) -> container::Style {
    container::Style {
        background: Some(theme.palette().background.into()),
        text_color: Some(theme.palette().text),
        border: Border {
            color: border(theme),
            width: 1.,
            radius: 8.into(),
        },
        shadow: if shadows {
            Shadow {
                color: Color::from_rgba(0., 0., 0., 0.22),
                offset: Vector::new(0., 2.),
                blur_radius: 8.,
            }
        } else {
            Shadow::default()
        },
        ..Default::default()
    }
}
pub(crate) fn raised(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(surface(theme, 0.055).into()),
        text_color: Some(theme.palette().text),
        border: Border {
            color: border(theme),
            width: 1.,
            radius: 4.into(),
        },
        ..Default::default()
    }
}
pub(crate) fn selected_group(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(surface(theme, 0.10).into()),
        ..raised(theme)
    }
}
pub(crate) fn accent_mark(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(theme.palette().primary.into()),
        border: Border {
            radius: 2.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}
pub(crate) fn bar(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(theme.palette().background.into()),
        text_color: Some(theme.palette().text),
        ..Default::default()
    }
}
fn control(
    theme: &Theme,
    status: button::Status,
    fill: Color,
    text: Color,
    outlined: bool,
) -> button::Style {
    let p = theme.palette();
    let fill = match status {
        button::Status::Hovered => mix(fill, p.text, 0.07),
        button::Status::Pressed => mix(fill, p.text, 0.12),
        button::Status::Disabled => mix(fill, p.background, 0.6),
        _ => fill,
    };
    button::Style {
        background: Some(fill.into()),
        text_color: if status == button::Status::Disabled {
            mix(fill, text, 0.45)
        } else {
            text
        },
        border: Border {
            color: border(theme),
            width: if outlined { 1. } else { 0. },
            radius: 4.into(),
        },
        ..Default::default()
    }
}
pub(crate) fn standard(theme: &Theme, status: button::Status) -> button::Style {
    control(
        theme,
        status,
        surface(theme, 0.055),
        theme.palette().text,
        true,
    )
}
pub(crate) fn quiet(theme: &Theme, status: button::Status) -> button::Style {
    control(
        theme,
        status,
        theme.palette().background,
        theme.palette().text,
        false,
    )
}
pub(crate) fn selected(theme: &Theme, status: button::Status) -> button::Style {
    control(
        theme,
        status,
        surface(theme, 0.12),
        theme.palette().text,
        false,
    )
}
pub(crate) fn primary(theme: &Theme, status: button::Status) -> button::Style {
    let p = theme.extended_palette().primary.base;
    control(theme, status, p.color, p.text, false)
}
pub(crate) fn danger(theme: &Theme, status: button::Status) -> button::Style {
    let p = theme.extended_palette().danger.base;
    control(theme, status, p.color, p.text, false)
}
pub(crate) fn input(theme: &Theme, status: text_input::Status) -> text_input::Style {
    let focused = matches!(status, text_input::Status::Focused { .. });
    text_input::Style {
        background: surface(theme, 0.035).into(),
        border: Border {
            color: if focused {
                theme.palette().primary
            } else {
                border(theme)
            },
            width: if focused { 2. } else { 1. },
            radius: 4.into(),
        },
        icon: muted(theme),
        placeholder: muted(theme),
        value: if status == text_input::Status::Disabled {
            muted(theme)
        } else {
            theme.palette().text
        },
        selection: mix(theme.palette().background, theme.palette().primary, 0.4),
    }
}
pub(crate) fn slider_style(theme: &Theme, status: slider::Status) -> slider::Style {
    slider::Style {
        rail: slider::Rail {
            backgrounds: (theme.palette().primary.into(), surface(theme, 0.17).into()),
            width: 4.,
            border: Border {
                radius: 2.into(),
                ..Default::default()
            },
        },
        handle: slider::Handle {
            shape: slider::HandleShape::Circle {
                radius: if matches!(status, slider::Status::Active) {
                    6.
                } else {
                    7.
                },
            },
            background: theme.palette().primary.into(),
            border_width: 2.,
            border_color: theme.palette().background,
        },
    }
}

pub(crate) fn blank(_: &Theme) -> container::Style {
    container::Style::default()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn surfaces_and_states_remain_opaque_for_light_and_dark_palettes() {
        // Custom colors and both luminance directions must survive style derivation.
        for theme in [
            Theme::Light,
            Theme::Dark,
            crate::ui::theme(&cabby_config::Config {
                background: "#eef2f6".into(),
                foreground: "#19232d".into(),
                accent: "#326b85".into(),
                ..Default::default()
            }),
        ] {
            for s in [
                panel(&theme, true),
                panel(&theme, false),
                raised(&theme),
                selected_group(&theme),
            ] {
                assert!(matches!(s.background, Some(iced::Background::Color(c)) if c.a == 1.));
                assert_eq!(s.text_color.unwrap(), theme.palette().text);
            }
            for status in [
                button::Status::Active,
                button::Status::Hovered,
                button::Status::Pressed,
                button::Status::Disabled,
            ] {
                for style in [
                    standard(&theme, status),
                    quiet(&theme, status),
                    selected(&theme, status),
                    primary(&theme, status),
                ] {
                    assert!(
                        matches!(style.background, Some(iced::Background::Color(c)) if c.a == 1.)
                    );
                    assert_eq!(style.text_color.a, 1.);
                }
            }
            assert_ne!(
                quiet(&theme, button::Status::Active).background,
                selected(&theme, button::Status::Active).background
            );
            assert_eq!(
                panel(&theme, false).background,
                Some(theme.palette().background.into())
            );
        }
    }
}
