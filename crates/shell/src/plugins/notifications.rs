//! Notification state, service subscription, actions, and card content.
use cabby_notification_service::{Command, Error, Handle, Service};
use cabby_notifications::{Entry, MAX_VISIBLE, Token};
use iced::futures::SinkExt;
use iced::widget::{Space, button, column, container, row, scrollable};
use iced::{Element, Length, Subscription, Task};

#[derive(Default)]
pub(crate) struct Notifications {
    entries: Vec<Entry>,
    handle: Option<Handle>,
    failed: bool,
}
#[derive(Debug, Clone)]
pub(crate) enum Message {
    Connected(Handle),
    Snapshot(Vec<Entry>),
    Presented(Token),
    Hover(Token, bool, u64),
    Dismiss(Token),
    Invoke(Token, usize),
    Result(Result<(), Error>),
    Fatal(Error),
}
pub(crate) enum Action {
    Update(Task<Message>),
    Changed,
    Exit(Error),
}
fn service() -> impl iced::futures::Stream<Item = Message> {
    iced::stream::channel(
        1,
        |mut output: iced::futures::channel::mpsc::Sender<Message>| async move {
            let connected = match std::env::var("DBUS_SESSION_BUS_ADDRESS") {
                Ok(address) => Service::connect(&address).await,
                Err(_) => Err(Error::Transport),
            };
            let (service, handle) = match connected {
                Ok(pair) => pair,
                Err(error) => {
                    let _ = output.send(Message::Fatal(error)).await;
                    return;
                }
            };
            let mut snapshots = handle.snapshots.clone();
            if output.send(Message::Connected(handle)).await.is_err() {
                return;
            }
            let running = service.with_presentation().run();
            tokio::pin!(running);
            loop {
                tokio::select! {
                    result = &mut running => { let _ = output.send(Message::Fatal(result.err().unwrap_or(Error::Stopped))).await; return; }
                    changed = snapshots.changed() => {
                        if changed.is_err() { return; }
                        let entries = snapshots.borrow_and_update().clone();
                        if output.send(Message::Snapshot(entries)).await.is_err() { return; }
                    }
                }
            }
        },
    )
}
impl Notifications {
    pub(crate) fn subscription(&self) -> Subscription<Message> {
        Subscription::run(service)
    }
    pub(crate) fn visible(&self) -> Vec<Token> {
        self.entries
            .iter()
            .take(MAX_VISIBLE)
            .map(Entry::token)
            .collect()
    }
    pub(crate) fn update(&mut self, message: Message) -> Action {
        match message {
            Message::Connected(handle) => self.handle = Some(handle),
            Message::Snapshot(entries) => {
                self.entries = entries;
                return Action::Changed;
            }
            Message::Presented(token) => {
                return Action::Update(self.command(Command::Presented(token)));
            }
            Message::Hover(token, hovered, sequence) => {
                return Action::Update(self.command(Command::Hover(token, hovered, sequence)));
            }
            Message::Dismiss(token) => {
                return Action::Update(self.command(Command::Dismiss(token)));
            }
            Message::Invoke(token, index) => {
                if let Some(entry) = self.entries.iter().find(|e| e.token() == token)
                    && let Some(action) = entry.actions.get(index)
                {
                    return Action::Update(
                        self.command(Command::Invoke(token, action.key.clone())),
                    );
                }
            }
            Message::Result(Err(Error::Policy(_))) => (),
            Message::Result(result) => self.failed = result.is_err(),
            Message::Fatal(error) => {
                self.entries.clear();
                self.handle = None;
                return Action::Exit(error);
            }
        }
        Action::Update(Task::none())
    }
    fn command(&self, command: Command) -> Task<Message> {
        let Some(handle) = self.handle.clone() else {
            return Task::none();
        };
        Task::perform(
            async move { handle.command(command).await },
            Message::Result,
        )
    }
    pub(crate) fn view(&self, token: Token, config: &cabby_config::Config) -> Element<'_, Message> {
        let Some(entry) = self.entries.iter().find(|e| e.token() == token) else {
            return Space::new().into();
        };
        let token = entry.token();
        let mut content = column![
            row![
                crate::ui::body(config, &entry.app_name).width(Length::Fill),
                button("Close").on_press(Message::Dismiss(token))
            ]
            .spacing(8),
            crate::ui::body(config, &entry.summary),
            crate::ui::body(config, &entry.body)
        ]
        .spacing(6);
        for (index, action) in entry.actions.iter().enumerate() {
            content = content.push(
                button(crate::ui::body(config, &action.label))
                    .on_press(Message::Invoke(token, index)),
            );
        }
        if self.failed {
            content = content.push(crate::ui::body(config, "Action unavailable; retry"));
        }
        container(scrollable(content))
            .padding(8)
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
    fn snapshots_bound_visible_cards_and_fatal_requests_host_exit() {
        let mut plugin = Notifications::default();
        let mut store = cabby_notifications::Store::default();
        for _ in 0..4 {
            store.notify(Default::default()).unwrap();
        }
        assert!(matches!(
            plugin.update(Message::Snapshot(store.entries().to_vec())),
            Action::Changed
        ));
        assert_eq!(plugin.visible().len(), MAX_VISIBLE);
        let first = plugin.visible()[0];
        store
            .notify(cabby_notifications::Request {
                replaces_id: first.id(),
                ..Default::default()
            })
            .unwrap();
        let _ = plugin.update(Message::Snapshot(store.entries().to_vec()));
        assert_ne!(plugin.visible()[0], first);
        assert!(matches!(
            plugin.update(Message::Invoke(first, 0)),
            Action::Update(_)
        ));
        assert!(matches!(
            plugin.update(Message::Fatal(Error::Conflict)),
            Action::Exit(Error::Conflict)
        ));
        assert!(plugin.visible().is_empty());
    }
}
