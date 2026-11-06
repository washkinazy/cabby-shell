//! Output/surface ownership for passive desktop plugins and on-demand device panels.
use super::{
    Context,
    panel::{self, Origin},
};
use crate::{
    plugins::{audio, bar, bluetooth, clipboard, screenshot, tray, wallpaper},
    ui,
};
use cabby_config::{BarPosition, Config};
use iced::widget::{Space, mouse_area};
use iced::{Element, Subscription, Task, window};
use iced_exwlshell::reexport::{
    Anchor, KeyboardInteractivity, Layer, LayerSize, NewLayerShellSettings, OutputOption,
};
use iced_exwlshell::settings::{LayerShellSettings, Settings, StartMode};
use iced_exwlshell::shell::{self, ShellEvent, ShellReceiver};
use iced_exwlshell::{daemon, to_exwlshell_message};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Bar,
    Wallpaper,
}

#[derive(Default)]
struct Surfaces {
    outputs: BTreeMap<u32, String>,
    windows: BTreeMap<window::Id, (u32, Kind)>,
}
impl Surfaces {
    fn add(&mut self, output: u32, name: String, kinds: &[Kind]) -> Vec<(window::Id, Kind)> {
        self.outputs.insert(output, name);
        let mut added = Vec::new();
        for kind in kinds {
            if !self.windows.values().any(|entry| *entry == (output, *kind)) {
                let id = window::Id::unique();
                self.windows.insert(id, (output, *kind));
                added.push((id, *kind));
            }
        }
        added
    }
    fn remove_output(&mut self, output: u32) -> Vec<window::Id> {
        self.outputs.remove(&output);
        let removed: Vec<_> = self
            .windows
            .iter()
            .filter(|(_, (o, _))| *o == output)
            .map(|(id, _)| *id)
            .collect();
        for id in &removed {
            self.windows.remove(id);
        }
        removed
    }
}

enum PanelPlugin {
    Clipboard(clipboard::Picker),
    Screenshot(Box<screenshot::Picker>),
    Tray,
    Audio,
    Bluetooth,
}
struct DevicePanel {
    id: window::Id,
    output: Option<u32>,
    origin: Origin,
    shield: window::Id,
    focused: bool,
    plugin: PanelPlugin,
}
struct Desktop {
    config: Config,
    config_path: Option<std::path::PathBuf>,
    shell: ShellReceiver,
    bar: Option<bar::Bar>,
    wallpaper: Option<wallpaper::Wallpaper>,
    surfaces: Surfaces,
    panel: Option<DevicePanel>,
    origin: Option<Origin>,
    pointers: BTreeMap<window::Id, f32>,
    bluetooth: bluetooth::Bluetooth,
    bluetooth_generation: u64,
    open_bluetooth_on_output: bool,
    bluetooth_probe_stage: Option<u8>,
    tray_probe_stage: Option<u8>,
    audio: audio::Audio,
    open_audio_on_output: bool,
    panel_probe: bool,
    output_sizes: BTreeMap<u32, (u32, u32)>,
}
#[to_exwlshell_message]
#[derive(Clone, Debug)]
enum Message {
    Bar(bar::Message),
    FromBar(window::Id, bar::Message),
    Pointer(window::Id, iced::Point),
    Outside(window::Id),
    Focus(window::Id, bool),
    Audio(window::Id, audio::Message),
    AudioState(audio::Message),
    BluetoothState(u64, bluetooth::Message),
    Bluetooth(window::Id, bluetooth::Message),
    Tray(window::Id, tray::Message),
    Escape(window::Id),
    ProbeClose(window::Id),
    Shell(ShellEvent),
    Resized,
    Config(super::live_config::Update),
    Control(super::control::Command),
    Clipboard(window::Id, clipboard::Message),
    ClipboardRevision,
    Screenshot(window::Id, screenshot::Message),
}

pub(super) fn run(
    context: Context,
    with_bar: bool,
    with_wallpaper: bool,
) -> iced_exwlshell::Result {
    run_with_audio(context, with_bar, with_wallpaper, None)
}

pub(super) fn run_with_audio(
    context: Context,
    with_bar: bool,
    with_wallpaper: bool,
    fixture: Option<cabby_audio::Client>,
) -> iced_exwlshell::Result {
    run_with_panels(context, with_bar, with_wallpaper, fixture, None, false)
}

