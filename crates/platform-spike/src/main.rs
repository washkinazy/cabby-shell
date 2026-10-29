use cabby_config::{BarPosition, Config};
use cabby_platform_spike::niri::{self, Model};
use iced::futures::SinkExt;
use iced::widget::{button, container, row, scrollable, text};
use iced::{Color, Element, Length, Subscription, Task, Theme, window};
use iced_exwlshell::reexport::{Anchor, KeyboardInteractivity, LayerSize};
use iced_exwlshell::settings::{LayerShellSettings, Settings, StartMode};
use iced_exwlshell::shell::{self, ShellEvent, ShellReceiver};
use iced_exwlshell::{daemon, to_exwlshell_message};
use std::collections::BTreeMap;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::time::Duration;

fn configuration() -> cabby_config::Result<Config> {
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
    cabby_config::Source::from_env(path.as_deref())?.load(true)
}
fn color(value: &str) -> Color {
    let [r, g, b] = cabby_config::rgb(value).expect("validated color");
    Color::from_rgb8(r, g, b)
}
fn main() -> iced_exwlshell::Result {
    let config = configuration().unwrap_or_else(|error| {
        eprintln!("{error}");
        std::process::exit(1)
    });
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
    cabby_platform_spike::renderer_diagnostics();
    iced_exwlshell::disable_clipboard();
    let (sender, receiver) = shell::channel();
    let settings = Settings {
        shell_broadcast: sender,
        default_text_size: iced::Pixels(f32::from(config.font_size)),
        layer_settings: LayerShellSettings {
            anchor: (if config.bar_position == BarPosition::Top {
                Anchor::Top
            } else {
                Anchor::Bottom
            }) | Anchor::Left
                | Anchor::Right,
            size: LayerSize::fill_width(u32::from(config.bar_height)),
            exclusive_zone: i32::from(config.bar_height),
            keyboard_interactivity: KeyboardInteractivity::None,
            start_mode: StartMode::AllScreens,
            ..Default::default()
        },
        ..Default::default()
    };
    daemon(
        move || Bar {
            config: config.clone(),
            shell: receiver.clone(),
            model: Model::default(),
            outputs: BTreeMap::new(),
            action_pending: false,
            action_failed: false,
        },
        || "cabby-bar".into(),
        Bar::update,
        Bar::view,
    )
    .subscription(Bar::subscription)
    .theme(Bar::theme)
    .settings(settings)
    .run()
}
struct Bar {
    config: Config,
    shell: ShellReceiver,
    model: Model,
    outputs: BTreeMap<window::Id, String>,
    action_pending: bool,
    action_failed: bool,
}
#[to_exwlshell_message]
#[derive(Debug, Clone)]
enum Message {
    Niri(niri::Event),
    Disconnected,
    Shell(ShellEvent),
    Focus(u64),
    FocusResult(Result<(), String>),
}
fn events() -> impl iced::futures::Stream<Item = Message> {
    iced::stream::channel(
        64,
        |mut output: iced::futures::channel::mpsc::Sender<Message>| async move {
            loop {
                if let Some(path) = std::env::var_os("NIRI_SOCKET")
                    && let Ok(mut events) = niri::Events::connect(&PathBuf::from(path)).await
                {
                    while let Ok(event) = events.next().await {
                        if matches!(event, niri::Event::Ignored) {
                            continue;
                        }
                        if output.send(Message::Niri(event)).await.is_err() {
                            return;
                        }
                    }
                }
                if output.send(Message::Disconnected).await.is_err() {
                    return;
                }
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        },
    )
}
impl Bar {
    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            Subscription::run(events),
            self.shell.listen().map(Message::Shell),
        ])
    }
    fn theme(&self, _: window::Id) -> Theme {
        Theme::custom(
            "Cabby",
            iced::theme::Palette {
                background: color(&self.config.background),
                text: color(&self.config.foreground),
                primary: color(&self.config.accent),
                success: color(&self.config.accent),
                danger: Color::from_rgb8(235, 110, 110),
                warning: Color::from_rgb8(235, 190, 110),
            },
        )
    }
    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Niri(event) => self.model.apply(event),
            Message::Disconnected => {
                self.model.disconnect();
                self.action_failed = false;
            }
            Message::Shell(ShellEvent::WindowOutputChanged { window, output }) => {
                self.outputs.remove(&window);
                if let Some(name) = output.and_then(|o| o.name) {
                    self.outputs.insert(window, name);
                }
            }
            Message::Shell(ShellEvent::Closed(id)) => {
                self.outputs.remove(&id);
            }
            Message::Shell(ShellEvent::OutputRemoved(output)) => {
                if let Some(name) = output.name {
                    self.outputs.retain(|_, v| *v != name);
                }
            }
            Message::Focus(id)
                if self.model.connected
                    && !self.action_pending
                    && self.model.workspaces.contains_key(&id) =>
            {
                self.action_pending = true;
                return Task::perform(
                    async move {
                        let path = std::env::var_os("NIRI_SOCKET").ok_or("niri unavailable")?;
                        niri::focus(&PathBuf::from(path), id).await
                    },
                    Message::FocusResult,
                );
            }
            Message::FocusResult(result) => {
                self.action_pending = false;
                self.action_failed = result.is_err();
            }
            _ => (),
        }
        Task::none()
    }
    fn view(&self, id: window::Id) -> Element<'_, Message> {
        let output = self.outputs.get(&id).map(String::as_str).unwrap_or("");
        let mut workspaces = row![].spacing(f32::from(self.config.bar_spacing));
        for workspace in self.model.on_output(output) {
            let name = workspace
                .name
                .clone()
                .unwrap_or_else(|| workspace.idx.to_string());
            let label = format!(
                "{}{}",
                if workspace.is_urgent { "! " } else { "" },
                niri::label(&name, 24)
            );
            let mut item = button(text(label).line_height(1.0)).padding([2, 8]);
            if !self.action_pending {
                item = item.on_press(Message::Focus(workspace.id));
            }
            item = if workspace.is_active {
                item.style(button::primary)
            } else {
                item.style(button::text)
            };
            workspaces = workspaces.push(item);
        }
        let status = if !self.model.connected {
            "niri unavailable"
        } else if self.action_failed {
            "Workspace switch failed"
        } else if output.is_empty() {
            "Waiting for output"
        } else {
            ""
        };
        let title = if self.config.show_window_title {
            self.model.title(output)
        } else {
            String::new()
        };
        let contents = row![
            scrollable(workspaces)
                .direction(scrollable::Direction::Horizontal(
                    scrollable::Scrollbar::new()
                ))
                .width(Length::FillPortion(1)),
            container(
                text(title)
                    .wrapping(iced::widget::text::Wrapping::None)
                    .line_height(1.0)
            )
            .clip(true)
            .width(Length::FillPortion(2)),
            text(status).line_height(1.0)
        ]
        .spacing(f32::from(self.config.bar_spacing))
        .align_y(iced::Alignment::Center);
        container(contents)
            .padding([0, self.config.bar_padding])
            .width(Length::Fill)
            .height(Length::Fill)
            .center_y(Length::Fill)
            .style(|_| container::Style {
                background: Some(color(&self.config.background).into()),
                text_color: Some(color(&self.config.foreground)),
                ..Default::default()
            })
            .into()
    }
}
