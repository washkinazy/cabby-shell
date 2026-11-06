use cabby_config::{Config, Source};
use iced::{Color, Theme};
use std::{os::unix::process::CommandExt, path::PathBuf};
mod fluent;
pub(crate) use fluent::{
    accent_mark, bar as bar_style, blank, danger, input as input_style, primary, quiet, raised,
    selected, selected_group, slider_style, standard,
};
pub(crate) fn panel(
    renderer: cabby_config::Renderer,
) -> impl Fn(&Theme) -> iced::widget::container::Style {
    move |theme| fluent::panel(theme, renderer == cabby_config::Renderer::Vulkan)
}

pub(crate) fn button<'a, Message: 'a>(
    content: impl Into<iced::Element<'a, Message>>,
) -> iced::widget::Button<'a, Message> {
    iced::widget::button(content)
        .padding([5, 10])
        .style(standard)
}
pub(crate) fn heading<'a>(
    config: &Config,
    value: impl iced::widget::text::IntoFragment<'a>,
) -> iced::widget::Text<'a> {
    body(config, value)
        .size(f32::from(config.font_size) + 5.)
        .font(iced::Font {
            weight: iced::font::Weight::Semibold,
            ..Default::default()
        })
}
pub(crate) fn secondary<'a>(
    config: &Config,
    value: impl iced::widget::text::IntoFragment<'a>,
) -> iced::widget::Text<'a> {
    body(config, value).color(fluent::mix(
        color(&config.background),
        color(&config.foreground),
        0.74,
    ))
}
pub fn configuration_source() -> cabby_config::Result<Source> {
    let mut args = std::env::args_os().skip(1);
    let path = match args.next() {
        None => None,
        Some(flag) if flag == "--config" => Some(PathBuf::from(
            args.next().ok_or("--config requires a path")?,
        )),
        _ => return Err("Usage: cabby-platform-spike [--config PATH]".into()),
    };
    if args.next().is_some() {
        return Err("unexpected arguments".into());
    }
    Source::from_env(path.as_deref())
}
pub fn color(value: &str) -> Color {
    let [r, g, b] = cabby_config::rgb(value).expect("validated color");
    Color::from_rgb8(r, g, b)
}

pub fn renderer(config: &Config) {
    let environment = config.renderer.environment();
    if environment
        .iter()
        .any(|(key, value)| std::env::var(key).as_deref() != Ok(*value))
    {
        let error = std::process::Command::new(std::env::current_exe().expect("executable path"))
            .args(std::env::args_os().skip(1))
            .envs(environment)
            .exec();
        eprintln!("cannot apply renderer environment: {error}");
        std::process::exit(1);
    }
}
pub fn theme(config: &Config) -> Theme {
    Theme::custom(
        "Cabby",
        iced::theme::Palette {
            background: color(&config.background),
            text: color(&config.foreground),
            primary: color(&config.accent),
            success: color(&config.accent),
            danger: Color::from_rgb8(235, 110, 110),
            warning: Color::from_rgb8(235, 190, 110),
        },
    )
}
/// Helpers stay in the caller's systemd cgroup. No shell invocation or global environment mutation.
pub async fn command(args: Vec<std::ffi::OsString>) -> Result<(), String> {
    let executable = std::env::current_exe()
        .map_err(|_| "Cannot locate cabby executable")?
        .with_file_name("cabby");
    let mut command = tokio::process::Command::new(executable);
    command
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    let status = tokio::time::timeout(std::time::Duration::from_secs(10), command.status())
        .await
        .map_err(|_| "Operation timed out; check session status before retrying")?
        .map_err(|_| "Cannot start cabby helper")?;
    if status.success() {
        Ok(())
    } else {
        Err("Could not launch: check the application, terminal command, and Cabby session".into())
    }
}

/// Shared live typography for feature content; headings may override the size.
pub fn body<'a>(
    config: &Config,
    value: impl iced::widget::text::IntoFragment<'a>,
) -> iced::widget::Text<'a> {
    iced::widget::text(value).size(f32::from(config.font_size))
}
