use crate::niri::{self, Model};
use crate::ui::{self, color};
use cabby_config::{BarWidget, Config};
use iced::futures::SinkExt;
use iced::widget::{button, container, row, scrollable, text, tooltip};
use iced::{Element, Length, Subscription, Task};
use std::path::PathBuf;
use std::time::Duration;

pub(crate) struct Bar {
    config: Config,
    pub(crate) tray: super::tray::Tray,
    model: Model,
    clock: String,
    pub(crate) audio_label: String,
    pub(crate) bluetooth_label: String,
    network: crate::network::Status,
    caffeine: super::caffeine::Status,
    caffeine_pending: bool,
    caffeine_failed: bool,
    nightlight: super::nightlight::Status,
    nightlight_pending: bool,
    nightlight_failed: bool,
    idle: super::idle::Status,
    idle_pending: bool,
    idle_failed: bool,
    action_pending: bool,
    action_failed: bool,
}
#[derive(Debug, Clone)]
pub(crate) enum Message {
    Niri(niri::Event),
    Tray(super::tray::Message),
    Disconnected,
    Clock(String),
    IdleState(super::idle::Status),
    CaffeineState(super::caffeine::Status),
    CaffeineToggle,
    CaffeineResult(Result<(), String>),
    NightlightState(super::nightlight::Status),
    NightlightToggle,
    NightlightResult(Result<(), String>),
    IdleToggle,
    IdleResult(Result<(), String>),
    Network(crate::network::Status),
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
            tray: super::tray::Tray::default(),
            model: Model::default(),
            clock: super::clock::now(),
            audio_label: "Audio ?".into(),
            bluetooth_label: "Bluetooth ?".into(),
            network: crate::network::Status::default(),
            caffeine: Default::default(),
            caffeine_pending: false,
            caffeine_failed: false,
            nightlight: super::nightlight::Status::default(),
            nightlight_pending: false,
            nightlight_failed: false,
            idle: super::idle::Status::default(),
            idle_pending: false,
            idle_failed: false,
            action_pending: false,
            action_failed: false,
        }
    }
    pub(crate) fn configure(&mut self, config: Config) {
        self.config = config;
    }
    pub(crate) fn subscription(&self) -> Subscription<Message> {
        let mut subscriptions = Vec::new();
        if [BarWidget::Workspaces, BarWidget::Title, BarWidget::Status]
            .iter()
            .any(|w| self.config.has_widget(*w))
        {
            subscriptions.push(Subscription::run(events));
        }
        if self.config.has_widget(BarWidget::Tray) {
            subscriptions.push(self.tray.subscription().map(Message::Tray));
        }
        if self.config.has_widget(BarWidget::Clock) {
            subscriptions.push(Subscription::run(clock_events));
        }
        if self.config.has_widget(BarWidget::Network) {
            subscriptions.push(Subscription::run(network_events));
        }
        if self.config.has_widget(BarWidget::Idle) {
            subscriptions.push(Subscription::run(super::idle::events).map(Message::IdleState));
        }
        if self.config.has_widget(BarWidget::Nightlight) {
            subscriptions
                .push(Subscription::run(super::nightlight::events).map(Message::NightlightState));
        }
        if self.config.has_widget(BarWidget::Caffeine) {
            subscriptions
                .push(Subscription::run(super::caffeine::events).map(Message::CaffeineState));
        }
        Subscription::batch(subscriptions)
    }

    pub(crate) fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::CaffeineState(status) => self.caffeine = status,
            Message::CaffeineToggle if !self.caffeine_pending => {
                self.caffeine_pending = true;
                return Task::perform(
                    ui::command(vec!["caffeine-toggle".into()]),
                    Message::CaffeineResult,
                );
            }
            Message::CaffeineResult(result) => {
                self.caffeine_pending = false;
                self.caffeine_failed = result.is_err();
            }
            Message::NightlightState(status) => self.nightlight = status,
            Message::NightlightToggle if !self.nightlight_pending => {
                self.nightlight_pending = true;
                return Task::perform(
                    ui::command(vec!["nightlight-toggle".into()]),
                    Message::NightlightResult,
                );
            }
            Message::NightlightResult(result) => {
                self.nightlight_pending = false;
                self.nightlight_failed = result.is_err();
            }
            Message::IdleState(state) => self.idle = state,
            Message::IdleToggle if !self.idle_pending => {
                self.idle_pending = true;
                return Task::perform(
                    ui::command(vec!["idle-inhibit-toggle".into()]),
                    Message::IdleResult,
                );
            }
            Message::IdleResult(result) => {
                self.idle_pending = false;
                self.idle_failed = result.is_err();
            }
            Message::Clock(clock) => self.clock = clock,
            Message::Network(network) => self.network = network,
            Message::Tray(message) => return self.tray.update(message).map(Message::Tray),
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
    fn workspaces(&self, output: &str) -> Element<'_, Message> {
        if !self.model.connected {
            return text("niri unavailable")
                .wrapping(iced::widget::text::Wrapping::None)
                .into();
        }
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
            let mut item = button(
                text(label)
                    .size(f32::from(self.config.font_size))
                    .line_height(1.0),
            )
            .padding([2, 8]);
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
        if self.action_failed {
            workspaces = workspaces.push(text("Action failed"));
        }
        workspaces.into()
    }
    fn widget(&self, widget: BarWidget, output: &str) -> Element<'_, Message> {
        let label = |value: String| {
            text(value)
                .size(f32::from(self.config.font_size))
                .wrapping(iced::widget::text::Wrapping::None)
                .line_height(1.0)
        };
        match widget {
            BarWidget::Workspaces => self.workspaces(output),
            BarWidget::Caffeine => {
                let icon = iced::widget::svg(self.caffeine.icon())
                    .width(18)
                    .height(18)
                    .style(|theme: &iced::Theme, _| iced::widget::svg::Style {
                        color: Some(theme.palette().text),
                    });
                let control = button(icon)
                    .padding([2, 3])
                    .style(self.caffeine.style(self.caffeine_failed));
                let control = if self.caffeine_pending {
                    control
                } else {
                    control.on_press(Message::CaffeineToggle)
                };
                tooltip(
                    control,
                    text(self.caffeine.detail()),
                    tooltip::Position::Bottom,
                )
                .into()
            }
            BarWidget::Nightlight => {
                let control = button(ui::body(
                    &self.config,
                    if self.nightlight_failed {
                        "Night action failed"
                    } else {
                        self.nightlight.label()
                    },
                ));
                let control = if self.nightlight_pending {
                    control
                } else {
                    control.on_press(Message::NightlightToggle)
                };
                tooltip(
                    control,
                    text(self.nightlight.detail()),
                    tooltip::Position::Bottom,
                )
                .into()
            }
            BarWidget::Idle => {
                let label = if self.idle_failed {
                    "Idle action failed"
                } else {
                    self.idle.label()
                };
                let control = button(ui::body(&self.config, label));
                if self.idle_pending {
                    control.into()
                } else {
                    control.on_press(Message::IdleToggle).into()
                }
            }
            BarWidget::Clock => label(self.clock.clone()).into(),
            BarWidget::Audio => button(ui::body(&self.config, &self.audio_label))
                .on_press(Message::Audio)
                .into(),
            BarWidget::Bluetooth => button(ui::body(&self.config, &self.bluetooth_label))
                .on_press(Message::Bluetooth)
                .into(),
            BarWidget::Apps => button(if self.action_failed {
                "Action failed"
            } else {
                "Apps"
            })
            .on_press(Message::Launcher)
            .into(),
            BarWidget::Network => tooltip(
                label(self.network.label.clone()),
                text(self.network.detail),
                tooltip::Position::Bottom,
            )
            .into(),
            BarWidget::Tray => self.tray.view(&self.config).map(Message::Tray),
            BarWidget::Title => label(if self.config.show_window_title {
                self.model.title(output)
            } else {
                String::new()
            })
            .into(),
            BarWidget::Status => label(
                if !self.model.connected {
                    "niri unavailable"
                } else if self.action_failed {
                    "Action failed"
                } else if output.is_empty() {
                    "Waiting for output"
                } else {
                    ""
                }
                .into(),
            )
            .into(),
        }
    }
    fn region(
        &self,
        widgets: &[BarWidget],
        output: &str,
        alignment: iced::alignment::Horizontal,
    ) -> Element<'_, Message> {
        let contents = row(widgets.iter().map(|w| self.widget(*w, output)))
            .spacing(f32::from(self.config.bar_spacing))
            .align_y(iced::Alignment::Center);
        // Shrink to contents when there is room; scrolling bounds overflow to its region.
        let scroll = scrollable(contents).direction(scrollable::Direction::Horizontal(
            scrollable::Scrollbar::new().width(3).scroller_width(3),
        ));
        container(scroll)
            .width(if alignment == iced::alignment::Horizontal::Center {
                Length::Shrink
            } else {
                Length::FillPortion(1)
            })
            .align_x(alignment)
            .center_y(Length::Fill)
            .clip(true)
            .into()
    }
    pub(crate) fn view(&self, output: &str) -> Element<'_, Message> {
        let contents = row![
            self.region(
                &self.config.bar_left,
                output,
                iced::alignment::Horizontal::Left
            ),
            self.region(
                &self.config.bar_middle,
                output,
                iced::alignment::Horizontal::Center
            ),
            self.region(
                &self.config.bar_right,
                output,
                iced::alignment::Horizontal::Right
            )
        ]
        .spacing(f32::from(self.config.bar_spacing))
        .height(Length::Fill);
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