pub(super) fn run_with_bluetooth(
    context: Context,
    endpoint: cabby_bluetooth::Endpoint,
) -> iced_exwlshell::Result {
    run_with_panels(context, true, true, None, Some(endpoint), false)
}
pub(super) fn run_with_tray(context: Context) -> iced_exwlshell::Result {
    run_with_panels(context, true, true, None, None, true)
}
fn run_with_panels(
    context: Context,
    with_bar: bool,
    with_wallpaper: bool,
    fixture: Option<cabby_audio::Client>,
    bluetooth_fixture: Option<cabby_bluetooth::Endpoint>,
    tray_probe: bool,
) -> iced_exwlshell::Result {
    let config_path = context.source.path;
    let config = context.config;
    let wallpaper = with_wallpaper
        .then(|| wallpaper::Wallpaper::new(config.clone()))
        .transpose()
        .unwrap_or_else(|error| {
            eprintln!("{error}");
            std::process::exit(1)
        });
    let (sender, receiver) = shell::channel();
    let text_size = iced::Pixels(f32::from(config.font_size));
    daemon(
        move || Desktop {
            config: config.clone(),
            config_path: Some(config_path.clone()),
            shell: receiver.clone(),
            bar: with_bar.then(|| bar::Bar::new(config.clone())),
            wallpaper: wallpaper.clone(),
            surfaces: Surfaces::default(),
            panel: None,
            origin: None,
            pointers: BTreeMap::new(),
            audio: audio::Audio::new(fixture.clone().unwrap_or_default()),
            open_audio_on_output: fixture.is_some(),
            panel_probe: fixture.is_some() || bluetooth_fixture.is_some() || tray_probe,
            bluetooth: bluetooth::Bluetooth::new(bluetooth_fixture.clone().unwrap_or_default()),
            bluetooth_generation: 0,
            open_bluetooth_on_output: bluetooth_fixture.is_some(),
            bluetooth_probe_stage: bluetooth_fixture.as_ref().map(|_| 0),
            tray_probe_stage: tray_probe.then_some(0),
            output_sizes: BTreeMap::new(),
        },
        || "cabby-desktop".into(),
        Desktop::update,
        Desktop::view,
    )
    .subscription(Desktop::subscription)
    .theme(|state: &Desktop, _| ui::theme(&state.config))
    .style(|_: &Desktop, theme: &iced::Theme| iced::theme::Style {
        background_color: iced::Color::TRANSPARENT,
        text_color: theme.palette().text,
    })
    .settings(Settings {
        shell_broadcast: sender,
        default_text_size: text_size,
        layer_settings: LayerShellSettings {
            start_mode: StartMode::Background,
            ..Default::default()
        },
        ..Default::default()
    })
    .run()
}

