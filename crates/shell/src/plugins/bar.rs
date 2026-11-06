use crate::niri::{self, Model};
use crate::ui;
use cabby_config::{BarWidget, Config};
use iced::futures::SinkExt;
use iced::widget::{container, mouse_area, row, scrollable, text};
use iced::{Element, Length, Subscription, Task};
use std::path::PathBuf;
use std::time::Duration;

pub(crate) struct Bar {
    config: Config,
    pub(crate) tray: super::tray::Tray,
    model: Model,
    clock: String,
    pub(crate) audio_label: String,
    pub(crate) audio_icon: ui::symbol::Symbol,
    pub(crate) bluetooth_icon: ui::symbol::Symbol,
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
    Clipboard,
    Hint(Option<String>),
    Screenshot,
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
            audio_icon: ui::symbol::Symbol::Muted,
            bluetooth_icon: ui::symbol::Symbol::BluetoothOff,
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
            || self.config.osd.enabled
                && (self.config.osd.volume
                    || self.config.osd.microphone
                    || self.config.osd.caps_lock
                    || self.config.osd.num_lock)
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
    pub(crate) fn focused_output(&self) -> Option<&str> {
        self.model
            .workspaces
            .values()
            .find(|w| w.is_focused)
            .and_then(|w| w.output.as_deref())
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
            let mark = container(iced::widget::Space::new())
                .width(14)
                .height(2)
                .style(if workspace.is_active {
                    ui::accent_mark
                } else {
                    crate::ui::blank
                });
            let mut item = crate::ui::button(
                iced::widget::column![
                    text(label)
                        .size(f32::from(self.config.font_size))
                        .line_height(1.0),
                    mark
                ]
                .spacing(2)
                .align_x(iced::Alignment::Center),
            )
            .padding([2, 8]);
            if !self.action_pending {
                item = item.on_press(Message::Focus(workspace.id));
            }
            item = if workspace.is_active {
                item.style(crate::ui::selected)
            } else {
                item.style(crate::ui::quiet)
            };
            workspaces = workspaces.push(item);
        }
        if self.action_failed {
            workspaces = workspaces.push(text("Action failed"));
        }
        workspaces.into()
    }
    fn icon<'a>(
        &self,
        symbol: ui::symbol::Symbol,
        label: String,
        action: Option<Message>,
        active: bool,
    ) -> Element<'a, Message> {
        let control = crate::ui::button(ui::symbol::view(symbol.handle()))
            .padding([2, 4])
            .style(if active { ui::selected } else { ui::quiet });
        let control = if let Some(action) = action {
            control.on_press(action)
        } else {
            control
        };
        hint(control.into(), label)
    }
    fn widget(&self, widget: BarWidget, output: &str) -> Element<'_, Message> {
        use ui::symbol::Symbol;
        let label = |value: String| {
            text(value)
                .size(f32::from(self.config.font_size))
                .wrapping(iced::widget::text::Wrapping::None)
                .line_height(1.0)
        };
        match widget {
            BarWidget::Screenshot => self.icon(
                Symbol::Screenshot,
                "Take a screenshot".into(),
                Some(Message::Screenshot),
                false,
            ),
            BarWidget::Clipboard => self.icon(
                Symbol::Clipboard,
                "Clipboard history".into(),
                Some(Message::Clipboard),
                false,
            ),
            BarWidget::Workspaces => self.workspaces(output),
            BarWidget::Caffeine => {
                let style = self.caffeine.style(self.caffeine_failed);
                let icon = iced::widget::svg(self.caffeine.icon())
                    .width(18)
                    .height(18)
                    .style(move |theme: &iced::Theme, _| iced::widget::svg::Style {
                        color: Some(style(theme, iced::widget::button::Status::Active).text_color),
                    });
                let control = crate::ui::button(icon).padding([2, 4]).style(style);
                let control = if self.caffeine_pending {
                    control
                } else {
                    control.on_press(Message::CaffeineToggle)
                };
                hint(
                    control.into(),
                    if self.caffeine_failed {
                        "Caffeine action failed".into()
                    } else {
                        self.caffeine.detail()
                    },
                )
            }
            BarWidget::Nightlight => self.icon(
                self.nightlight.icon(),
                if self.nightlight_failed {
                    "Night light action failed".into()
                } else {
                    format!("{} · {}", self.nightlight.label(), self.nightlight.detail())
                },
                (!self.nightlight_pending).then_some(Message::NightlightToggle),
                self.nightlight.active(),
            ),
            BarWidget::Idle => self.icon(
                Symbol::Lock,
                if self.idle_failed {
                    "Idle action failed".into()
                } else {
                    self.idle.label().into()
                },
                (!self.idle_pending).then_some(Message::IdleToggle),
                false,
            ),
            BarWidget::Clock => label(self.clock.clone()).into(),
            BarWidget::Audio => self.icon(
                self.audio_icon,
                self.audio_label.clone(),
                Some(Message::Audio),
                false,
            ),
            BarWidget::Bluetooth => self.icon(
                self.bluetooth_icon,
                self.bluetooth_label.clone(),
                Some(Message::Bluetooth),
                false,
            ),
            BarWidget::Apps => self.icon(
                Symbol::Apps,
                "Open applications".into(),
                Some(Message::Launcher),
                false,
            ),
            BarWidget::Network => self.icon(
                match self.network.transport {
                    crate::network::Transport::Wifi => Symbol::Wifi,
                    crate::network::Transport::Offline => Symbol::Offline,
                    _ => Symbol::Wired,
                },
                format!("{} · {}", self.network.label, self.network.detail),
                None,
                false,
            ),
            BarWidget::Tray => self.tray.view(&self.config).map(Message::Tray),
            BarWidget::Title => label(if self.config.show_window_title {
                self.model.title(output)
            } else {
                String::new()
            })
            .into(),
            BarWidget::Status => {
                let status = if !self.model.connected {
                    "niri unavailable"
                } else if self.action_failed {
                    "Action failed"
                } else if output.is_empty() {
                    "Waiting for output"
                } else {
                    ""
                };
                if status.is_empty() {
                    iced::widget::Space::new().into()
                } else {
                    self.icon(Symbol::Warning, status.into(), None, false)
                }
            }
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
            .style(ui::bar_style)
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

fn hint<'a>(content: Element<'a, Message>, label: String) -> Element<'a, Message> {
    mouse_area(content)
        .on_enter(Message::Hint(Some(label)))
        .on_exit(Message::Hint(None))
        .into()
}