fn clock_events() -> impl iced::futures::Stream<Item = Message> {
    iced::stream::channel(
        1,
        |mut output: iced::futures::channel::mpsc::Sender<Message>| async move {
            loop {
                if output
                    .send(Message::Clock(super::clock::now()))
                    .await
                    .is_err()
                {
                    return;
                }
                let elapsed = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default();
                tokio::time::sleep(super::clock::until_next_minute(elapsed)).await;
            }
        },
    )
}
fn network_events() -> impl iced::futures::Stream<Item = Message> {
    iced::stream::channel(
        1,
        |mut output: iced::futures::channel::mpsc::Sender<Message>| async move {
            let mut previous = None;
            loop {
                if let Ok(mut client) = crate::network::Client::connect(None).await
                    && let Ok(mut status) = client.snapshot().await
                {
                    loop {
                        if previous.as_ref() != Some(&status) {
                            previous = Some(status.clone());
                            if output.send(Message::Network(status)).await.is_err() {
                                return;
                            }
                        }
                        match client.next().await {
                            Ok(next) => status = next,
                            Err(_) => break,
                        }
                    }
                }
                let unavailable = crate::network::Status::default();
                if previous.as_ref() != Some(&unavailable) {
                    previous = Some(unavailable.clone());
                    if output.send(Message::Network(unavailable)).await.is_err() {
                        return;
                    }
                }
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        },
    )
}
