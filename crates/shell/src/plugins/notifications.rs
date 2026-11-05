use crate::ui;
use cabby_config::{BarPosition, Config};
use cabby_notification_service::{Command, Error, Handle, Service};
use cabby_notifications::{Entry, MAX_VISIBLE, Token};
use iced::futures::SinkExt;
use iced::widget::{Space, button, column, container, row, scrollable, text};
use iced::{Element, Length, Subscription, Task, window};
use iced_exwlshell::reexport::{
    Anchor, KeyboardInteractivity, Layer, LayerSize, NewLayerShellSettings, OutputOption,
};
use iced_exwlshell::settings::{LayerShellSettings, Settings, StartMode};
use iced_exwlshell::shell::{self, ShellEvent, ShellReceiver};
use iced_exwlshell::{daemon, to_exwlshell_message};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicI32, Ordering};

static EXIT_CODE: AtomicI32 = AtomicI32::new(0);
pub(crate) fn run(context: crate::host::Context) -> iced_exwlshell::Result {
    let config = context.config;
    let (sender, receiver) = shell::channel();
    let text_size = iced::Pixels(f32::from(config.font_size));
    let result = daemon(
        move || Notifications {
            config: config.clone(),
            shell: receiver.clone(),
            entries: Vec::new(),
            handle: None,
            cards: BTreeMap::new(),
            outputs: BTreeMap::new(),
            failed: false,
        },
        || "cabby-notifications".into(),
        Notifications::update,
        Notifications::view,
    )
    .subscription(Notifications::subscription)
    .theme(|state: &Notifications, _| ui::theme(&state.config))
    .settings(Settings {
        default_text_size: text_size,
        shell_broadcast: sender,
        layer_settings: LayerShellSettings {
            start_mode: StartMode::Background,
            ..Default::default()
        },
        ..Default::default()
    })
    .run();
    let code = EXIT_CODE.load(Ordering::Relaxed);
    if code != 0 {
        std::process::exit(code);
    }
    result
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
struct Card {
    token: Token,
    output: Option<u32>,
    waiting_frame: bool,
}
struct Notifications {
    config: Config,
    shell: ShellReceiver,
    entries: Vec<Entry>,
    handle: Option<Handle>,
    cards: BTreeMap<window::Id, Card>,
    outputs: BTreeMap<u32, (u32, u32)>,
    failed: bool,
}
#[to_exwlshell_message]
#[derive(Debug, Clone)]
enum Message {
    Connected(Handle),
    Snapshot(Vec<Entry>),
    Shell(ShellEvent),
    Drawn(window::Id, Token),
    Dismiss(Token),
    Invoke(Token, usize),
    Result(Result<(), Error>),
    Fatal(Error),
}
fn geometry(config: &Config, outputs: &BTreeMap<u32, (u32, u32)>) -> (u32, u32, i32) {
    let width = outputs.values().map(|s| s.0).min().unwrap_or(400);
    let height = outputs.values().map(|s| s.1).min().unwrap_or(640);
    let top = if config.bar_position == BarPosition::Top {
        u32::from(config.bar_height) + 8
    } else {
        8
    };
    (
        width.saturating_sub(16).clamp(1, 380),
        height
            .saturating_sub(
                top + 24
                    + if config.bar_position == BarPosition::Bottom {
                        u32::from(config.bar_height)
                    } else {
                        0
                    },
            )
            .saturating_div(3)
            .clamp(1, 180),
        top as i32,
    )
}
impl Notifications {
    fn subscription(&self) -> Subscription<Message> {
        let pending: Vec<_> = self
            .cards
            .iter()
            .filter(|(_, c)| c.waiting_frame)
            .map(|(id, card)| (*id, card.token))
            .collect();
        Subscription::batch([
            Subscription::run(service),
            self.shell.listen().map(Message::Shell),
            iced::event::listen_raw(|event, _, id| {
                matches!(
                    event,
                    iced::Event::Window(window::Event::RedrawRequested(_))
                )
                .then_some(id)
            })
            .with(pending)
            .filter_map(|(pending, id)| {
                pending
                    .iter()
                    .find(|(window, _)| *window == id)
                    .map(|(_, token)| Message::Drawn(id, *token))
            }),
        ])
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
    fn reconcile(&mut self) -> Task<Message> {
        let mut tasks = Vec::new();
        let wanted: Vec<_> = self
            .entries
            .iter()
            .take(MAX_VISIBLE)
            .map(Entry::token)
            .collect();
        let removed: Vec<_> = self
            .cards
            .iter()
            .filter(|(_, card)| !wanted.iter().any(|t| t.id() == card.token.id()))
            .map(|(id, _)| *id)
            .collect();
        for id in removed {
            self.cards.remove(&id);
            tasks.push(Task::done(Message::RemoveWindow(id)));
        }
        if self.outputs.is_empty() {
            return Task::batch(tasks);
        }
        let (width, height, top) = geometry(&self.config, &self.outputs);
        for (slot, token) in wanted.into_iter().enumerate() {
            let margin = (top + slot as i32 * (height as i32 + 8), 8, 0, 0);
            if let Some((id, card)) = self
                .cards
                .iter_mut()
                .find(|(_, c)| c.token.id() == token.id())
            {
                if card.token != token {
                    card.token = token;
                    card.waiting_frame = true;
                }
                tasks.push(Task::done(Message::MarginChange { id: *id, margin }));
                tasks.push(Task::done(Message::LayoutChange {
                    id: *id,
                    anchor: Anchor::Top | Anchor::Right,
                    size: LayerSize::px(width, height),
                }));
            } else {
                let id = window::Id::unique();
                self.cards.insert(
                    id,
                    Card {
                        token,
                        output: None,
                        waiting_frame: true,
                    },
                );
                tasks.push(Task::done(Message::NewLayerShell {
                    id,
                    settings: NewLayerShellSettings {
                        size: LayerSize::px(width, height),
                        anchor: Anchor::Top | Anchor::Right,
                        layer: Layer::Overlay,
                        exclusive_zone: Some(-1),
                        margin: Some(margin),
                        keyboard_interactivity: KeyboardInteractivity::None,
                        output_option: OutputOption::Active,
                        namespace: Some("cabby-notifications".into()),
                        ..Default::default()
                    },
                }));
            }
        }
        Task::batch(tasks)
    }
    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Connected(handle) => self.handle = Some(handle),
            Message::Snapshot(entries) => {
                self.entries = entries;
                return self.reconcile();
            }
            Message::Shell(ShellEvent::OutputAdded(info) | ShellEvent::OutputUpdated(info)) => {
                let size = info.logical_size.unwrap_or((800, 600));
                self.outputs
                    .insert(info.id, (size.0.max(1) as u32, size.1.max(1) as u32));
                return self.reconcile();
            }
            Message::Shell(ShellEvent::OutputRemoved(info)) => {
                self.outputs.remove(&info.id);
                let removed: Vec<_> = self
                    .cards
                    .iter()
                    .filter(|(_, c)| c.output == Some(info.id))
                    .map(|(id, _)| *id)
                    .collect();
                let mut tasks = Vec::new();
                for id in removed {
                    self.cards.remove(&id);
                    tasks.push(Task::done(Message::RemoveWindow(id)));
                }
                tasks.push(self.reconcile());
                return Task::batch(tasks);
            }
            Message::Shell(ShellEvent::WindowOutputChanged { window, output }) => {
                if let Some(card) = self.cards.get_mut(&window) {
                    card.output = output.map(|o| o.id);
                }
            }
            Message::Shell(ShellEvent::Closed(id)) => {
                if self.cards.remove(&id).is_some() {
                    return self.reconcile();
                }
            }
            Message::Drawn(id, drawn_token) => {
                if let Some(card) = self.cards.get_mut(&id)
                    && card.waiting_frame
                    && card.token == drawn_token
                {
                    card.waiting_frame = false;
                    let token = card.token;
                    return self.command(Command::Presented(token));
                }
            }
            Message::Dismiss(token) => return self.command(Command::Dismiss(token)),
            Message::Invoke(token, index) => {
                if let Some(entry) = self.entries.iter().find(|e| e.token() == token)
                    && let Some(action) = entry.actions.get(index)
                {
                    return self.command(Command::Invoke(token, action.key.clone()));
                }
            }
            Message::Result(Err(Error::Policy(_))) => (), // A replaced/closed card made this event stale.
            Message::Result(result) => self.failed = result.is_err(),
            Message::Fatal(error) => {
                eprintln!("{error}");
                EXIT_CODE.store(
                    if error == Error::Conflict { 73 } else { 1 },
                    Ordering::Relaxed,
                );
                self.entries.clear();
                self.handle = None;
                return iced::exit();
            }
            _ => (),
        }
        Task::none()
    }
    fn view(&self, id: window::Id) -> Element<'_, Message> {
        let Some(entry) = self
            .cards
            .get(&id)
            .and_then(|card| self.entries.iter().find(|e| e.token() == card.token))
        else {
            return Space::new().into();
        };
        let token = entry.token();
        let mut content = column![
            row![
                text(&entry.app_name).width(Length::Fill),
                button("Close").on_press(Message::Dismiss(token))
            ]
            .spacing(8),
            text(&entry.summary),
            text(&entry.body)
        ]
        .spacing(6);
        for (index, action) in entry.actions.iter().enumerate() {
            content =
                content.push(button(text(&action.label)).on_press(Message::Invoke(token, index)));
        }
        if self.failed {
            content = content.push(text("Action unavailable; retry"));
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
    fn state() -> Notifications {
        Notifications {
            config: Config::default(),
            shell: shell::channel().1,
            entries: vec![],
            handle: None,
            cards: BTreeMap::new(),
            outputs: BTreeMap::from([(1, (800, 600))]),
            failed: false,
        }
    }
    #[test]
    fn replacement_preserves_surface_and_old_close_cannot_remove_new_card() {
        let mut state = state();
        let mut store = cabby_notifications::Store::default();
        let first = store
            .notify(cabby_notifications::Request::default())
            .unwrap();
        state.entries = store.entries().to_vec();
        let _ = state.reconcile();
        let id = *state.cards.keys().next().unwrap();
        store
            .notify(cabby_notifications::Request {
                replaces_id: first.id(),
                ..Default::default()
            })
            .unwrap();
        state.entries = store.entries().to_vec();
        let _ = state.reconcile();
        assert_eq!(*state.cards.keys().next().unwrap(), id);
        assert_ne!(state.cards[&id].token, first);
        let _ = state.update(Message::Drawn(id, first));
        assert!(state.cards[&id].waiting_frame);
        state.cards.remove(&id);
        let _ = state.reconcile();
        let new = *state.cards.keys().next().unwrap();
        let _ = state.update(Message::Shell(ShellEvent::Closed(id)));
        assert!(state.cards.contains_key(&new));
    }
    #[test]
    fn queue_and_no_output_do_not_create_extra_surfaces() {
        let mut state = state();
        let mut store = cabby_notifications::Store::default();
        for _ in 0..4 {
            store.notify(Default::default()).unwrap();
        }
        state.entries = store.entries().to_vec();
        state.outputs.clear();
        let _ = state.reconcile();
        assert!(state.cards.is_empty());
        state.outputs.insert(2, (320, 400));
        let _ = state.reconcile();
        assert_eq!(state.cards.len(), 3);
        let (width, height, top) = geometry(&state.config, &state.outputs);
        assert!(width <= 304);
        assert!(top as u32 + 3 * (height + 8) <= 400);
    }
}
