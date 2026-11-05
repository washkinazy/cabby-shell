//! Credential editing and authentication; the host alone owns protocol exits.
use cabby_lock::{
    Secret, auth,
    policy::{Effect, Event as PolicyEvent, Policy, State},
};
use iced::{
    Element, Length, Task, keyboard,
    widget::{column, container, text},
};

#[derive(Clone)]
pub enum Message {
    Key(keyboard::Event),
    Submit,
    Authentication(u64, Result<(), auth::Error>),
}
impl std::fmt::Debug for Message {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("LockMessage([redacted])")
    }
}
pub struct Lock {
    pub policy: Policy,
    secret: Secret,
    client: auth::Client,
    status: &'static str,
}
impl Lock {
    pub fn new(client: auth::Client) -> Self {
        Self {
            policy: Policy::default(),
            secret: Secret::default(),
            client,
            status: "Securing session…",
        }
    }
    pub fn confirmed(&mut self) -> Effect {
        let effect = self.policy.update(PolicyEvent::Confirmed);
        if effect == Effect::Ready {
            self.status = "Enter your password";
        }
        effect
    }
    pub fn update(&mut self, message: Message) -> (Effect, Task<Message>) {
        match message {
            Message::Authentication(id, result) => {
                let current = self.policy.state == State::Authenticating(id);
                let effect = self.policy.update(PolicyEvent::Authentication(id, result));
                if current && result.is_err() {
                    self.status = match result {
                        Err(auth::Error::Denied) => "Authentication failed. Try again.",
                        _ => "Authentication unavailable. Try again.",
                    };
                }
                return (effect, Task::none());
            }
            Message::Key(keyboard::Event::KeyPressed {
                key,
                text,
                modifiers,
                ..
            }) if self.policy.state == State::Locked => match key {
                keyboard::Key::Named(keyboard::key::Named::Escape) => self.secret.clear(),
                keyboard::Key::Named(keyboard::key::Named::Backspace) => self.secret.backspace(),
                keyboard::Key::Named(keyboard::key::Named::Enter) => {
                    return self.update(Message::Submit);
                }
                keyboard::Key::Character(ref key)
                    if modifiers.control() && key.eq_ignore_ascii_case("u") =>
                {
                    self.secret.clear()
                }
                _ if !modifiers.control() && !modifiers.alt() && !modifiers.logo() => {
                    if let Some(text) = text {
                        self.secret.append(&text);
                    }
                }
                _ => {}
            },
            Message::Submit if !self.secret.is_empty() => {
                if let Effect::Authenticate(id) = self.policy.update(PolicyEvent::Submit) {
                    let secret = std::mem::take(&mut self.secret);
                    let client = self.client.clone();
                    self.status = "Checking…";
                    return (
                        Effect::None,
                        Task::perform(
                            async move { client.authenticate(secret).await },
                            move |result| Message::Authentication(id, result),
                        ),
                    );
                }
            }
            _ => {}
        }
        (Effect::None, Task::none())
    }
    // Only the guarded disposable-session probe supplies a synthetic credential.
    pub fn fixture_input(&mut self, value: &str) {
        self.secret.append(value);
    }
    pub fn view(&self) -> Element<'_, Message> {
        container(
            column![
                text("Session locked").size(32),
                text(self.status),
                text(if self.secret.is_empty() {
                    " ".into()
                } else {
                    self.secret.masked()
                })
                .size(24),
                text("Enter to unlock · Esc to clear").size(14)
            ]
            .spacing(20)
            .align_x(iced::Alignment::Center),
        )
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn key(key: keyboard::Key, text: Option<&str>, modifiers: keyboard::Modifiers) -> Message {
        Message::Key(keyboard::Event::KeyPressed {
            key: key.clone(),
            modified_key: key,
            physical_key: keyboard::key::Physical::Code(keyboard::key::Code::KeyA),
            location: keyboard::Location::Standard,
            modifiers,
            text: text.map(Into::into),
            repeat: false,
        })
    }
    #[test]
    fn input_is_gated_redacted_and_escape_clears_without_unlock() {
        let mut lock = Lock::new(auth::Client::new("/unused".into()));
        let character = key(
            keyboard::Key::Character("é".into()),
            Some("é"),
            keyboard::Modifiers::empty(),
        );
        assert!(!format!("{character:?}").contains('é'));
        let _ = lock.update(character.clone());
        assert!(lock.secret.is_empty());
        assert_eq!(lock.confirmed(), Effect::Ready);
        let _ = lock.update(character);
        assert_eq!(lock.secret.masked(), "•");
        let _ = lock.update(key(
            keyboard::Key::Character("v".into()),
            Some("v"),
            keyboard::Modifiers::CTRL,
        ));
        assert_eq!(lock.secret.masked(), "•");
        let (effect, _) = lock.update(key(
            keyboard::Key::Named(keyboard::key::Named::Escape),
            None,
            keyboard::Modifiers::empty(),
        ));
        assert_eq!(effect, Effect::None);
        assert!(lock.secret.is_empty());
        assert_eq!(lock.policy.state, State::Locked);
        let _ = lock.update(Message::Submit);
        assert_eq!(lock.policy.state, State::Locked);
        lock.fixture_input("fixture");
        let _ = lock.update(Message::Submit);
        assert!(lock.secret.is_empty());
        assert_eq!(lock.policy.state, State::Authenticating(1));
        let _ = lock.update(key(
            keyboard::Key::Character("a".into()),
            Some("a"),
            keyboard::Modifiers::empty(),
        ));
        assert!(lock.secret.is_empty());
        let (effect, _) = lock.update(Message::Authentication(2, Ok(())));
        assert_eq!(effect, Effect::None);
        let (effect, _) = lock.update(Message::Authentication(1, Err(auth::Error::Denied)));
        assert_eq!(effect, Effect::None);
        assert_eq!(lock.policy.state, State::Locked);
        lock.fixture_input("fixture");
        let _ = lock.update(Message::Submit);
        let (effect, _) = lock.update(Message::Authentication(2, Ok(())));
        assert_eq!(effect, Effect::Unlock);
    }
}
