//! Bluetooth controls and pairing interaction, independent of surface ownership.
use cabby_bluetooth::{Command, Endpoint, PromptKind, Reply, State};
use iced::{
    Element, Length, Subscription,
    futures::SinkExt,
    widget::{button, column, container, row, scrollable, text_input},
};
use tokio::sync::{mpsc, watch};
#[derive(Clone)]
pub(crate) enum Message {
    Ready(mpsc::Sender<Command>),
    State(State),
    Action(Command),
    Input(String),
    Submit,
    Remove(String),
    ConfirmRemove,
    KeepDevice,
    Close,
}
impl std::fmt::Debug for Message {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("BluetoothMessage(redacted)")
    }
}
fn service(endpoint: &Endpoint) -> impl iced::futures::Stream<Item = Message> + use<> {
    let endpoint = endpoint.clone();
    iced::stream::channel(
        1,
        move |mut output: iced::futures::channel::mpsc::Sender<Message>| async move {
            let (sender, commands) = mpsc::channel(8);
            let (states, mut snapshots) = watch::channel(State::default());
            if output.send(Message::Ready(sender)).await.is_err() {
                return;
            }
            let worker = cabby_bluetooth::run(endpoint, commands, states);
            tokio::pin!(worker);
            loop {
                tokio::select! {
                    _ = &mut worker => return,
                    result = snapshots.changed() => {
                        if result.is_err(){return;}
                        let state=snapshots.borrow_and_update().clone();
                        if output.send(Message::State(state)).await.is_err(){return;}
                    }
                }
            }
        },
    )
}
pub(crate) struct Bluetooth {
    endpoint: Endpoint,
    sender: Option<mpsc::Sender<Command>>,
    state: State,
    input: String,
    remove: Option<String>,
    submitted: bool,
}
impl Bluetooth {
    pub(crate) fn new(endpoint: Endpoint) -> Self {
        Self {
            endpoint,
            sender: None,
            state: State::default(),
            input: String::new(),
            remove: None,
            submitted: false,
        }
    }
    pub(crate) fn subscription(&self) -> Subscription<Message> {
        Subscription::run_with(self.endpoint.clone(), service)
    }
    pub(crate) fn update(&mut self, message: Message) -> bool {
        match message {
            Message::Ready(sender) => self.sender = Some(sender),
            Message::State(state) => {
                if state.prompt.as_ref().map(|p| p.id) != self.state.prompt.as_ref().map(|p| p.id) {
                    self.input.clear();
                }
                if !state.available
                    || self
                        .remove
                        .as_ref()
                        .is_some_and(|path| !state.devices.iter().any(|d| &d.path == path))
                {
                    self.remove = None;
                }
                self.submitted = false;
                self.state = state;
            }
            Message::Input(input) => {
                self.input = input
                    .chars()
                    .filter(|c| c.is_ascii_alphanumeric())
                    .take(16)
                    .collect()
            }
            Message::Submit => {
                if let Some(prompt) = &self.state.prompt
                    && valid_input(&prompt.kind, &self.input)
                {
                    let command =
                        Command::Reply(prompt.id, Reply::Text(std::mem::take(&mut self.input)));
                    self.send(command);
                }
            }
            Message::Action(command) => {
                let interactive = matches!(command, Command::Reply(..) | Command::CancelPair);
                if interactive || (!self.state.busy && !self.submitted && self.state.available) {
                    self.send(command);
                }
            }
            Message::Remove(path)
                if !self.state.busy && self.state.devices.iter().any(|d| d.path == path) =>
            {
                self.remove = Some(path)
            }
            Message::ConfirmRemove if !self.state.busy && !self.submitted => {
                if let Some(path) = self.remove.take() {
                    self.send(Command::Remove(path));
                }
            }
            Message::KeepDevice => self.remove = None,
            Message::Close => return true,
            _ => (),
        }
        false
    }
    fn send(&mut self, command: Command) {
        if self
            .sender
            .as_ref()
            .is_some_and(|s| s.try_send(command).is_ok())
        {
            self.submitted = true;
        } else {
            self.state.error = Some(cabby_bluetooth::Error::Unavailable);
        }
    }
    pub(crate) fn view(&self, config: &cabby_config::Config) -> Element<'_, Message> {
        let enabled = self.state.available && !self.state.busy && !self.submitted;
        let action = |label: &'static str, command, allow: bool| {
            let b = button(label);
            if enabled && allow {
                b.on_press(Message::Action(command))
            } else {
                b
            }
        };
        let header = row![
            crate::ui::body(config, "Bluetooth")
                .size(24)
                .width(Length::Fill),
            button("Close").on_press(Message::Close)
        ];
        let mut contents = column![].spacing(12).padding(iced::Padding {
            right: 12.0,
            ..Default::default()
        });
        if let Some(error) = self.state.error {
            contents = contents.push(crate::ui::body(config, error.to_string()));
        }
        if let Some(prompt) = &self.state.prompt {
            let name = self
                .state
                .devices
                .iter()
                .find(|d| d.path == prompt.device)
                .map(|d| d.name.as_str())
                .unwrap_or("Device");
            contents = contents.push(crate::ui::body(config, format!("Pair with {name}")).size(20));
            match &prompt.kind {
                PromptKind::Confirm(key) => {
                    contents = contents
                        .push(crate::ui::body(
                            config,
                            format!("Does {key:06} match the code on the device?"),
                        ))
                        .push(
                            button("Codes match").on_press(Message::Action(Command::Reply(
                                prompt.id,
                                Reply::Accept,
                            ))),
                        )
                }
                PromptKind::Authorize => {
                    contents =
                        contents
                            .push(crate::ui::body(config, "Allow this pairing request?"))
                            .push(button("Allow pairing").on_press(Message::Action(
                                Command::Reply(prompt.id, Reply::Accept),
                            )))
                }
                PromptKind::Display(value) => {
                    contents = contents.push(crate::ui::body(config, value))
                }
                PromptKind::Pin | PromptKind::Passkey => {
                    let placeholder = if matches!(prompt.kind, PromptKind::Pin) {
                        "PIN (1–16 letters or digits)"
                    } else {
                        "Passkey (up to six digits)"
                    };
                    let input = text_input(placeholder, &self.input)
                        .secure(true)
                        .on_input(Message::Input);
                    let submit = button("Submit");
                    contents =
                        contents
                            .push(input)
                            .push(if valid_input(&prompt.kind, &self.input) {
                                submit.on_press(Message::Submit)
                            } else {
                                submit
                            });
                }
            }
        }
        if self.state.pairing.is_some() {
            contents = contents
                .push(crate::ui::body(config, "Pairing…"))
                .push(button("Cancel pairing").on_press(Message::Action(Command::CancelPair)));
        }
        if let Some(path) = &self.remove
            && let Some(device) = self.state.devices.iter().find(|d| &d.path == path)
        {
            contents = contents
                .push(crate::ui::body(
                    config,
                    format!("Forget {} and remove its pairing information?", device.name),
                ))
                .push(
                    row![
                        button("Forget device").on_press(Message::ConfirmRemove),
                        button("Keep device").on_press(Message::KeepDevice)
                    ]
                    .spacing(8),
                );
        }
        if !self.state.available {
            contents = contents.push(crate::ui::body(config, "Waiting for Bluetooth…"));
        } else if self.state.adapters.is_empty() {
            contents = contents.push(crate::ui::body(config, "No Bluetooth adapters available"));
        }
        for adapter in &self.state.adapters {
            contents = contents
                .push(crate::ui::body(config, &adapter.name).size(20))
                .push(
                    row![
                        action(
                            if adapter.powered {
                                "Power off"
                            } else {
                                "Power on"
                            },
                            Command::Power(adapter.path.clone(), !adapter.powered),
                            true
                        ),
                        action(
                            "Find devices",
                            Command::Scan(adapter.path.clone()),
                            adapter.powered && self.state.scanning.is_none()
                        ),
                    ]
                    .spacing(8),
                );
            if self.state.scanning.as_deref() == Some(&adapter.path) {
                contents = contents
                    .push(crate::ui::body(config, "Searching for up to 60 seconds…"))
                    .push(action("Stop searching", Command::StopScan, true));
            }
            let mut count = 0;
            for device in self
                .state
                .devices
                .iter()
                .filter(|d| d.adapter == adapter.path)
            {
                count += 1;
                let status = if device.connected {
                    "connected"
                } else if device.paired {
                    "paired"
                } else {
                    "not paired"
                };
                let mut controls = row![action(
                    if device.connected {
                        "Disconnect"
                    } else {
                        "Connect"
                    },
                    if device.connected {
                        Command::Disconnect(device.path.clone())
                    } else {
                        Command::Connect(device.path.clone())
                    },
                    adapter.powered && device.paired
                )]
                .spacing(6);
                if !device.paired {
                    controls = controls.push(action(
                        "Pair",
                        Command::Pair(device.path.clone()),
                        adapter.powered,
                    ));
                }
                let remove = button("Forget");
                controls = controls.push(if enabled {
                    remove.on_press(Message::Remove(device.path.clone()))
                } else {
                    remove
                });
                contents = contents
                    .push(crate::ui::body(
                        config,
                        format!("{} · {status}", device.name),
                    ))
                    .push(controls);
            }
            if count == 0 {
                contents = contents.push(crate::ui::body(
                    config,
                    "No known devices. Use Find devices to search.",
                ));
            }
        }
        contents = contents.push(action(
            if self.state.busy {
                "Working…"
            } else {
                "Refresh"
            },
            Command::Refresh,
            true,
        ));
        container(column![header, scrollable(contents).height(Length::Fill)].spacing(12))
            .padding(16)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(container::rounded_box)
            .into()
    }
}
fn valid_input(kind: &PromptKind, input: &str) -> bool {
    match kind {
        PromptKind::Pin => {
            !input.is_empty()
                && input.len() <= 16
                && input.bytes().all(|c| c.is_ascii_alphanumeric())
        }
        PromptKind::Passkey => {
            !input.is_empty() && input.len() <= 6 && input.bytes().all(|c| c.is_ascii_digit())
        }
        _ => false,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pairing_input_and_removal_require_explicit_valid_actions() {
        let mut plugin = Bluetooth::new(Endpoint::System);
        let (tx, mut rx) = mpsc::channel(8);
        plugin.update(Message::Ready(tx));
        plugin.update(Message::State(State {
            available: true,
            devices: vec![cabby_bluetooth::Device {
                path: "/device".into(),
                ..Default::default()
            }],
            ..Default::default()
        }));
        plugin.update(Message::Remove("/device".into()));
        assert!(rx.try_recv().is_err());
        plugin.update(Message::KeepDevice);
        plugin.update(Message::ConfirmRemove);
        assert!(rx.try_recv().is_err());
        plugin.update(Message::Remove("/device".into()));
        plugin.update(Message::ConfirmRemove);
        assert!(matches!(rx.try_recv(), Ok(Command::Remove(_))));
        assert!(!valid_input(&PromptKind::Passkey, "1234567"));
        assert!(!valid_input(&PromptKind::Passkey, "abc"));
        assert!(valid_input(&PromptKind::Passkey, "000123"));
        plugin.state.prompt = Some(cabby_bluetooth::Prompt {
            id: 1,
            device: "/device".into(),
            kind: PromptKind::Pin,
        });
        plugin.update(Message::Input("1234".into()));
        plugin.update(Message::Submit);
        assert!(plugin.input.is_empty());
        assert!(matches!(
            rx.try_recv(),
            Ok(Command::Reply(1, Reply::Text(_)))
        ));
        plugin.update(Message::State(State::default()));
        assert!(plugin.remove.is_none());
        assert!(plugin.input.is_empty());
        assert!(plugin.update(Message::Close));
    }
}
