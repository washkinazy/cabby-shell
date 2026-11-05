use crate::{niri::label, ui};
use cabby_config::Config;
use cabby_launcher::{Catalog, Picker};
use iced::widget::{button, column, container, row, scrollable, text, text_input};
use iced::{Element, Event, Length, Task, event, keyboard, window};
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
}
#[derive(Debug, Clone)]
pub(crate) enum Message {
    Loaded(Catalog),
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
        Action::Update(Task::none())
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
            .id("launcher-query")
            .on_submit(Message::Submit);
        if !self.picker.pending {
            input = input.on_input(Message::Query);
        }
        let mut content = column![
            row![
                text("Applications").width(Length::Fill),
                button("Close").on_press(Message::Close)
            ],
            input
        ]
        .spacing(8);
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
            let mut item = button(
                container(text(name).line_height(1.0).wrapping(text::Wrapping::None))
                    .clip(true)
                    .width(Length::Fill),
            )
            .width(Length::Fill)
            .padding([6, 8]);
            if !self.picker.pending {
                item = item.on_press(Message::Choose(index));
            }
            item = if index == self.picker.selected {
                item.style(button::primary)
            } else {
                item.style(button::text)
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
        content = content.push(text(status));
        container(scrollable(content))
            .padding(16)
            .width(Length::Fill)
            .height(Length::Fill)
            .clip(true)
            .style(container::rounded_box)
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
