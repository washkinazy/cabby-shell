use crate::niri::{self, Model};
use crate::ui::{self, color};
use cabby_config::Config;
use iced::futures::SinkExt;
use iced::widget::{button, container, row, scrollable, text};
use iced::{Element, Length, Subscription, Task};
use std::path::PathBuf;
use std::time::Duration;

pub(crate) struct Bar {
    config: Config,
    model: Model,
    action_pending: bool,
    action_failed: bool,
}
#[derive(Debug, Clone)]
pub(crate) enum Message {
    Niri(niri::Event),
    Disconnected,
    Focus(u64),
    Launcher,
    Audio,
    Bluetooth,
    LauncherResult(Result<(), String>),
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
    pub(crate) fn new(config: Config) -> Self {
        Self {
            config,
            model: Model::default(),
            action_pending: false,
            action_failed: false,
        }
    }
    pub(crate) fn subscription(&self) -> Subscription<Message> {
        Subscription::run(events)
    }

    pub(crate) fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Launcher => {
                return Task::perform(
                    ui::command(vec!["launcher".into()]),
                    Message::LauncherResult,
                );
            }
            Message::LauncherResult(result) => {
                self.action_failed = result.is_err();
            }
            Message::Niri(event) => self.model.apply(event),
            Message::Disconnected => {
                self.model.disconnect();
                self.action_failed = false;
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
    pub(crate) fn view(&self, output: &str) -> Element<'_, Message> {
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
            "Action failed"
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
            button("Apps").on_press(Message::Launcher),
            button("Audio").on_press(Message::Audio),
            button("Bluetooth").on_press(Message::Bluetooth),
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
