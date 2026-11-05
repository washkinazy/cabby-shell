//! Output/surface ownership for passive desktop plugins and on-demand device panels.
use super::Context;
use crate::{
    plugins::{audio, bar, bluetooth, tray, wallpaper},
    ui,
};
use cabby_config::{BarPosition, Config};
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
    Tray,
    Audio(audio::Audio),
    Bluetooth(bluetooth::Bluetooth),
}
struct DevicePanel {
    id: window::Id,
    output: Option<u32>,
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
    bluetooth_endpoint: cabby_bluetooth::Endpoint,
    open_bluetooth_on_output: bool,
    bluetooth_probe_stage: Option<u8>,
    tray_probe_stage: Option<u8>,
    audio_client: cabby_audio::Client,
    open_audio_on_output: bool,
    output_sizes: BTreeMap<u32, (u32, u32)>,
}
#[to_exwlshell_message]
#[derive(Clone, Debug)]
enum Message {
    Bar(bar::Message),
    Audio(window::Id, audio::Message),
    Bluetooth(window::Id, bluetooth::Message),
    Tray(window::Id, tray::Message),
    Escape(window::Id),
    ProbeClose(window::Id),
    Shell(ShellEvent),
    Resized,
    Config(super::live_config::Update),
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
            audio_client: fixture.clone().unwrap_or_default(),
            open_audio_on_output: fixture.is_some(),
            bluetooth_endpoint: bluetooth_fixture.clone().unwrap_or_default(),
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

fn panel_geometry(config: &Config, outputs: &BTreeMap<u32, (u32, u32)>) -> (u32, u32, i32) {
    let width = outputs.values().map(|s| s.0).min().unwrap_or(800);
    let height = outputs.values().map(|s| s.1).min().unwrap_or(600);
    let top = if config.bar_position == BarPosition::Top {
        u32::from(config.bar_height) + 8
    } else {
        8
    };
    (
        width.saturating_sub(16).clamp(1, 440),
        height
            .saturating_sub(u32::from(config.bar_height) + 16)
            .clamp(1, 500),
        top as i32,
    )
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
    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            self.config_path
                .as_ref()
                .map(|p| {
                    super::live_config::subscription(p.clone(), self.wallpaper.is_some())
                        .map(Message::Config)
                })
                .unwrap_or_else(Subscription::none),
            self.shell.listen().map(Message::Shell),
            self.panel
                .as_ref()
                .map(|panel| match &panel.plugin {
                    PanelPlugin::Tray => Subscription::none(),
                    PanelPlugin::Audio(plugin) => plugin
                        .subscription()
                        .with(panel.id)
                        .map(|(id, message)| Message::Audio(id, message)),
                    PanelPlugin::Bluetooth(plugin) => plugin
                        .subscription()
                        .with(panel.id)
                        .map(|(id, message)| Message::Bluetooth(id, message)),
                })
                .unwrap_or_else(Subscription::none),
            iced::event::listen_raw(|event, _, id| {
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
                if changes.bar_geometry {
                    return Task::batch(
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
                    );
                }
            }
            Message::Tray(id, message)
                if self
                    .panel
                    .as_ref()
                    .is_some_and(|p| p.id == id && matches!(p.plugin, PanelPlugin::Tray)) =>
            {
                return self.update(Message::Bar(bar::Message::Tray(message)));
            }
            Message::Bar(bar::Message::Audio) => return self.toggle_panel(false),
            Message::Bar(bar::Message::Bluetooth) => return self.toggle_panel(true),
            Message::Audio(id, message) => {
                if let Some(panel) = &mut self.panel
                    && panel.id == id
                    && let PanelPlugin::Audio(plugin) = &mut panel.plugin
                    && plugin.update(message)
                {
                    return self.close_panel();
                }
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
                if let Some(panel) = &mut self.panel
                    && panel.id == id
                    && let PanelPlugin::Bluetooth(plugin) = &mut panel.plugin
                    && plugin.update(message)
                {
                    return self.close_panel();
                }
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
                if let Some(bar) = &mut self.bar {
                    bar.tray.update(message);
                }
                if let Some(stage) = &mut self.tray_probe_stage
                    && let Some(bar) = &self.bar
                {
                    if *stage == 0
                        && let Some(item) = bar.tray.state.items.first()
                    {
                        *stage = 1;
                        return Task::done(Message::Bar(bar::Message::Tray(
                            tray::Message::Action(cabby_tray::Command::Menu(item.token.clone(), 0)),
                        )));
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
                    return self.open_tray_menu();
                }
                if !menu && open {
                    return self.close_panel();
                }
            }
            Message::Bar(message) => {
                if let Some(bar) = &mut self.bar {
                    return bar.update(message).map(Message::Bar);
                }
            }
            Message::Shell(ShellEvent::OutputAdded(info) | ShellEvent::OutputUpdated(info)) => {
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
                let open = if self.open_audio_on_output || self.open_bluetooth_on_output {
                    let bluetooth = self.open_bluetooth_on_output;
                    self.open_audio_on_output = false;
                    self.open_bluetooth_on_output = false;
                    let open = self.toggle_panel(bluetooth);
                    let id = self.panel.as_ref().expect("probe panel opened").id;
                    Task::batch([
                        open,
                        Task::perform(
                            async move {
                                tokio::time::sleep(std::time::Duration::from_secs(8)).await;
                                id
                            },
                            Message::ProbeClose,
                        ),
                    ])
                } else if let Some(panel) = &self.panel {
                    let (width, height, _) = panel_geometry(&self.config, &self.output_sizes);
                    Task::done(Message::LayoutChange {
                        id: panel.id,
                        anchor: Anchor::Top | Anchor::Right,
                        size: LayerSize::px(width, height),
                    })
                } else {
                    Task::none()
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
                if self.panel.as_ref().is_some_and(|panel| panel.id == id) {
                    self.panel = None;
                }
            }
            _ => (),
        }
        Task::none()
    }
    fn close_panel(&mut self) -> Task<Message> {
        if self
            .panel
            .as_ref()
            .is_some_and(|p| matches!(p.plugin, PanelPlugin::Tray))
            && let Some(bar) = &mut self.bar
        {
            bar.tray.update(tray::Message::Close);
        }
        self.panel
            .take()
            .map(|panel| Task::done(Message::RemoveWindow(panel.id)))
            .unwrap_or_else(Task::none)
    }
    fn open_tray_menu(&mut self) -> Task<Message> {
        let previous = self.close_panel();
        let id = window::Id::unique();
        let (width, height, top) = panel_geometry(&self.config, &self.output_sizes);
        self.panel = Some(DevicePanel {
            id,
            output: None,
            plugin: PanelPlugin::Tray,
        });
        let probe_close = if self.tray_probe_stage.is_some() {
            Task::perform(
                async move {
                    tokio::time::sleep(std::time::Duration::from_secs(8)).await;
                    id
                },
                Message::ProbeClose,
            )
        } else {
            Task::none()
        };
        Task::batch([
            previous,
            probe_close,
            Task::done(Message::NewLayerShell {
                id,
                settings: NewLayerShellSettings {
                    namespace: Some("cabby-tray-menu".into()),
                    output_option: OutputOption::Active,
                    keyboard_interactivity: KeyboardInteractivity::Exclusive,
                    layer: Layer::Overlay,
                    anchor: Anchor::Top | Anchor::Right,
                    size: LayerSize::px(width, height),
                    margin: Some((top, 8, 8, 8)),
                    exclusive_zone: Some(-1),
                    ..Default::default()
                },
            }),
        ])
    }
    fn toggle_panel(&mut self, bluetooth: bool) -> Task<Message> {
        if let Some(bar) = &mut self.bar {
            bar.tray.update(tray::Message::Close);
        }
        if self.panel.as_ref().is_some_and(|p| {
            if bluetooth {
                matches!(p.plugin, PanelPlugin::Bluetooth(_))
            } else {
                matches!(p.plugin, PanelPlugin::Audio(_))
            }
        }) {
            return self.close_panel();
        }
        let previous = self.close_panel();
        let id = window::Id::unique();
        let (width, height, top) = panel_geometry(&self.config, &self.output_sizes);
        self.panel = Some(DevicePanel {
            id,
            output: None,
            plugin: if bluetooth {
                PanelPlugin::Bluetooth(bluetooth::Bluetooth::new(self.bluetooth_endpoint.clone()))
            } else {
                PanelPlugin::Audio(audio::Audio::new(self.audio_client.clone()))
            },
        });
        Task::batch([
            previous,
            Task::done(Message::NewLayerShell {
                id,
                settings: NewLayerShellSettings {
                    namespace: Some(
                        if bluetooth {
                            "cabby-bluetooth"
                        } else {
                            "cabby-audio"
                        }
                        .into(),
                    ),
                    output_option: OutputOption::Active,
                    keyboard_interactivity: KeyboardInteractivity::Exclusive,
                    layer: Layer::Overlay,
                    anchor: Anchor::Top | Anchor::Right,
                    size: LayerSize::px(width, height),
                    margin: Some((top, 8, 8, 8)),
                    exclusive_zone: Some(-1),
                    ..Default::default()
                },
            }),
        ])
    }
    fn view(&self, id: window::Id) -> Element<'_, Message> {
        if let Some(panel) = &self.panel
            && panel.id == id
        {
            return match &panel.plugin {
                PanelPlugin::Tray => self
                    .bar
                    .as_ref()
                    .expect("tray belongs to bar")
                    .tray
                    .menu_view(&self.config)
                    .map(move |m| Message::Tray(id, m)),
                PanelPlugin::Audio(plugin) => plugin
                    .view(&self.config)
                    .map(move |message| Message::Audio(id, message)),
                PanelPlugin::Bluetooth(plugin) => plugin
                    .view(&self.config)
                    .map(move |message| Message::Bluetooth(id, message)),
            };
        }

        if let Some((output, kind)) = self.surfaces.windows.get(&id) {
            match kind {
                Kind::Bar => {
                    if let Some(bar) = &self.bar {
                        return bar
                            .view(
                                self.surfaces
                                    .outputs
                                    .get(output)
                                    .map(String::as_str)
                                    .unwrap_or(""),
                            )
                            .map(Message::Bar);
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
            audio_client: cabby_audio::Client::default(),
            open_audio_on_output: false,
            bluetooth_endpoint: cabby_bluetooth::Endpoint::System,
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
        let (w, h, top) = panel_geometry(&state.config, &state.output_sizes);
        assert!(w <= 304 && h + top as u32 + 8 <= 400);
        let _ = state.update(Message::Escape(second));
        assert!(state.panel.is_none());
        assert!(state.bar.is_some());
        let _ = state.toggle_panel(false);
        let old = state.panel.as_ref().unwrap().id;
        let _ = state.toggle_panel(true);
        let new = state.panel.as_ref().unwrap().id;
        assert!(matches!(
            state.panel.as_ref().unwrap().plugin,
            PanelPlugin::Bluetooth(_)
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
            PanelPlugin::Audio(_)
        ));
        let _ = state.open_tray_menu();
        let current_menu = state.panel.as_ref().unwrap().id;
        let _ = state.update(Message::Tray(old_menu, tray::Message::Close));
        assert_eq!(state.panel.as_ref().unwrap().id, current_menu);
        let _ = state.update(Message::Escape(current_menu));
        assert!(state.panel.is_none());
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
