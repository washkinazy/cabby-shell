use iced::widget::{button, column, container, row, text, text_input};
use iced::{Element, Event, Length, Subscription, Task, event, keyboard, window};
use iced_exwlshell::actions::IcedNewPopupSettings;
use iced_exwlshell::reexport::{Anchor, KeyboardInteractivity, LayerSize, PixelSize};
use iced_exwlshell::settings::{LayerShellSettings, Settings, StartMode};
use iced_exwlshell::{daemon, to_exwlshell_message};

fn main() -> iced_exwlshell::Result {
    cabby_platform_spike::renderer_diagnostics();
    daemon(
        Spike::default,
        || "cabby-phase0".into(),
        Spike::update,
        Spike::view,
    )
    .subscription(Spike::subscription)
    .settings(Settings {
        layer_settings: LayerShellSettings {
            anchor: Anchor::Top | Anchor::Left | Anchor::Right,
            size: LayerSize::fill_width(40),
            exclusive_zone: 40,
            keyboard_interactivity: KeyboardInteractivity::None,
            start_mode: StartMode::AllScreens,
            ..Default::default()
        },
        ..Default::default()
    })
    .run()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_dismisses_popup_and_allows_reopening() {
        let mut spike = Spike::default();
        let parent = window::Id::unique();
        let _ = spike.update(Message::Open(parent));
        let _ = spike.update(Message::Event(Event::Keyboard(
            keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(keyboard::key::Named::Escape),
                modified_key: keyboard::Key::Named(keyboard::key::Named::Escape),
                physical_key: keyboard::key::Physical::Code(keyboard::key::Code::Escape),
                location: keyboard::Location::Standard,
                modifiers: keyboard::Modifiers::empty(),
                text: None,
                repeat: false,
            },
        )));
        assert_eq!(spike.popup, None);
        let _ = spike.update(Message::Open(parent));
        assert!(spike.popup.is_some());
    }

    #[test]
    fn repeated_open_does_not_replace_the_live_popup() {
        let mut spike = Spike::default();
        let parent = window::Id::unique();
        let _ = spike.update(Message::Open(parent));
        let first = spike.popup.expect("popup created");
        let _ = spike.update(Message::Open(parent));
        assert_eq!(spike.popup, Some(first));
    }

    #[test]
    fn delayed_close_does_not_clear_a_reopened_popup() {
        let mut spike = Spike::default();
        let parent = window::Id::unique();
        let _ = spike.update(Message::Open(parent));
        let old = spike.popup.expect("first popup");
        let _ = spike.update(Message::Close);
        assert_eq!(spike.popup, None);
        let _ = spike.update(Message::Open(parent));
        let new = spike.popup.expect("second popup");
        assert_ne!(old, new);
        let _ = spike.update(Message::Closed(old));
        assert_eq!(spike.popup, Some(new));
        let _ = spike.update(Message::Closed(new));
        assert_eq!(spike.popup, None);
    }
}

#[derive(Default)]
struct Spike {
    popup: Option<window::Id>,
    query: String,
}

#[to_exwlshell_message]
#[derive(Debug, Clone)]
enum Message {
    Open(window::Id),
    Close,
    Closed(window::Id),
    Input(String),
    Event(Event),
}

impl Spike {
    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            event::listen().map(Message::Event),
            window::close_events().map(Message::Closed),
        ])
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Open(parent) if self.popup.is_none() => {
                let id = window::Id::unique();
                self.popup = Some(id);
                Task::batch([
                    Task::done(Message::NewPopUp {
                        settings: IcedNewPopupSettings::new(
                            parent,
                            PixelSize::px(420, 180),
                            (0, 0),
                            PixelSize::px(180, 40),
                        ),
                        id,
                    }),
                    iced::widget::operation::focus("query"),
                ])
            }
            Message::Close
            | Message::Event(Event::Keyboard(keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(keyboard::key::Named::Escape),
                ..
            })) => self.popup.take().map_or_else(Task::none, window::close),
            Message::Closed(id) if self.popup == Some(id) => {
                self.popup = None;
                Task::none()
            }
            Message::Input(query) => {
                self.query = query;
                Task::none()
            }
            _ => Task::none(),
        }
    }

    fn view(&self, id: window::Id) -> Element<'_, Message> {
        if self.popup == Some(id) {
            return container(
                column![
                    text("Keyboard focus probe — Escape dismisses"),
                    text_input("Type here", &self.query)
                        .id("query")
                        .on_input(Message::Input),
                    button("Close").on_press(Message::Close),
                ]
                .spacing(12),
            )
            .padding(16)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(container::rounded_box)
            .into();
        }
        container(
            row![
                text("Cabby / Phase 0"),
                button("Open popup").on_press(Message::Open(id)),
            ]
            .spacing(20),
        )
        .padding(6)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(container::rounded_box)
        .into()
    }
}
