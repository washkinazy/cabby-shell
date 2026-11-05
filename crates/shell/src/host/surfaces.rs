//! Output/surface ownership for passive desktop plugins.
use super::Context;
use crate::{
    plugins::{bar, wallpaper},
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

struct Desktop {
    config: Config,
    shell: ShellReceiver,
    bar: Option<bar::Bar>,
    wallpaper: Option<wallpaper::Wallpaper>,
    surfaces: Surfaces,
}
#[to_exwlshell_message]
#[derive(Clone, Debug)]
enum Message {
    Bar(bar::Message),
    Shell(ShellEvent),
    Resized,
}

pub(super) fn run(
    context: Context,
    with_bar: bool,
    with_wallpaper: bool,
) -> iced_exwlshell::Result {
    let config = context.config;
    let wallpaper = with_wallpaper.then(|| wallpaper::Wallpaper::new(config.clone()));
    let (sender, receiver) = shell::channel();
    let text_size = iced::Pixels(f32::from(config.font_size));
    daemon(
        move || Desktop {
            config: config.clone(),
            shell: receiver.clone(),
            bar: with_bar.then(|| bar::Bar::new(config.clone())),
            wallpaper: wallpaper.clone(),
            surfaces: Surfaces::default(),
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
            self.shell.listen().map(Message::Shell),
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
            Message::Bar(message) => {
                if let Some(bar) = &mut self.bar {
                    return bar.update(message).map(Message::Bar);
                }
            }
            Message::Shell(ShellEvent::OutputAdded(info) | ShellEvent::OutputUpdated(info)) => {
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
                return Task::batch(added.into_iter().map(|(id, kind)| {
                    Task::done(Message::NewLayerShell {
                        id,
                        settings: settings(kind, info.id, &self.config),
                    })
                }));
            }
            Message::Shell(ShellEvent::OutputRemoved(info)) => {
                return Task::batch(
                    self.surfaces
                        .remove_output(info.id)
                        .into_iter()
                        .map(|id| Task::done(Message::RemoveWindow(id))),
                );
            }
            Message::Shell(ShellEvent::Closed(id)) => {
                self.surfaces.windows.remove(&id);
            }
            _ => (),
        }
        Task::none()
    }
    fn view(&self, id: window::Id) -> Element<'_, Message> {
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
