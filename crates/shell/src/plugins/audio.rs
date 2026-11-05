//! On-demand audio state and controls; all helpers belong to this subscription.
use cabby_audio::{Client, Command, Error, Kind, Operation, Snapshot};
use iced::futures::SinkExt;
use iced::widget::{button, column, container, row, scrollable, slider};
use iced::{Element, Length, Subscription};
use std::time::Duration;
use tokio::sync::mpsc;

#[derive(Clone, Debug)]
pub(crate) enum Request {
    Apply(Command),
    Refresh,
}
#[derive(Clone, Debug)]
pub(crate) enum Message {
    Connected(mpsc::Sender<Request>),
    Snapshot(Result<Snapshot, Error>),
    Done(Result<(), Error>),
    Apply(Command),
    Volume(Kind, String, u8),
    CommitVolume(Kind, String),
    Refresh,
    Close,
}
fn service(client: &Client) -> impl iced::futures::Stream<Item = Message> + use<> {
    let client = client.clone();
    iced::stream::channel(
        1,
        move |mut output: iced::futures::channel::mpsc::Sender<Message>| async move {
            let (sender, mut requests) = mpsc::channel(8);
            if output.send(Message::Connected(sender)).await.is_err() {
                return;
            }
            loop {
                let mut events = client.subscribe();
                let initial = if events.is_ok() {
                    client.snapshot().await
                } else {
                    Err(Error::Unavailable)
                };
                // Commands from a lost connection must never be replayed after reconnect.
                while requests.try_recv().is_ok() {
                    if output
                        .send(Message::Done(Err(Error::Disconnected)))
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
                if output
                    .send(Message::Snapshot(initial.clone()))
                    .await
                    .is_err()
                {
                    return;
                }
                if let (Ok(events), Ok(mut snapshot)) = (&mut events, initial) {
                    loop {
                        let result = tokio::select! {
                            event = events.next() => {
                                if let Err(error) = event { Err(error) } else {
                                    // Bound refresh frequency during bursts; no timer runs while healthy/idle.
                                    tokio::time::sleep(Duration::from_millis(100)).await;
                                    client.snapshot().await
                                }
                            }
                            request = requests.recv() => {
                                let Some(request) = request else { return; };
                                let result = match request {
                                    Request::Apply(command) => client.apply(&snapshot, &command).await,
                                    Request::Refresh => Ok(()),
                                };
                                let refreshed = client.snapshot().await;
                                if output.send(Message::Done(result)).await.is_err() { return; }
                                refreshed
                            }
                        };
                        if output
                            .send(Message::Snapshot(result.clone()))
                            .await
                            .is_err()
                        {
                            return;
                        }
                        match result {
                            Ok(current) => snapshot = current,
                            Err(_) => break,
                        }
                    }
                }
                if let Ok(events) = events {
                    events.close().await;
                }
                let delay = tokio::time::sleep(Duration::from_secs(2));
                tokio::pin!(delay);
                loop {
                    tokio::select! {
                        _ = &mut delay => break,
                        request = requests.recv() => {
                            if request.is_none() { return; }
                            if output.send(Message::Done(Err(Error::Disconnected))).await.is_err() { return; }
                        }
                    }
                }
            }
        },
    )
}

pub(crate) struct Audio {
    client: Client,
    sender: Option<mpsc::Sender<Request>>,
    snapshot: Option<Snapshot>,
    pending: bool,
    error: Option<Error>,
    draft: Option<(Kind, String, u8)>,
}
impl Audio {
    pub(crate) fn new(client: Client) -> Self {
        Self {
            client,
            sender: None,
            snapshot: None,
            pending: false,
            error: None,
            draft: None,
        }
    }
    pub(crate) fn subscription(&self) -> Subscription<Message> {
        Subscription::run_with(self.client.clone(), service)
    }
    /// True asks the host to close only this panel.
    pub(crate) fn update(&mut self, message: Message) -> bool {
        match message {
            Message::Connected(sender) => self.sender = Some(sender),
            Message::Snapshot(Ok(snapshot)) => {
                if self.snapshot.is_none() {
                    self.error = None;
                }
                self.draft = self.draft.take().filter(|(kind, name, _)| {
                    snapshot
                        .devices
                        .iter()
                        .any(|d| d.kind == *kind && d.name == *name)
                });
                self.snapshot = Some(snapshot);
            }
            Message::Snapshot(Err(error)) => {
                self.draft = None;
                self.snapshot = None;
                self.pending = false;
                self.error = Some(error);
            }
            Message::Done(result) => {
                self.pending = false;
                self.error = result.err();
            }
            Message::Volume(kind, name, value) if !self.pending => {
                self.draft = Some((kind, name, value.min(100)))
            }
            Message::CommitVolume(kind, name) => {
                if let Some((k, n, value)) = self.draft.take()
                    && k == kind
                    && n == name
                {
                    return self.update(Message::Apply(Command {
                        kind,
                        name,
                        operation: Operation::Volume(value),
                    }));
                }
            }
            Message::Close => {
                self.draft = None;
                return true;
            }
            Message::Apply(command) if !self.pending => {
                if self
                    .snapshot
                    .as_ref()
                    .is_some_and(|s| s.arguments(&command).is_ok())
                {
                    self.send(Request::Apply(command));
                } else {
                    self.error = Some(Error::InvalidCommand);
                }
            }
            Message::Refresh if !self.pending => self.send(Request::Refresh),
            _ => (),
        }
        false
    }
    pub(crate) fn close(&mut self) {
        self.draft = None;
    }
    pub(crate) fn label(&self) -> String {
        match self.snapshot.as_ref().and_then(|s| {
            s.devices
                .iter()
                .find(|d| d.kind == Kind::Output && s.is_default(d))
        }) {
            Some(d) if d.muted => "Audio muted".into(),
            Some(d) => format!("Audio {}%", d.percent),
            None => "Audio ?".into(),
        }
    }
    fn send(&mut self, request: Request) {
        if self
            .sender
            .as_ref()
            .is_some_and(|s| s.try_send(request).is_ok())
        {
            self.pending = true;
            self.error = None;
        } else {
            self.error = Some(Error::Unavailable);
        }
    }
    pub(crate) fn view(&self, config: &cabby_config::Config) -> Element<'_, Message> {
        let mut contents = column![
            row![
                crate::ui::body(config, "Audio")
                    .size(24)
                    .width(Length::Fill),
                button("Close").on_press(Message::Close)
            ],
            crate::ui::body(config, "Existing apps may keep their current device."),
        ]
        .spacing(12);
        if let Some(error) = self.error {
            contents = contents.push(crate::ui::body(config, error.to_string()));
        }
        if let Some(snapshot) = &self.snapshot {
            for kind in [Kind::Output, Kind::Input] {
                contents = contents.push(
                    crate::ui::body(
                        config,
                        if kind == Kind::Output {
                            "Output"
                        } else {
                            "Input"
                        },
                    )
                    .size(20),
                );
                let mut found = false;
                for device in snapshot.devices.iter().filter(|d| d.kind == kind) {
                    found = true;
                    let command = |operation| {
                        Message::Apply(Command {
                            kind,
                            name: device.name.clone(),
                            operation,
                        })
                    };
                    let action = |label: &'static str, operation, enabled: bool| {
                        let b = button(label);
                        if self.pending || !enabled {
                            b
                        } else {
                            b.on_press(command(operation))
                        }
                    };
                    let label = if snapshot.is_default(device) {
                        format!("✓ {} · selected", device.description)
                    } else {
                        device.description.clone()
                    };
                    let value = self
                        .draft
                        .as_ref()
                        .filter(|(k, n, _)| *k == kind && n == &device.name)
                        .map(|(_, _, v)| *v)
                        .unwrap_or(device.percent.min(100) as u8);
                    let name = device.name.clone();
                    let volume = slider(0..=100, value, move |v| {
                        Message::Volume(kind, name.clone(), v)
                    })
                    .on_release(Message::CommitVolume(kind, device.name.clone()));
                    contents = contents.push(
                        column![
                            crate::ui::body(config, label),
                            volume,
                            row![
                                action("Use", Operation::Default, !snapshot.is_default(device)),
                                action(
                                    if device.muted { "Unmute" } else { "Mute" },
                                    Operation::Mute(!device.muted),
                                    true
                                ),
                                action(
                                    "−",
                                    Operation::Volume(
                                        device.percent.saturating_sub(5).min(100) as u8
                                    ),
                                    device.percent > 0
                                ),
                                crate::ui::body(config, format!("{}%", device.percent)),
                                action(
                                    "+",
                                    Operation::Volume(
                                        device.percent.saturating_add(5).min(100) as u8
                                    ),
                                    device.percent < 100
                                ),
                            ]
                            .spacing(6)
                            .align_y(iced::Alignment::Center)
                        ]
                        .spacing(6),
                    );
                }
                if !found {
                    contents = contents.push(crate::ui::body(config, "No devices available"));
                }
            }
        } else {
            contents = contents.push(crate::ui::body(config, "Connecting to audio…"));
        }
        let refresh = button(if self.pending {
            "Working…"
        } else {
            "Refresh"
        });
        contents = contents.push(if self.pending {
            refresh
        } else {
            refresh.on_press(Message::Refresh)
        });
        container(scrollable(contents))
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
    use iced::futures::StreamExt;
    use std::{fs, os::unix::fs::PermissionsExt};
    fn fixture() -> (tempfile::TempDir, Client) {
        let dir = tempfile::tempdir().unwrap();
        let source =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/fixtures");
        let helper = dir.path().join("audio-helper.py");
        fs::copy(source.join("audio-helper.py"), &helper).unwrap();
        fs::set_permissions(&helper, fs::Permissions::from_mode(0o700)).unwrap();
        fs::copy(
            source.join("audio-state.json"),
            dir.path().join("audio-state.json"),
        )
        .unwrap();
        (dir, Client::with_executable(helper))
    }
    async fn next(
        stream: &mut std::pin::Pin<Box<impl iced::futures::Stream<Item = Message>>>,
    ) -> Message {
        tokio::time::timeout(Duration::from_secs(12), stream.next())
            .await
            .unwrap()
            .unwrap()
    }
    #[tokio::test]
    async fn worker_commands_events_reconnect_and_shutdown() {
        let (dir, client) = fixture();
        let mut stream = Box::pin(service(&client));
        let Message::Connected(sender) = next(&mut stream).await else {
            panic!("missing channel")
        };
        let Message::Snapshot(Ok(snapshot)) = next(&mut stream).await else {
            panic!("missing snapshot from fixture")
        };
        assert_eq!(snapshot.devices.len(), 3);
        sender
            .send(Request::Apply(Command {
                kind: Kind::Output,
                name: "fixture.headphones".into(),
                operation: Operation::Default,
            }))
            .await
            .unwrap();
        let mut done = false;
        loop {
            match next(&mut stream).await {
                Message::Done(Ok(())) => done = true,
                Message::Snapshot(Ok(s))
                    if done && s.default_output.as_deref() == Some("fixture.headphones") =>
                {
                    break;
                }
                _ => (),
            }
        }
        for (kind, name, operation, reject) in [
            (
                Kind::Output,
                "fixture.headphones",
                Operation::Volume(65),
                false,
            ),
            (Kind::Input, "fixture.mic", Operation::Mute(false), false),
            (
                Kind::Output,
                "fixture.headphones",
                Operation::Volume(80),
                true,
            ),
        ] {
            if reject {
                fs::write(dir.path().join("audio-reject"), "").unwrap();
            }
            sender
                .send(Request::Apply(Command {
                    kind,
                    name: name.into(),
                    operation,
                }))
                .await
                .unwrap();
            let mut acknowledged = false;
            loop {
                match next(&mut stream).await {
                    Message::Done(result) => {
                        assert_eq!(result.is_err(), reject);
                        acknowledged = true;
                    }
                    Message::Snapshot(Ok(s)) if acknowledged => {
                        let device = s.devices.iter().find(|d| d.name == name).unwrap();
                        if kind == Kind::Input {
                            assert!(!device.muted);
                        } else {
                            assert_eq!(device.percent, 65);
                        }
                        break;
                    }
                    _ => (),
                }
            }
            if reject {
                fs::remove_file(dir.path().join("audio-reject")).unwrap();
            }
        }
        // External changes are observed without requesting a refresh.
        let path = dir.path().join("audio-state.json");
        let mut state: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        state["version"] = 20.into();
        state["sinks"][0]["mute"] = true.into();
        let tmp = dir.path().join("external.tmp");
        fs::write(&tmp, state.to_string()).unwrap();
        fs::rename(tmp, path).unwrap();
        loop {
            if let Message::Snapshot(Ok(s)) = next(&mut stream).await
                && s.devices[0].muted
            {
                break;
            }
        }
        fs::write(dir.path().join("audio-offline"), "").unwrap();
        loop {
            if matches!(next(&mut stream).await, Message::Snapshot(Err(_))) {
                break;
            }
        }
        fs::remove_file(dir.path().join("audio-offline")).unwrap();
        loop {
            if matches!(next(&mut stream).await, Message::Snapshot(Ok(_))) {
                break;
            }
        }
        drop(stream);
        drop(sender);
        let pids = fs::read_to_string(dir.path().join("audio-pids")).unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            while pids
                .lines()
                .any(|pid| std::path::Path::new("/proc").join(pid).exists())
            {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("audio subscription leaked helpers");
    }
    #[test]
    fn pending_and_disconnected_controls_do_not_queue_stale_operations() {
        let mut audio = Audio::new(Client::default());
        let (sender, mut receiver) = mpsc::channel(8);
        audio.update(Message::Connected(sender));
        let snapshot = Snapshot {
            devices: vec![cabby_audio::Device {
                kind: Kind::Output,
                name: "fixture".into(),
                description: "Fixture".into(),
                percent: 50,
                muted: false,
            }],
            ..Default::default()
        };
        audio.update(Message::Snapshot(Ok(snapshot)));
        let command = Command {
            kind: Kind::Output,
            name: "fixture".into(),
            operation: Operation::Volume(55),
        };
        audio.update(Message::Volume(Kind::Output, "fixture".into(), 40));
        audio.update(Message::Volume(Kind::Output, "fixture".into(), 55));
        assert!(receiver.try_recv().is_err()); // dragging never sends device commands
        audio.update(Message::CommitVolume(Kind::Output, "fixture".into()));
        audio.update(Message::Apply(command.clone()));
        assert!(receiver.try_recv().is_ok());
        assert!(receiver.try_recv().is_err());
        audio.update(Message::Snapshot(Err(Error::Disconnected)));
        audio.update(Message::Apply(command));
        assert!(receiver.try_recv().is_err());
        assert!(audio.update(Message::Close));
    }
}
