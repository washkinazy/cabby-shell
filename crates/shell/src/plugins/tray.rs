//! Tray UI and owned worker subscription; the host owns the menu surface.
use cabby_tray::{Command, Handle, Service, State, Token};
use iced::futures::SinkExt;
use iced::widget::{
    button, column, container, image, mouse_area, row, rule, scrollable, svg, tooltip,
};
use iced::{Element, Length, Subscription};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Clone, Debug)]
pub(crate) enum Message {
    Connected(Handle),
    State(State),
    Action(Command),
    Close,
}
#[derive(Default)]
pub(crate) struct Tray {
    pub state: State,
    handle: Option<Handle>,
    icons: HashMap<Token, Icon>,
    menu_requested: bool,
}
enum Icon {
    Raster(image::Handle),
    Vector(svg::Handle),
}
fn worker() -> impl iced::futures::Stream<Item = Message> {
    iced::stream::channel(
        1,
        |mut output: iced::futures::channel::mpsc::Sender<Message>| async move {
            loop {
                if let Ok(address) = std::env::var("DBUS_SESSION_BUS_ADDRESS")
                    && let Ok((service, handle)) = Service::connect(&address).await
                {
                    let mut snapshots = handle.snapshots.clone();
                    if output.send(Message::Connected(handle)).await.is_err() {
                        return;
                    }
                    let run = service.run();
                    tokio::pin!(run);
                    loop {
                        let state = snapshots.borrow_and_update().clone();
                        if output.send(Message::State(state)).await.is_err() {
                            return;
                        }
                        tokio::select! { _ = &mut run => break, result = snapshots.changed() => { if result.is_err() { break; } } }
                    }
                }
                if output.send(Message::State(State::default())).await.is_err() {
                    return;
                }
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        },
    )
}
impl Tray {
    pub fn subscription(&self) -> Subscription<Message> {
        Subscription::run(worker)
    }
    pub fn update(&mut self, message: Message) {
        match message {
            Message::Connected(handle) => self.handle = Some(handle),
            Message::State(mut state) => {
                if !self.menu_requested {
                    state.menu = None;
                }
                if !state.available {
                    self.handle = None;
                }
                // Decode/load only on backend changes, never during rendering.
                self.icons
                    .retain(|token, _| state.items.iter().any(|i| &i.token == token));
                for item in &state.items {
                    if self.state.items.iter().any(|old| {
                        old.token == item.token
                            && old.pixmap == item.pixmap
                            && old.icon_name == item.icon_name
                            && old.icon_theme_path == item.icon_theme_path
                    }) {
                        continue;
                    }
                    let icon = if let Some(p) = &item.pixmap {
                        Some(Icon::Raster(image::Handle::from_rgba(
                            p.width,
                            p.height,
                            p.rgba.clone(),
                        )))
                    } else {
                        themed_icon(&item.icon_name, &item.icon_theme_path)
                    };
                    self.icons.remove(&item.token);
                    if let Some(icon) = icon {
                        self.icons.insert(item.token.clone(), icon);
                    }
                }
                self.state = state;
            }
            Message::Action(command) => {
                match &command {
                    Command::Menu(_, _) => self.menu_requested = true,
                    Command::Activate(token) => {
                        self.menu_requested = self
                            .state
                            .items
                            .iter()
                            .any(|i| i.token == *token && i.is_menu)
                    }
                    _ => (),
                }
                if self
                    .handle
                    .as_ref()
                    .is_none_or(|h| h.commands.try_send(command).is_err())
                {
                    self.state.failed = true;
                }
            }
            Message::Close => {
                self.menu_requested = false;
                self.state.menu = None;
                if let Some(h) = &self.handle {
                    let _ = h.commands.try_send(Command::CloseMenu);
                }
            }
        }
    }
    pub fn view(&self, config: &cabby_config::Config) -> Element<'_, Message> {
        let mut icons = row![].spacing(4).align_y(iced::Alignment::Center);
        if !self.state.available {
            return crate::ui::body(config, "Tray unavailable").into();
        }
        for item in self.state.items.iter().filter(|i| !i.passive) {
            let content: Element<'_, Message> = match self.icons.get(&item.token) {
                Some(Icon::Raster(handle)) => image(handle.clone()).width(22).height(22).into(),
                Some(Icon::Vector(handle)) => svg(handle.clone()).width(22).height(22).into(),
                None => {
                    crate::ui::body(config, item.title.chars().take(3).collect::<String>()).into()
                }
            };
            let token = item.token.clone();
            let button = button(content)
                .padding(3)
                .style(if item.attention {
                    button::primary
                } else {
                    button::text
                })
                .on_press(Message::Action(Command::Activate(token.clone())));
            let mouse = mouse_area(button)
                .on_right_press(Message::Action(Command::Menu(token.clone(), 0)))
                .on_middle_press(Message::Action(Command::Secondary(token)));
            icons = icons.push(tooltip(
                mouse,
                crate::ui::body(config, &item.title),
                tooltip::Position::Bottom,
            ));
        }
        if self.state.failed {
            icons = icons.push(crate::ui::body(config, "Tray action failed"));
        }
        icons.into()
    }
    pub fn menu_view(&self, config: &cabby_config::Config) -> Element<'_, Message> {
        let Some(menu) = &self.state.menu else {
            return crate::ui::body(config, "Menu unavailable").into();
        };
        let mut entries = column![].spacing(4);
        if menu.parent != 0 {
            entries = entries.push(
                button("Back to main menu")
                    .on_press(Message::Action(Command::Menu(menu.token.clone(), 0))),
            );
        }
        for entry in &menu.entries {
            if entry.separator {
                entries = entries.push(rule::horizontal(1));
                continue;
            }
            let label = format!(
                "{}{}{}",
                entry
                    .checked
                    .map(|c| if c { "✓ " } else { "○ " })
                    .unwrap_or(""),
                entry.label,
                if entry.submenu { "  ›" } else { "" }
            );
            let mut b = button(crate::ui::body(config, label)).width(Length::Fill);
            if entry.enabled {
                b = b.on_press(Message::Action(Command::Click(
                    menu.token.clone(),
                    entry.id,
                )));
            }
            entries = entries.push(b);
        }
        if self.state.failed {
            entries = entries.push(crate::ui::body(
                config,
                "Application did not accept the action",
            ));
        }
        container(
            column![
                row![
                    crate::ui::body(config, "Application menu").width(Length::Fill),
                    button("Close").on_press(Message::Close)
                ]
                .spacing(8),
                scrollable(entries).height(Length::Fill)
            ]
            .spacing(12),
        )
        .padding(16)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(container::rounded_box)
        .into()
    }
}
fn themed_icon(name: &str, extra: &str) -> Option<Icon> {
    if name.is_empty() {
        return None;
    }
    let mut candidates = Vec::new();
    if Path::new(name).is_absolute() {
        candidates.push(PathBuf::from(name));
    } else if !name.contains('/') && name != "." && name != ".." {
        let mut roots = Vec::new();
        if Path::new(extra).is_absolute() {
            roots.push(PathBuf::from(extra));
        }
        if let Some(home) = std::env::var_os("HOME") {
            roots.push(PathBuf::from(&home).join(".icons"));
            roots.push(
                std::env::var_os("XDG_DATA_HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from(home).join(".local/share"))
                    .join("icons"),
            );
        }
        for root in std::env::split_paths(
            &std::env::var_os("XDG_DATA_DIRS")
                .unwrap_or_else(|| "/usr/local/share:/usr/share".into()),
        ) {
            roots.push(root.join("icons"));
            candidates.push(root.join("pixmaps").join(format!("{name}.png")));
            candidates.push(root.join("pixmaps").join(format!("{name}.svg")));
        }
        for root in roots {
            for suffix in ["", "hicolor", "Adwaita"] {
                let base = root.join(suffix);
                for size in [
                    "", "24x24", "22x22", "32x32", "16x16", "48x48", "scalable", "symbolic",
                ] {
                    for category in ["", "status", "apps", "devices", "actions"] {
                        for extension in ["png", "svg"] {
                            candidates.push(
                                base.join(size)
                                    .join(category)
                                    .join(format!("{name}.{extension}")),
                            );
                        }
                    }
                }
            }
        }
    }
    for path in candidates {
        if !path
            .metadata()
            .is_ok_and(|m| m.is_file() && m.len() <= 1024 * 1024)
        {
            continue;
        }
        if path.extension().is_some_and(|e| e == "svg") {
            if let Ok(bytes) = std::fs::read(&path) {
                return Some(Icon::Vector(svg::Handle::from_memory(bytes)));
            }
        } else {
            let config = cabby_config::Config {
                wallpaper_image: path.to_string_lossy().into_owned(),
                ..Default::default()
            };
            if let Ok(Some(asset)) = cabby_config::wallpaper::load(&config) {
                return Some(Icon::Raster(image::Handle::from_rgba(
                    asset.width,
                    asset.height,
                    asset.rgba,
                )));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn closed_menus_cannot_be_reopened_by_late_worker_snapshots() {
        let token = Token {
            owner: ":1.2".into(),
            path: "/Item".into(),
        };
        let mut tray = Tray::default();
        tray.update(Message::Action(Command::Menu(token.clone(), 0)));
        let snapshot = State {
            available: true,
            menu: Some(cabby_tray::menu::Menu {
                token,
                path: "/Menu".into(),
                parent: 0,
                entries: Vec::new(),
            }),
            ..State::default()
        };
        tray.update(Message::State(snapshot.clone()));
        assert!(tray.state.menu.is_some());
        tray.update(Message::Close);
        tray.update(Message::State(snapshot));
        assert!(tray.state.menu.is_none());
    }
}
