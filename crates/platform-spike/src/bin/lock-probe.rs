//! Protocol experiment only: no authentication and never a production locker.
use iced::widget::{button, column, container, text};
use iced::{Element, Length, Task};
use iced_exwlshell::{sessionlock::application, to_sessionlock_message};

fn main() -> iced_exwlshell::Result {
    cabby_platform_spike::renderer_diagnostics();
    // The harness supplies both paths only to children of its disposable compositor.
    let expected = std::env::var("CABBY_TEST_WAYLAND").ok();
    let actual = std::env::var("WAYLAND_DISPLAY").ok();
    if expected.is_none() || expected != actual {
        eprintln!("lock-probe must run through the isolated Phase 0 harness");
        std::process::exit(2);
    }
    application(|| LockProbe, LockProbe::update, LockProbe::view).run()
}

struct LockProbe;

#[to_sessionlock_message]
#[derive(Debug, Clone)]
enum Message {
    UnLock,
}

impl LockProbe {
    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::UnLock => iced::exit(),
        }
    }

    fn view(&self) -> Element<'_, Message> {
        container(
            column![
                text("Cabby nested-session lock probe — NO AUTHENTICATION"),
                button("Release test lock").on_press(Message::UnLock),
            ]
            .spacing(20),
        )
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
    }
}
