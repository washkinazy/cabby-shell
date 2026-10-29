use cabby_config::Config;
use cabby_launcher::{Catalog, Picker};
use cabby_platform_spike::{niri::label, ui};
use iced::widget::{button, column, container, row, scrollable, text, text_input};
use iced::{Element, Event, Length, Task, event, keyboard, window};
use iced_exwlshell::reexport::{Anchor, KeyboardInteractivity, Layer, LayerSize};
use iced_exwlshell::settings::{LayerShellSettings, Settings, StartMode};
use iced_exwlshell::{daemon, to_exwlshell_message};
use std::path::PathBuf;

fn main() -> iced_exwlshell::Result {
    let source = ui::configuration_source().unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(1)
    });
    let config = source.load(true).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(1)
    });
    ui::renderer(&config);
    cabby_platform_spike::renderer_diagnostics();
    let settings = Settings {
        default_text_size: iced::Pixels(f32::from(config.font_size)),
        layer_settings: LayerShellSettings {
            anchor: Anchor::empty(),
            layer: Layer::Overlay,
            size: LayerSize::px(
                u32::from(config.launcher_width),
                u32::from(config.launcher_rows + 3) * u32::from(config.font_size + 20) + 64,
            ),
            exclusive_zone: 0,
            keyboard_interactivity: KeyboardInteractivity::Exclusive,
            start_mode: StartMode::Active,
            ..Default::default()
        },
        ..Default::default()
    };
    daemon(
        move || {
            (
                Launcher {
                    config: config.clone(),
                    source: source.explicit.then(|| source.path.clone()),
                    picker: Picker::default(),
                    loading: true,
                },
                Task::batch([
                    Task::perform(
                        async {
                            tokio::task::spawn_blocking(cabby_launcher::discover)
                                .await
                                .unwrap_or_default()
                        },
                        Message::Loaded,
                    ),
                    iced::widget::operation::focus("launcher-query"),
                ]),
            )
        },
        || "cabby-launcher".into(),
        Launcher::update,
        Launcher::view,
    )
    .on_new_shell(|_| Some(Message::Ready))
    .subscription(|_| event::listen_with(launcher_event))
    .theme(|state: &Launcher, _| ui::theme(&state.config))
    .settings(settings)
    .run()
}
// Escape is captured by text_input when it releases widget focus. Still close
// the launcher on the first press; Enter stays exclusively on on_submit.
fn launcher_event(event: Event, _: event::Status, _: window::Id) -> Option<Message> {
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
struct Launcher {
    config: Config,
    source: Option<PathBuf>,
    picker: Picker,
    loading: bool,
}
#[to_exwlshell_message]
#[derive(Debug, Clone)]
enum Message {
    Ready,
    Loaded(Catalog),
    Query(String),
    Choose(usize),
    Submit,
    LaunchResult(Result<(), String>),
    Event(Event),
    Close,
}
impl Launcher {
    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Ready => return iced::widget::operation::focus("launcher-query"),
            Message::Loaded(catalog) => {
                let query = self.picker.query.clone();
                self.picker = Picker::loaded(catalog);
                self.picker.filter(query);
                self.loading = false;
            }
            Message::Query(query) => self.picker.filter(query),
            Message::Choose(index) if !self.picker.pending && index < self.picker.matches.len() => {
                self.picker.selected = index;
                return self.submit();
            }
            Message::Submit => return self.submit(),
            Message::Close => return iced::exit(),
            Message::LaunchResult(Ok(())) => return iced::exit(),
            Message::LaunchResult(Err(error)) => self.picker.failed(error),
            Message::Event(Event::Keyboard(keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(key),
                ..
            })) => match key {
                keyboard::key::Named::Escape => return iced::exit(),
                keyboard::key::Named::ArrowUp => self.picker.step(true),
                keyboard::key::Named::ArrowDown => self.picker.step(false),
                _ => (),
            },
            Message::Event(Event::Window(window::Event::Closed)) => return iced::exit(),
            _ => (),
        }
        Task::none()
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

    fn view(&self, _: window::Id) -> Element<'_, Message> {
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