fn settings(kind: Kind, output: u32, config: &Config) -> NewLayerShellSettings {
    let mut settings = NewLayerShellSettings {
        output_option: OutputOption::GlobalName(output),
        keyboard_interactivity: KeyboardInteractivity::None,
        ..Default::default()
    };
    match kind {
        Kind::Bar => {
            settings.namespace = Some("cabby-bar".into());
            settings.layer = Layer::Top;
            settings.anchor = (if config.bar_position == BarPosition::Top {
                Anchor::Top
            } else {
                Anchor::Bottom
            }) | Anchor::Left
                | Anchor::Right;
            settings.size = LayerSize::fill_width(u32::from(config.bar_height));
            settings.exclusive_zone = Some(i32::from(config.bar_height));
        }
        Kind::Wallpaper => {
            settings.namespace = Some("cabby-wallpaper".into());
            settings.layer = Layer::Background;
            settings.anchor = Anchor::all();
            settings.size = LayerSize::FILL;
            settings.exclusive_zone = Some(-1);
            settings.events_transparent = true;
        }
    }
    settings
}
impl Desktop {
    fn audio_open(&self) -> bool {
        self.panel
            .as_ref()
            .is_some_and(|p| matches!(p.plugin, PanelPlugin::Audio))
    }
    fn bluetooth_open(&self) -> bool {
        self.panel
            .as_ref()
            .is_some_and(|p| matches!(p.plugin, PanelPlugin::Bluetooth))
    }
    fn refresh_labels(&mut self) {
        if let Some(bar) = &mut self.bar {
            bar.audio_label = self.audio.label();
            bar.bluetooth_label = self.bluetooth.label();
        }
    }
    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            if self.bar.is_some() {
                self.config_path
                    .as_deref()
                    .and_then(super::control::directory)
                    .map(super::control::subscription)
                    .map(|s| s.map(Message::Control))
                    .unwrap_or_else(Subscription::none)
            } else {
                Subscription::none()
            },
            if self
                .panel
                .as_ref()
                .is_some_and(|p| matches!(p.plugin, PanelPlugin::Clipboard(_)))
            {
                self.config_path
                    .as_deref()
                    .and_then(super::control::directory)
                    .map(|root| {
                        Subscription::run_with(root.join("clipboard.json"), |path| {
                            crate::runtime_status::updates::<clipboard::Revision>(Some(
                                path.clone(),
                            ))
                        })
                        .map(|_| Message::ClipboardRevision)
                    })
                    .unwrap_or_else(Subscription::none)
            } else {
                Subscription::none()
            },
            self.config_path
                .as_ref()
                .map(|p| {
                    super::live_config::subscription(p.clone(), self.wallpaper.is_some())
                        .map(Message::Config)
                })
                .unwrap_or_else(Subscription::none),
            self.shell.listen().map(Message::Shell),
            if self.bar.is_some()
                && (self.config.has_widget(cabby_config::BarWidget::Audio) || self.audio_open())
            {
                self.audio.subscription().map(Message::AudioState)
            } else {
                Subscription::none()
            },
            if self.bar.is_some()
                && (self.config.has_widget(cabby_config::BarWidget::Bluetooth)
                    || self.bluetooth_open())
            {
                self.bluetooth
                    .subscription(self.bluetooth_open(), self.bluetooth_generation)
                    .with(self.bluetooth_generation)
                    .map(|(generation, message)| Message::BluetoothState(generation, message))
            } else {
                Subscription::none()
            },
            iced::event::listen_raw(|event, status, id| {
                if matches!(event, iced::Event::Window(window::Event::Focused)) {
                    return Some(Message::Focus(id, true));
                }
                if matches!(event, iced::Event::Window(window::Event::Unfocused)) {
                    return Some(Message::Focus(id, false));
                }
                let pointer = match &event {
                    iced::Event::Mouse(iced::mouse::Event::CursorMoved { position }) => {
                        Some(screenshot::Message::Move(*position))
                    }
                    iced::Event::Mouse(iced::mouse::Event::ButtonPressed(
                        iced::mouse::Button::Left,
                    )) if status == iced::event::Status::Ignored => {
                        Some(screenshot::Message::Press)
                    }
                    iced::Event::Mouse(iced::mouse::Event::ButtonReleased(
                        iced::mouse::Button::Left,
                    )) if status == iced::event::Status::Ignored => {
                        Some(screenshot::Message::Release)
                    }
                    _ => None,
                };
                if let Some(pointer) = pointer {
                    return Some(Message::Screenshot(id, pointer));
                }
                if let iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                    key: iced::keyboard::Key::Named(key),
                    ..
                }) = &event
                {
                    let command = match key {
                        iced::keyboard::key::Named::ArrowUp => Some(clipboard::Message::Move(-1)),
                        iced::keyboard::key::Named::ArrowDown => Some(clipboard::Message::Move(1)),
                        _ => None,
                    };
                    if let Some(command) = command {
                        return Some(Message::Clipboard(id, command));
                    }
                }
                matches!(
                    event,
                    iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                        key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
                        ..
                    })
                )
                .then_some(Message::Escape(id))
            }),
            self.bar
                .as_ref()
                .map(|bar| bar.subscription().map(Message::Bar))
                .unwrap_or_else(Subscription::none),
            // Rebuild static wallpaper placement after logical size/scale changes.
            iced::window::resize_events().map(|_| Message::Resized),
        ])
    }
    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Control(super::control::Command::ScreenshotCancel) => {
                if self
                    .panel
                    .as_ref()
                    .is_some_and(|p| matches!(p.plugin, PanelPlugin::Screenshot(_)))
                {
                    return self.close_panel();
                }
            }
            Message::Control(super::control::Command::Screenshot(monitor)) => {
                return self.open_screenshot(monitor, None);
            }
            Message::Screenshot(id, message) => {
                if let Some(p) = &mut self.panel
                    && p.id == id
                    && let PanelPlugin::Screenshot(picker) = &mut p.plugin
                {
                    let fullscreen = picker.fullscreen();
                    let action = picker.update(message, &self.config);
                    let map = !picker.mapped() && !picker.capturing();
                    let changed = fullscreen != picker.fullscreen();
                    if map {
                        picker.mark_mapped();
                    }
                    return match action {
                        screenshot::Action::Close => self.close_panel(),
                        screenshot::Action::Update(task) => Task::batch([
                            if map {
                                self.create_panel("cabby-screenshot")
                            } else if changed {
                                self.layout_panel()
                            } else {
                                Task::none()
                            },
                            task.map(move |m| Message::Screenshot(id, m)),
                        ]),
                    };
                }
            }
            Message::Control(super::control::Command::Clipboard) => {
                if let Some(name) = self.bar.as_ref().and_then(|b| b.focused_output())
                    && let Some((output, _)) = self
                        .surfaces
                        .outputs
                        .iter()
                        .find(|(_, n)| n.as_str() == name)
                    && let Some(size) = self.output_sizes.get(output)
                {
                    self.origin = Some(Origin {
                        output: *output,
                        x: size.0 as f32 / 2.0,
                    });
                }
                return self.open_clipboard();
            }
            Message::ClipboardRevision => {
                if let Some(p) = &self.panel {
                    return self.update(Message::Clipboard(p.id, clipboard::Message::Refresh));
                }
            }
            Message::Clipboard(id, message) => {
                if let Some(p) = &mut self.panel
                    && p.id == id
                    && let PanelPlugin::Clipboard(picker) = &mut p.plugin
                {
                    return match picker.update(message) {
                        clipboard::Action::Close => self.close_panel(),
                        clipboard::Action::Update(task) => {
                            task.map(move |m| Message::Clipboard(id, m))
                        }
                    };
                }
            }
            Message::Pointer(id, point) => {
                self.pointers.insert(id, point.x);
            }
            Message::FromBar(id, message) => {
                if let Some((output, Kind::Bar)) = self.surfaces.windows.get(&id) {
                    self.origin = Some(Origin {
                        output: *output,
                        x: self.pointers.get(&id).copied().unwrap_or(0.0),
                    });
                }
                return self.update(Message::Bar(message));
            }
            Message::Outside(id) if self.panel.as_ref().is_some_and(|p| p.shield == id) => {
                return self.close_panel();
            }
            Message::Focus(id, focused) if self.panel.as_ref().is_some_and(|p| p.id == id) => {
                let panel = self.panel.as_mut().unwrap();
                if !focused && panel.focused {
                    return self.close_panel();
                }
                let acquired = focused && !panel.focused;
                panel.focused = focused;
                if acquired && matches!(panel.plugin, PanelPlugin::Clipboard(_)) {
                    return iced::widget::operation::focus("clipboard-query");
                }
                if acquired && self.panel_probe {
                    return Task::perform(
                        async move {
                            tokio::time::sleep(std::time::Duration::from_secs(8)).await;
                            id
                        },
                        Message::ProbeClose,
                    );
                }
            }
            Message::Config(update) if update.config.renderer == self.config.renderer => {
                let changes = cabby_config::changes::Changes::between(&self.config, &update.config);
                if let Some(bar) = &mut self.bar {
                    bar.configure(update.config.clone());
                }
                if self.wallpaper.is_some() {
                    self.wallpaper = Some(wallpaper::Wallpaper::from_asset(
                        update.config.clone(),
                        update.asset.as_deref(),
                    ));
                }
                self.config = update.config;
                let panel_task = self.layout_panel();
                if changes.bar_geometry {
                    return Task::batch([
                        panel_task,
                        Task::batch(
                            self.surfaces
                                .windows
                                .iter()
                                .filter(|(_, (_, kind))| *kind == Kind::Bar)
                                .flat_map(|(id, (output, kind))| {
                                    let settings = settings(*kind, *output, &self.config);
                                    [
                                        Task::done(Message::LayoutChange {
                                            id: *id,
                                            anchor: settings.anchor,
                                            size: settings.size,
                                        }),
                                        Task::done(Message::ExclusiveZoneChange {
                                            id: *id,
                                            zone_size: i32::from(self.config.bar_height),
                                        }),
                                    ]
                                }),
                        ),
                    ]);
                }
                return panel_task;
            }
            Message::Tray(id, message)
                if self
                    .panel
                    .as_ref()
                    .is_some_and(|p| p.id == id && matches!(p.plugin, PanelPlugin::Tray)) =>
            {
                return self.update(Message::Bar(bar::Message::Tray(message)));
            }
            Message::Bar(bar::Message::Screenshot) => {
                return self.open_screenshot(false, self.origin.map(|o| o.output));
            }
            Message::Bar(bar::Message::Clipboard) => return self.open_clipboard(),
            Message::Bar(bar::Message::Audio) => return self.toggle_panel(false),
            Message::Bar(bar::Message::Bluetooth) => return self.toggle_panel(true),
            Message::AudioState(message) => {
                self.audio.update(message);
                self.refresh_labels();
            }
            Message::BluetoothState(generation, message)
                if generation == self.bluetooth_generation =>
            {
                if let Some(panel) = &self.panel
                    && self.bluetooth_open()
                {
                    return self.update(Message::Bluetooth(panel.id, message));
                }
                self.bluetooth.update(message);
                self.refresh_labels();
            }
            Message::Audio(id, message) => {
                if self.panel.as_ref().is_some_and(|p| p.id == id)
                    && self.audio_open()
                    && self.audio.update(message)
                {
                    return self.close_panel();
                }
                self.refresh_labels();
            }
            Message::Bluetooth(id, message) => {
                let mut probe_action = None;
                if self.panel.as_ref().is_some_and(|panel| panel.id == id)
                    && let Some(stage) = &mut self.bluetooth_probe_stage
                    && let bluetooth::Message::State(state) = &message
                {
                    if *stage == 0 && state.available && !state.adapters.is_empty() {
                        *stage = 1;
                        probe_action = Some(cabby_bluetooth::Command::Scan(
                            state.adapters[0].path.clone(),
                        ));
                    } else if *stage == 1
                        && state.scanning.is_some()
                        && !state.busy
                        && !state.devices.is_empty()
                    {
                        *stage = 2;
                        probe_action = Some(cabby_bluetooth::Command::Pair(
                            state.devices[0].path.clone(),
                        ));
                    } else if *stage == 2 && state.prompt.is_some() {
                        *stage = 3;
                        println!("bluetooth-probe-prompt");
                    }
                }
                if self.panel.as_ref().is_some_and(|p| p.id == id)
                    && self.bluetooth_open()
                    && self.bluetooth.update(message)
                {
                    return self.close_panel();
                }
                self.refresh_labels();
                if let Some(command) = probe_action {
                    return Task::done(Message::Bluetooth(id, bluetooth::Message::Action(command)));
                }
            }
            Message::Escape(id) | Message::ProbeClose(id)
                if self.panel.as_ref().is_some_and(|panel| panel.id == id) =>
            {
                return self.close_panel();
            }
            Message::Shell(ShellEvent::WindowOutputChanged { window, output }) => {
                if let Some(panel) = &mut self.panel
                    && panel.id == window
                {
                    panel.output = output.map(|o| o.id);
                }
            }
            Message::Bar(bar::Message::Tray(message)) => {
                let closing = matches!(message, tray::Message::Close);
                let icons = if let Some(bar) = &mut self.bar {
                    bar.tray
                        .update(message)
                        .map(|m| Message::Bar(bar::Message::Tray(m)))
                } else {
                    Task::none()
                };
                if let Some(stage) = &mut self.tray_probe_stage
                    && let Some(bar) = &self.bar
                {
                    if *stage == 0
                        && let Some(item) = bar.tray.state.items.first()
                    {
                        *stage = 1;
                        return Task::batch([
                            icons,
                            Task::done(Message::Bar(bar::Message::Tray(tray::Message::Action(
                                cabby_tray::Command::Menu(item.token.clone(), 0),
                            )))),
                        ]);
                    }
                    if *stage == 1 && bar.tray.state.menu.is_some() {
                        *stage = 2;
                        println!("tray-probe-menu");
                    }
                }
                let menu = !closing
                    && self
                        .bar
                        .as_ref()
                        .is_some_and(|b| b.tray.state.menu.is_some());
                let open = self
                    .panel
                    .as_ref()
                    .is_some_and(|p| matches!(p.plugin, PanelPlugin::Tray));
                if menu && !open {
                    return Task::batch([icons, self.open_tray_menu()]);
                }
                if !menu && open {
                    return Task::batch([icons, self.close_panel()]);
                }
                return icons;
            }
            Message::Bar(message) => {
                if let Some(bar) = &mut self.bar {
                    return bar.update(message).map(Message::Bar);
                }
            }
            Message::Shell(ShellEvent::OutputAdded(info) | ShellEvent::OutputUpdated(info)) => {
                let capture_changed = self.output_sizes.contains_key(&info.id)
                    && self.panel.as_ref().is_some_and(|p| {
                        p.output == Some(info.id) && matches!(p.plugin, PanelPlugin::Screenshot(_))
                    });
                let size = info.logical_size.unwrap_or((800, 600));
                self.output_sizes
                    .insert(info.id, (size.0.max(1) as u32, size.1.max(1) as u32));
                let mut kinds = Vec::new();
                if self.wallpaper.is_some() {
                    kinds.push(Kind::Wallpaper);
                }
                if self.bar.is_some() {
                    kinds.push(Kind::Bar);
                }
                let added = self
                    .surfaces
                    .add(info.id, info.name.unwrap_or_default(), &kinds);

                let open = if capture_changed {
                    self.close_panel()
                } else if self.open_audio_on_output || self.open_bluetooth_on_output {
                    let bluetooth = self.open_bluetooth_on_output;
                    self.open_audio_on_output = false;
                    self.open_bluetooth_on_output = false;
                    self.toggle_panel(bluetooth)
                } else {
                    self.layout_panel()
                };
                return Task::batch([
                    open,
                    Task::batch(added.into_iter().map(|(id, kind)| {
                        Task::done(Message::NewLayerShell {
                            id,
                            settings: settings(kind, info.id, &self.config),
                        })
                    })),
                ]);
            }
            Message::Shell(ShellEvent::OutputRemoved(info)) => {
                self.output_sizes.remove(&info.id);
                let close = if self.panel.as_ref().is_some_and(|panel| {
                    panel.output == Some(info.id) || self.output_sizes.is_empty()
                }) {
                    self.close_panel()
                } else {
                    Task::none()
                };
                return Task::batch([
                    close,
                    Task::batch(
                        self.surfaces
                            .remove_output(info.id)
                            .into_iter()
                            .map(|id| Task::done(Message::RemoveWindow(id))),
                    ),
                ]);
            }
            Message::Shell(ShellEvent::Closed(id)) => {
                self.surfaces.windows.remove(&id);
                self.pointers.remove(&id);
                if self
                    .panel
                    .as_ref()
                    .is_some_and(|panel| panel.id == id || panel.shield == id)
                {
                    return self.close_panel();
                }
            }
            _ => (),
        }
        Task::none()
    }
    fn close_panel(&mut self) -> Task<Message> {
        self.audio.close();
        if self.bluetooth_open() {
            self.bluetooth_generation = self.bluetooth_generation.wrapping_add(1);
            self.bluetooth.reset_interaction();
        }
        if self
            .panel
            .as_ref()
            .is_some_and(|p| matches!(p.plugin, PanelPlugin::Tray))
            && let Some(bar) = &mut self.bar
        {
            let _ = bar.tray.update(tray::Message::Close);
        }
        self.panel
            .take()
            .map(|panel| {
                if matches!(&panel.plugin,PanelPlugin::Screenshot(picker) if !picker.mapped()) {
                    return Task::none();
                }
                Task::batch([
                    Task::done(Message::RemoveWindow(panel.id)),
                    Task::done(Message::RemoveWindow(panel.shield)),
                ])
            })
            .unwrap_or_else(Task::none)
    }
    fn selected_origin(&self) -> Option<Origin> {
        self.origin
            .filter(|o| self.output_sizes.contains_key(&o.output))
            .or_else(|| {
                self.output_sizes
                    .first_key_value()
                    .map(|(output, size)| Origin {
                        output: *output,
                        x: size.0 as f32,
                    })
            })
    }
    fn layout_panel(&self) -> Task<Message> {
        let Some(p) = &self.panel else {
            return Task::none();
        };
        if matches!(&p.plugin,PanelPlugin::Screenshot(picker) if !picker.mapped()) {
            return Task::none();
        }
        let Some(size) = self.output_sizes.get(&p.origin.output) else {
            return Task::none();
        };
        let g = self.panel_geometry(p, *size);
        Task::batch([
            Task::done(Message::LayoutChange {
                id: p.id,
                anchor: g.anchor,
                size: LayerSize::px(g.width, g.height),
            }),
            Task::done(Message::MarginChange {
                id: p.id,
                margin: g.margin,
            }),
            Task::done(Message::LayoutChange {
                id: p.shield,
                anchor: g.shield_anchor,
                size: g.shield_size,
            }),
        ])
    }
    fn create_panel(&self, namespace: &str) -> Task<Message> {
        let p = self.panel.as_ref().unwrap();
        let g = self.panel_geometry(p, self.output_sizes[&p.origin.output]);
        Task::done(Message::NewLayerShell {
            id: p.shield,
            settings: NewLayerShellSettings {
                namespace: Some("cabby-panel-shield".into()),
                output_option: OutputOption::GlobalName(p.origin.output),
                layer: Layer::Overlay,
                keyboard_interactivity: KeyboardInteractivity::None,
                anchor: g.shield_anchor,
                size: g.shield_size,
                exclusive_zone: Some(-1),
                ..Default::default()
            },
        })
        .chain(Task::done(Message::NewLayerShell {
            id: p.id,
            settings: NewLayerShellSettings {
                namespace: Some(namespace.into()),
                output_option: OutputOption::GlobalName(p.origin.output),
                layer: Layer::Overlay,
                keyboard_interactivity: KeyboardInteractivity::Exclusive,
                anchor: g.anchor,
                size: LayerSize::px(g.width, g.height),
                margin: Some(g.margin),
                exclusive_zone: Some(-1),
                ..Default::default()
            },
        }))
    }
    fn panel_geometry(&self, p: &DevicePanel, size: (u32, u32)) -> panel::Geometry {
        let mut g = panel::geometry(&self.config, size, p.origin.x);
        if let PanelPlugin::Screenshot(picker) = &p.plugin {
            g.anchor = Anchor::Top | Anchor::Left;
            g.shield_anchor = Anchor::all();
            g.shield_size = LayerSize::FILL;
            if picker.fullscreen() {
                // The selector covers the output itself. Keep its older shield
                // tiny until preview, preserving stacking without an opaque flash.
                g.shield_anchor = Anchor::Top | Anchor::Left;
                g.shield_size = LayerSize::px(1, 1);
                g.width = size.0;
                g.height = size.1;
                g.margin = (0, 0, 0, 0);
            } else {
                g.margin = (
                    (size.1.saturating_sub(g.height) / 2) as i32,
                    0,
                    0,
                    (size.0.saturating_sub(g.width) / 2) as i32,
                );
            }
        }
        g
    }
    fn open_screenshot(&mut self, monitor: bool, requested: Option<u32>) -> Task<Message> {
        if let Some(p) = &self.panel
            && matches!(p.plugin, PanelPlugin::Screenshot(_))
        {
            return self.update(Message::Screenshot(
                p.id,
                if monitor {
                    screenshot::Message::Monitor
                } else {
                    screenshot::Message::Reselect
                },
            ));
        }
        let previous = self.close_panel();
        let output = requested
            .filter(|o| self.output_sizes.contains_key(o))
            .or_else(|| {
                self.bar
                    .as_ref()
                    .and_then(|b| b.focused_output())
                    .and_then(|name| {
                        self.surfaces
                            .outputs
                            .iter()
                            .find(|(_, n)| n.as_str() == name)
                            .map(|(o, _)| *o)
                    })
                    .or_else(|| self.selected_origin().map(|o| o.output))
            });
        let Some(output) = output else {
            return previous;
        };
        let Some(root) = self
            .config_path
            .as_deref()
            .and_then(super::control::directory)
        else {
            return previous;
        };
        let logical = self.output_sizes[&output];
        let name = self.surfaces.outputs[&output].clone();
        let id = window::Id::unique();
        let picker = screenshot::Picker::new(root, name, logical, monitor);
        self.panel = Some(DevicePanel {
            id,
            output: Some(output),
            origin: Origin {
                output,
                x: logical.0 as f32 / 2.,
            },
            shield: window::Id::unique(),
            focused: false,
            plugin: PanelPlugin::Screenshot(Box::new(picker)),
        });
        let begin = self.update(Message::Screenshot(id, screenshot::Message::Begin));
        previous.chain(begin)
    }
    fn open_clipboard(&mut self) -> Task<Message> {
        let previous = self.close_panel();
        let Some(origin) = self.selected_origin() else {
            return previous;
        };
        let Some(root) = self
            .config_path
            .as_deref()
            .and_then(super::control::directory)
        else {
            return previous;
        };
        let (picker, task) = clipboard::Picker::new(root);
        let id = window::Id::unique();
        self.panel = Some(DevicePanel {
            id,
            output: Some(origin.output),
            origin,
            shield: window::Id::unique(),
            focused: false,
            plugin: PanelPlugin::Clipboard(picker),
        });
        Task::batch([
            previous,
            self.create_panel("cabby-clipboard"),
            task.map(move |m| Message::Clipboard(id, m)),
            iced::widget::operation::focus("clipboard-query"),
        ])
    }
    fn open_tray_menu(&mut self) -> Task<Message> {
        let previous = self.close_panel();
        let id = window::Id::unique();
        let Some(origin) = self.selected_origin() else {
            return previous;
        };
        self.panel = Some(DevicePanel {
            id,
            output: Some(origin.output),
            origin,
            shield: window::Id::unique(),
            focused: false,
            plugin: PanelPlugin::Tray,
        });
        Task::batch([previous, self.create_panel("cabby-tray-menu")])
    }
    fn toggle_panel(&mut self, bluetooth: bool) -> Task<Message> {
        if let Some(bar) = &mut self.bar {
            let _ = bar.tray.update(tray::Message::Close);
        }
        if self.panel.as_ref().is_some_and(|p| {
            if bluetooth {
                matches!(p.plugin, PanelPlugin::Bluetooth)
            } else {
                matches!(p.plugin, PanelPlugin::Audio)
            }
        }) {
            return self.close_panel();
        }
        let previous = self.close_panel();
        let id = window::Id::unique();
        let Some(origin) = self.selected_origin() else {
            return previous;
        };
        if bluetooth {
            self.bluetooth_generation = self.bluetooth_generation.wrapping_add(1);
            self.bluetooth.reset_interaction();
        }
        self.panel = Some(DevicePanel {
            id,
            output: Some(origin.output),
            origin,
            shield: window::Id::unique(),
            focused: false,
            plugin: if bluetooth {
                PanelPlugin::Bluetooth
            } else {
                PanelPlugin::Audio
            },
        });
        Task::batch([
            previous,
            self.create_panel(if bluetooth {
                "cabby-bluetooth"
            } else {
                "cabby-audio"
            }),
        ])
    }
    fn view(&self, id: window::Id) -> Element<'_, Message> {
        if self.panel.as_ref().is_some_and(|p| p.shield == id) {
            return mouse_area(
                Space::new()
                    .width(iced::Length::Fill)
                    .height(iced::Length::Fill),
            )
            .on_press(Message::Outside(id))
            .on_right_press(Message::Outside(id))
            .on_middle_press(Message::Outside(id))
            .into();
        }
        if let Some(panel) = &self.panel
            && panel.id == id
        {
            return match &panel.plugin {
                PanelPlugin::Screenshot(picker) => picker
                    .view(&self.config)
                    .map(move |m| Message::Screenshot(id, m)),
                PanelPlugin::Clipboard(picker) => picker
                    .view(&self.config)
                    .map(move |m| Message::Clipboard(id, m)),
                PanelPlugin::Tray => self
                    .bar
                    .as_ref()
                    .expect("tray belongs to bar")
                    .tray
                    .menu_view(&self.config)
                    .map(move |m| Message::Tray(id, m)),
                PanelPlugin::Audio => self
                    .audio
                    .view(&self.config)
                    .map(move |message| Message::Audio(id, message)),
                PanelPlugin::Bluetooth => self
                    .bluetooth
                    .view(&self.config)
                    .map(move |message| Message::Bluetooth(id, message)),
            };
        }

        if let Some((output, kind)) = self.surfaces.windows.get(&id) {
            match kind {
                Kind::Bar => {
                    if let Some(bar) = &self.bar {
                        return mouse_area(
                            bar.view(
                                self.surfaces
                                    .outputs
                                    .get(output)
                                    .map(String::as_str)
                                    .unwrap_or(""),
                            )
                            .map(move |message| Message::FromBar(id, message)),
                        )
                        .on_move(move |point| Message::Pointer(id, point))
                        .into();
                    }
                }
                Kind::Wallpaper => {
                    if let Some(wallpaper) = &self.wallpaper {
                        return wallpaper.view();
                    }
                }
            }
        }
        iced::widget::Space::new().into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn audio_panel_toggle_and_stale_callbacks_preserve_desktop_plugins() {
        let mut state = Desktop {
            config: Config::default(),
            config_path: None,
            shell: shell::channel().1,
            bar: Some(bar::Bar::new(Config::default())),
            wallpaper: None,
            surfaces: Surfaces::default(),
            panel: None,
            origin: None,
            pointers: BTreeMap::new(),
            audio: audio::Audio::new(cabby_audio::Client::default()),
            open_audio_on_output: false,
            panel_probe: false,
            bluetooth: bluetooth::Bluetooth::new(cabby_bluetooth::Endpoint::System),
            bluetooth_generation: 0,
            open_bluetooth_on_output: false,
            bluetooth_probe_stage: None,
            tray_probe_stage: None,
            output_sizes: BTreeMap::from([(1, (320, 400))]),
        };
        let _ = state.toggle_panel(false);
        let first = state.panel.as_ref().unwrap().id;
        let config = Config {
            bar_height: 48,
            background: "#112233".into(),
            ..state.config.clone()
        };
        let _ = state.update(Message::Config(super::super::live_config::Update {
            config,
            asset: None,
        }));
        assert_eq!(state.panel.as_ref().unwrap().id, first);
        assert_eq!(state.config.bar_height, 48);
        let _ = state.toggle_panel(false);
        assert!(state.panel.is_none());
        let _ = state.toggle_panel(false);
        let second = state.panel.as_ref().unwrap().id;
        assert_ne!(first, second);
        let _ = state.update(Message::Audio(first, audio::Message::Close));
        let _ = state.update(Message::Shell(ShellEvent::Closed(first)));
        assert_eq!(state.panel.as_ref().unwrap().id, second);
        let g = panel::geometry(&state.config, (320, 400), 320.0);
        assert!(g.width <= 304 && g.height + g.margin.0 as u32 + 8 <= 400);
        let old_shield = state.panel.as_ref().unwrap().shield;
        let _ = state.update(Message::Outside(old_shield));
        assert!(state.panel.is_none());
        let _ = state.toggle_panel(false);
        let second = state.panel.as_ref().unwrap().id;
        let _ = state.update(Message::Outside(old_shield));
        assert_eq!(state.panel.as_ref().unwrap().id, second);
        let _ = state.update(Message::Escape(second));
        assert!(state.panel.is_none());
        assert!(state.bar.is_some());
        let _ = state.toggle_panel(false);
        let old = state.panel.as_ref().unwrap().id;
        let _ = state.toggle_panel(true);
        let new = state.panel.as_ref().unwrap().id;
        let generation = state.bluetooth_generation;
        let _ = state.update(Message::BluetoothState(
            generation.wrapping_sub(1),
            bluetooth::Message::Close,
        ));
        assert_eq!(state.panel.as_ref().unwrap().id, new);
        assert!(matches!(
            state.panel.as_ref().unwrap().plugin,
            PanelPlugin::Bluetooth
        ));
        let _ = state.update(Message::Audio(old, audio::Message::Close));
        assert_eq!(state.panel.as_ref().unwrap().id, new);
        let _ = state.update(Message::Bluetooth(new, bluetooth::Message::Close));
        assert!(state.panel.is_none());
        let _ = state.open_tray_menu();
        let old_menu = state.panel.as_ref().unwrap().id;
        let _ = state.toggle_panel(false);
        assert!(matches!(
            state.panel.as_ref().unwrap().plugin,
            PanelPlugin::Audio
        ));
        let _ = state.open_tray_menu();
        let current_menu = state.panel.as_ref().unwrap().id;
        let _ = state.update(Message::Tray(old_menu, tray::Message::Close));
        assert_eq!(state.panel.as_ref().unwrap().id, current_menu);
        let _ = state.update(Message::Focus(current_menu, false));
        assert!(state.panel.is_some());
        let _ = state.update(Message::Focus(current_menu, true));
        let _ = state.update(Message::Focus(old_menu, false));
        assert!(state.panel.is_some());
        let _ = state.update(Message::Focus(current_menu, false));
        assert!(state.panel.is_none());
        state.output_sizes.insert(2, (1920, 1080));
        state.origin = Some(Origin {
            output: 2,
            x: 500.0,
        });
        let _ = state.toggle_panel(false);
        assert_eq!(state.panel.as_ref().unwrap().output, Some(2));
    }
    #[test]
    fn output_updates_preserve_windows_and_removal_is_scoped() {
        let mut surfaces = Surfaces::default();
        let first = surfaces.add(10, "left".into(), &[Kind::Bar, Kind::Wallpaper]);
        let second = surfaces.add(20, "right".into(), &[Kind::Bar, Kind::Wallpaper]);
        assert_eq!(first.len(), 2);
        assert_eq!(second.len(), 2);
        assert!(
            surfaces
                .add(10, "renamed".into(), &[Kind::Bar, Kind::Wallpaper])
                .is_empty()
        );
        assert_eq!(surfaces.outputs[&10], "renamed");
        let removed = surfaces.remove_output(10);
        assert_eq!(removed.len(), 2);
        assert!(first.iter().all(|(id, _)| removed.contains(id)));
        assert!(
            second
                .iter()
                .all(|(id, _)| surfaces.windows.contains_key(id))
        );
        // Late close callbacks from a removed output cannot affect a replacement.
        let replacement = surfaces.add(10, "returned".into(), &[Kind::Bar, Kind::Wallpaper]);
        for id in removed {
            surfaces.windows.remove(&id);
        }
        assert!(
            replacement
                .iter()
                .all(|(id, _)| surfaces.windows.contains_key(id))
        );
    }
    #[test]
    fn surface_roles_keep_input_and_exclusive_zone_policy() {
        let config = Config {
            bar_position: BarPosition::Bottom,
            bar_height: 40,
            ..Config::default()
        };
        let bar = settings(Kind::Bar, 42, &config);
        let wallpaper = settings(Kind::Wallpaper, 42, &config);
        assert_eq!(bar.exclusive_zone, Some(40));
        assert!(bar.anchor.contains(Anchor::Bottom));
        assert_eq!(bar.keyboard_interactivity, KeyboardInteractivity::None);
        assert_eq!(wallpaper.exclusive_zone, Some(-1));
        assert_eq!(wallpaper.layer, Layer::Background);
        assert!(wallpaper.events_transparent);
        assert_eq!(bar.output_option, OutputOption::GlobalName(42));
        assert_eq!(wallpaper.output_option, bar.output_option);
    }
}
