use crate::{
    icons::{self, Cache, Icon, Key},
    niri::label,
    ui,
};
use cabby_config::Config;
use cabby_launcher::{Catalog, Picker};
use iced::widget::{column, container, row, scrollable, text, text_input};
use iced::{Element, Event, Length, Task, event, keyboard, window};
use std::collections::BTreeMap;
use std::path::PathBuf;

// Escape is captured by text_input when it releases widget focus. Still close
// the launcher on the first press; Enter stays exclusively on on_submit.
pub(crate) fn launcher_event(event: Event, _: event::Status, _: window::Id) -> Option<Message> {
    match &event {
        Event::Keyboard(keyboard::Event::KeyPressed {
            key:
                keyboard::Key::Named(
                    keyboard::key::Named::Escape
                    | keyboard::key::Named::ArrowUp
                    | keyboard::key::Named::ArrowDown,
                ),
            ..
        })
        | Event::Window(window::Event::Closed) => Some(Message::Event(event)),
        _ => None,
    }
}
pub(crate) struct Launcher {
    config: Config,
    source: Option<PathBuf>,
    picker: Picker,
    loading: bool,
    icons: BTreeMap<Key, Option<Icon>>,
    icon_cache: Cache,
    icons_loading: bool,
}
#[derive(Debug, Clone)]
pub(crate) enum Message {
    Loaded(Catalog),
    Icons(Vec<(Key, Option<Icon>)>),
    Query(String),
    Choose(usize),
    Submit,
    LaunchResult(Result<(), String>),
    Event(Event),
    Close,
}
pub(crate) enum Action {
    Update(Task<Message>),
    Close,
}
impl Launcher {
    pub(crate) fn new(config: Config, source: Option<PathBuf>) -> (Self, Task<Message>) {
        (
            Self {
                config,
                source,
                picker: Picker::default(),
                loading: true,
                icons: BTreeMap::new(),
                icon_cache: Cache::default(),
                icons_loading: false,
            },
            Task::perform(
                async {
                    tokio::task::spawn_blocking(cabby_launcher::discover)
                        .await
                        .unwrap_or_default()
                },
                Message::Loaded,
            ),
        )
    }
    pub(crate) fn update(&mut self, message: Message) -> Action {
        match message {
            Message::Loaded(catalog) => {
                let query = self.picker.query.clone();
                self.picker = Picker::loaded(catalog);
                self.picker.filter(query);
                self.loading = false;
            }
            Message::Icons(results) => {
                self.icons_loading = false;
                // Only current-page handles are retained; the shared cache bounds older results.
                let wanted = self.icon_keys();
                self.icons.retain(|key, _| wanted.contains(key));
                for (key, icon) in results {
                    if wanted.contains(&key) {
                        self.icons.insert(key, icon);
                    }
                }
            }
            Message::Query(query) => self.picker.filter(query),
            Message::Choose(index) if !self.picker.pending && index < self.picker.matches.len() => {
                self.picker.selected = index;
                return Action::Update(self.submit());
            }
            Message::Submit => return Action::Update(self.submit()),
            Message::Close => return Action::Close,
            Message::LaunchResult(Ok(())) => return Action::Close,
            Message::LaunchResult(Err(error)) => self.picker.failed(error),
            Message::Event(Event::Keyboard(keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(key),
                ..
            })) => match key {
                keyboard::key::Named::Escape => return Action::Close,
                keyboard::key::Named::ArrowUp => self.picker.step(true),
                keyboard::key::Named::ArrowDown => self.picker.step(false),
                _ => (),
            },
            Message::Event(Event::Window(window::Event::Closed)) => return Action::Close,
            _ => (),
        }
        Action::Update(self.load_icons())
    }
    fn icon_keys(&self) -> Vec<Key> {
        let rows = usize::from(self.config.launcher_rows);
        let page = self.picker.selected / rows * rows;
        self.picker
            .matches
            .iter()
            .skip(page)
            .take(rows)
            .filter_map(|i| self.picker.catalog.apps[*i].icon.as_ref())
            .map(|name| Key(name.clone(), String::new()))
            .collect()
    }
    fn load_icons(&mut self) -> Task<Message> {
        let wanted = self.icon_keys();
        self.icons.retain(|key, _| wanted.contains(key));
        if self.icons_loading {
            return Task::none();
        }
        let keys = wanted
            .into_iter()
            .filter(|key| !self.icons.contains_key(key))
            .collect::<Vec<_>>();
        if keys.is_empty() {
            return Task::none();
        }
        self.icons_loading = true;
        Task::perform(icons::load(self.icon_cache.clone(), keys), Message::Icons)
    }
    fn submit(&mut self) -> Task<Message> {
        let Some(app) = self.picker.begin() else {
            return Task::none();
        };
        if let Err(error) = app.command(&self.config.launcher_terminal) {
            self.picker.failed(error);
            return Task::none();
        }
        let mut args = vec!["launch-app".into(), "--desktop-id".into(), app.id.into()];
        if let Some(source) = &self.source {
            args.extend(["--config".into(), source.clone().into_os_string()]);
        }
        Task::perform(ui::command(args), Message::LaunchResult)
    }

    pub(crate) fn view(&self) -> Element<'_, Message> {
        let mut input = text_input("Search applications", &self.picker.query)
            .style(crate::ui::input_style)
            .padding([8, 10])
            .id("launcher-query")
            .on_submit(Message::Submit);
        if !self.picker.pending {
            input = input.on_input(Message::Query);
        }
        let header = row![
            ui::heading(&self.config, "Applications").width(Length::Fill),
            crate::ui::button("Close")
                .style(crate::ui::quiet)
                .on_press(Message::Close)
        ];
        let mut content = column![].spacing(8).padding(iced::Padding {
            right: 12.0,
            ..Default::default()
        });
        let page = self.picker.selected / usize::from(self.config.launcher_rows)
            * usize::from(self.config.launcher_rows);
        for (index, &app_index) in self
            .picker
            .matches
            .iter()
            .enumerate()
            .skip(page)
            .take(usize::from(self.config.launcher_rows))
        {
            let app = &self.picker.catalog.apps[app_index];
            let name = format!(
                "{}{}",
                label(&app.name, 70),
                if app.terminal { "  [terminal]" } else { "" }
            );
            let icon: Element<'_, Message> = match app
                .icon
                .as_ref()
                .and_then(|name| self.icons.get(&Key(name.clone(), String::new())))
                .and_then(Option::as_ref)
            {
                Some(Icon::Raster(handle)) => iced::widget::image(handle.clone())
                    .width(24)
                    .height(24)
                    .into(),
                Some(Icon::Vector(handle)) => iced::widget::svg(handle.clone())
                    .width(24)
                    .height(24)
                    .into(),
                None => container(text("·")).center(24).into(),
            };
            let mut item = crate::ui::button(
                container(
                    row![
                        container(iced::widget::Space::new())
                            .width(3)
                            .height(22)
                            .style(if index == self.picker.selected {
                                ui::accent_mark
                            } else {
                                crate::ui::blank
                            }),
                        icon,
                        text(name).line_height(1.0).wrapping(text::Wrapping::None)
                    ]
                    .spacing(8)
                    .align_y(iced::Alignment::Center),
                )
                .clip(true)
                .width(Length::Fill),
            )
            .width(Length::Fill)
            .padding([6, 8]);
            if !self.picker.pending {
                item = item.on_press(Message::Choose(index));
            }
            item = if index == self.picker.selected {
                item.style(crate::ui::selected)
            } else {
                item.style(crate::ui::quiet)
            };
            content = content.push(item);
        }
        let status = if self.loading {
            "Loading applications…".into()
        } else if self.picker.pending {
            "Launching…".into()
        } else if let Some(error) = &self.picker.error {
            error.clone()
        } else if self.picker.matches.is_empty() {
            "No matching applications".into()
        } else {
            format!(
                "{} results · ↑ ↓ select · Enter launch · Esc close",
                self.picker.matches.len()
            )
        };
        container(
            column![
                header,
                input,
                scrollable(content).height(Length::Fill),
                ui::secondary(&self.config, status)
            ]
            .spacing(8),
        )
        .padding(16)
        .width(Length::Fill)
        .height(Length::Fill)
        .clip(true)
        .style(crate::ui::panel(self.config.renderer))
        .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn key(named: keyboard::key::Named) -> Event {
        Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(named),
            modified_key: keyboard::Key::Named(named),
            physical_key: keyboard::key::Physical::Code(keyboard::key::Code::Escape),
            location: keyboard::Location::Standard,
            modifiers: keyboard::Modifiers::empty(),
            text: None,
            repeat: false,
        })
    }
    #[test]
    fn late_icons_do_not_repopulate_a_changed_result_page() {
        let (mut launcher, _) = Launcher::new(Config::default(), None);
        let _ = launcher.update(Message::Query("no results".into()));
        let _ = launcher.update(Message::Icons(vec![(
            Key("old".into(), String::new()),
            Some(Icon::Raster(iced::widget::image::Handle::from_rgba(
                1,
                1,
                vec![0, 0, 0, 255],
            ))),
        )]));
        assert!(launcher.icons.is_empty());
        assert!(!launcher.icons_loading);
    }
    #[test]
    fn dismissals_request_host_close_and_launch_errors_keep_overlay_open() {
        let (mut launcher, _) = Launcher::new(Config::default(), None);
        for message in [
            Message::Close,
            Message::LaunchResult(Ok(())),
            Message::Event(key(keyboard::key::Named::Escape)),
            Message::Event(Event::Window(window::Event::Closed)),
        ] {
            assert!(matches!(launcher.update(message), Action::Close));
        }
        assert!(matches!(
            launcher.update(Message::LaunchResult(Err("fixture failure".into()))),
            Action::Update(_)
        ));
        assert_eq!(launcher.picker.error.as_deref(), Some("fixture failure"));
    }
    #[test]
    fn captured_escape_closes_but_enter_cannot_submit_twice() {
        let id = window::Id::unique();
        assert!(
            launcher_event(
                key(keyboard::key::Named::Escape),
                event::Status::Captured,
                id
            )
            .is_some()
        );
        assert!(
            launcher_event(
                key(keyboard::key::Named::ArrowDown),
                event::Status::Captured,
                id
            )
            .is_some()
        );
        assert!(
            launcher_event(key(keyboard::key::Named::Enter), event::Status::Ignored, id).is_none()
        );
    }
}
