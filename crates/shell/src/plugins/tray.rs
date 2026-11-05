//! Tray UI and owned worker subscription; the host owns the menu surface.
use crate::icons::{self, Cache, Icon, Key};
use cabby_tray::{Command, Handle, Service, State, Token};
use iced::futures::SinkExt;
use iced::widget::{
    button, column, container, image, mouse_area, row, rule, scrollable, svg, tooltip,
};
use iced::{Element, Length, Subscription, Task};
use std::{collections::HashMap, time::Duration};

#[derive(Clone, Debug)]
pub(crate) enum Message {
    Connected(Handle),
    State(State),
    Action(Command),
    Close,
    Icons(Vec<(Key, Option<Icon>)>),
}
#[derive(Default)]
pub(crate) struct Tray {
    pub state: State,
    handle: Option<Handle>,
    icons: HashMap<Token, Icon>,
    menu_requested: bool,
    cache: Cache,
    loading: bool,
    resolved: HashMap<Token, Key>,
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
    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Connected(handle) => self.handle = Some(handle),
            Message::State(mut state) => {
                if !self.menu_requested {
                    state.menu = None;
                }
                if !state.available {
                    self.handle = None;
                }
                self.icons
                    .retain(|token, _| state.items.iter().any(|i| &i.token == token));
                self.resolved
                    .retain(|token, _| state.items.iter().any(|i| &i.token == token));
                for item in &state.items {
                    if let Some(p) = &item.pixmap {
                        if !self
                            .state
                            .items
                            .iter()
                            .any(|old| old.token == item.token && old.pixmap == item.pixmap)
                        {
                            self.icons.insert(
                                item.token.clone(),
                                Icon::Raster(image::Handle::from_rgba(
                                    p.width,
                                    p.height,
                                    p.rgba.clone(),
                                )),
                            );
                        }
                        self.resolved.remove(&item.token);
                    }
                }
                self.state = state;
                return self.load_icons();
            }
            Message::Icons(results) => {
                self.loading = false;
                for (key, icon) in results {
                    for item in self.state.items.iter().filter(|i| {
                        i.pixmap.is_none()
                            && Key(i.icon_name.clone(), i.icon_theme_path.clone()) == key
                    }) {
                        self.resolved.insert(item.token.clone(), key.clone());
                        if let Some(icon) = &icon {
                            self.icons.insert(item.token.clone(), icon.clone());
                        }
                    }
                }
                return self.load_icons();
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
        Task::none()
    }
    fn load_icons(&mut self) -> Task<Message> {
        if self.loading {
            return Task::none();
        }
        let keys: std::collections::BTreeSet<_> = self
            .state
            .items
            .iter()
            .filter(|i| i.pixmap.is_none())
            .map(|i| (i, Key(i.icon_name.clone(), i.icon_theme_path.clone())))
            .filter(|(i, key)| self.resolved.get(&i.token) != Some(key))
            .map(|(_, key)| key)
            .collect();
        if keys.is_empty() {
            return Task::none();
        }
        self.loading = true;
        Task::perform(
            icons::load(self.cache.clone(), keys.into_iter().collect()),
            Message::Icons,
        )
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn late_icon_results_cannot_restore_removed_items() {
        let mut tray = Tray {
            loading: true,
            ..Default::default()
        };
        let _ = tray.update(Message::State(State::default()));
        let _ = tray.update(Message::Icons(vec![(
            Key("gone".into(), String::new()),
            Some(Icon::Raster(image::Handle::from_rgba(
                1,
                1,
                vec![0, 0, 0, 255],
            ))),
        )]));
        assert!(tray.icons.is_empty());
        assert!(tray.resolved.is_empty());
        assert!(!tray.loading);
    }
    #[test]
    fn closed_menus_cannot_be_reopened_by_late_worker_snapshots() {
        let token = Token {
            owner: ":1.2".into(),
            path: "/Item".into(),
        };
        let mut tray = Tray::default();
        let _ = tray.update(Message::Action(Command::Menu(token.clone(), 0)));
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
        let _ = tray.update(Message::State(snapshot.clone()));
        assert!(tray.state.menu.is_some());
        let _ = tray.update(Message::Close);
        let _ = tray.update(Message::State(snapshot));
        assert!(tray.state.menu.is_none());
    }
}
