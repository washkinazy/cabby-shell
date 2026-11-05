//! Explicit-address D-Bus adapter. No session discovery, activation, or detached tasks.
use cabby_notifications::{Entry, Event, Request, Store, Token, Urgency};
use futures_util::StreamExt;
use std::{
    collections::HashMap,
    future::pending,
    time::{Duration, Instant},
};
use tokio::sync::{mpsc, oneshot, watch};
use zbus::{
    Connection, MatchRule, Message, MessageStream,
    connection::Builder,
    fdo::{DBusProxy, RequestNameFlags, RequestNameReply},
    message::{Flags, Type},
    zvariant::OwnedValue,
};

pub const NAME: &str = "org.freedesktop.Notifications";
pub const PATH: &str = "/org/freedesktop/Notifications";
pub const MAX_REQUEST_BYTES: usize = 64 * 1024;
const TIMEOUT: Duration = Duration::from_secs(5);
const XML: &str = include_str!("interface.xml");

/// Content-free errors: never forward transport diagnostics containing a request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Conflict,
    Transport,
    Timeout,
    Busy,
    Stopped,
    Policy(cabby_notifications::Error),
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "notification service: {self:?}")
    }
}
impl std::error::Error for Error {}

pub enum Command {
    Presented(Token),
    Dismiss(Token),
    Invoke(Token, String),
}
struct Envelope {
    command: Command,
    reply: oneshot::Sender<Result<(), Error>>,
}

/// In-process UI bridge. Snapshots coalesce; commands use a bounded queue.
pub struct Handle {
    commands: mpsc::Sender<Envelope>,
    pub snapshots: watch::Receiver<Vec<Entry>>,
}
impl Handle {
    pub async fn command(&self, command: Command) -> Result<(), Error> {
        if let Command::Invoke(_, key) = &command
            && key.len() > cabby_notifications::MAX_TEXT_BYTES
        {
            return Err(Error::Policy(cabby_notifications::Error::InvalidActions));
        }
        let (reply, receive) = oneshot::channel();
        self.commands
            .try_send(Envelope { command, reply })
            .map_err(|e| match e {
                mpsc::error::TrySendError::Full(_) => Error::Busy,
                mpsc::error::TrySendError::Closed(_) => Error::Stopped,
            })?;
        tokio::time::timeout(TIMEOUT, receive)
            .await
            .map_err(|_| Error::Timeout)?
            .map_err(|_| Error::Stopped)?
    }
}

pub struct Service {
    connection: Connection,
    messages: MessageStream,
    store: Store,
    commands: mpsc::Receiver<Envelope>,
    snapshots: watch::Sender<Vec<Entry>>,
}
impl Service {
    /// Call only with an explicitly chosen bus. Success means exclusive ownership
    /// is acquired and the receive stream is installed, not that a UI is visible.
    pub async fn connect(address: &str) -> Result<(Self, Handle), Error> {
        tokio::time::timeout(TIMEOUT, Self::connect_inner(address))
            .await
            .map_err(|_| Error::Timeout)?
    }
    async fn connect_inner(address: &str) -> Result<(Self, Handle), Error> {
        let connection = Builder::address(address)
            .map_err(|_| Error::Transport)?
            .max_queued(1)
            .method_timeout(TIMEOUT)
            .build()
            .await
            .map_err(|_| Error::Transport)?;
        // Avoid ObjectServer's per-call task spawning; this adapter dispatches serially.
        let rule = MatchRule::builder().msg_type(Type::MethodCall).build();
        let messages = MessageStream::for_match_rule(rule, &connection, Some(1))
            .await
            .map_err(|_| Error::Transport)?;
        let proxy = DBusProxy::new(&connection)
            .await
            .map_err(|_| Error::Transport)?;
        let result = proxy
            .request_name(
                NAME.try_into().map_err(|_| Error::Transport)?,
                RequestNameFlags::DoNotQueue.into(),
            )
            .await
            .map_err(|_| Error::Transport)?;
        if result != RequestNameReply::PrimaryOwner {
            return Err(Error::Conflict);
        }
        let (sender, commands) = mpsc::channel(8);
        let (snapshots, receiver) = watch::channel(Vec::new());
        Ok((
            Self {
                connection,
                messages,
                store: Store::default(),
                commands,
                snapshots,
            },
            Handle {
                commands: sender,
                snapshots: receiver,
            },
        ))
    }
    /// Run in the owning UI task. Drop the handle for orderly shutdown; bus loss
    /// returns an error for the future supervisor. Always clears retained snapshots.
    pub async fn run(mut self) -> Result<(), Error> {
        let result = self.run_inner().await;
        self.store = Store::default();
        self.snapshots.send_replace(Vec::new());
        let _ = tokio::time::timeout(TIMEOUT, self.connection.clone().close()).await;
        result
    }
    async fn run_inner(&mut self) -> Result<(), Error> {
        loop {
            let deadline = self.store.next_deadline();
            tokio::select! {
                _ = self.connection.closed() => return Err(Error::Transport),
                message = self.messages.next() => {
                    let message = message.ok_or(Error::Transport)?.map_err(|_| Error::Transport)?;
                    tokio::time::timeout(TIMEOUT, self.dispatch(&message)).await.map_err(|_| Error::Timeout)?.map_err(|_| Error::Transport)?;
                }
                envelope = self.commands.recv() => {
                    let Some(envelope) = envelope else { return Ok(()); };
                    // A timed-out/cancelled caller must not trigger a queued action.
                    if envelope.reply.is_closed() { continue; }
                    let result = tokio::time::timeout(TIMEOUT, self.command(envelope.command)).await.map_err(|_| Error::Timeout)?;
                    let failed_transport = result == Err(Error::Transport);
                    let _ = envelope.reply.send(result);
                    if failed_transport { return Err(Error::Transport); }
                }
                _ = wait_deadline(deadline) => {
                    if let Some((token, _)) = deadline
                        && let Some(event) = self.store.expire(token, Instant::now()) {
                            self.publish();
                            tokio::time::timeout(TIMEOUT, self.emit(event)).await.map_err(|_| Error::Timeout)?.map_err(|_| Error::Transport)?;
                        }
                }
            }
        }
    }
    fn publish(&self) {
        self.snapshots.send_replace(self.store.entries().to_vec());
    }
    async fn command(&mut self, command: Command) -> Result<(), Error> {
        let events = match command {
            Command::Presented(token) => {
                self.store
                    .presented(token, Instant::now())
                    .map_err(Error::Policy)?;
                Vec::new()
            }
            Command::Dismiss(token) => vec![self.store.dismiss(token).map_err(Error::Policy)?],
            Command::Invoke(token, key) => self.store.invoke(token, &key).map_err(Error::Policy)?,
        };
        self.publish();
        for event in events {
            self.emit(event).await.map_err(|_| Error::Transport)?;
        }
        Ok(())
    }
    async fn emit(&self, event: Event) -> zbus::Result<()> {
        match event {
            Event::Closed { id, reason } => {
                self.connection
                    .emit_signal(
                        None::<&str>,
                        PATH,
                        NAME,
                        "NotificationClosed",
                        &(id, reason as u32),
                    )
                    .await
            }
            Event::ActionInvoked { id, key } => {
                self.connection
                    .emit_signal(None::<&str>, PATH, NAME, "ActionInvoked", &(id, key))
                    .await
            }
        }
    }
    async fn reply<B: serde::Serialize + zbus::zvariant::DynamicType>(
        &self,
        message: &Message,
        body: &B,
    ) -> zbus::Result<()> {
        if message
            .primary_header()
            .flags()
            .contains(Flags::NoReplyExpected)
        {
            return Ok(());
        }
        self.connection.reply(&message.header(), body).await
    }
    async fn error(&self, message: &Message, name: &str) -> zbus::Result<()> {
        if message
            .primary_header()
            .flags()
            .contains(Flags::NoReplyExpected)
        {
            return Ok(());
        }
        self.connection
            .reply_error(&message.header(), name, &"Notification request rejected")
            .await
    }
    async fn dispatch(&mut self, message: &Message) -> zbus::Result<()> {
        if message.message_type() != Type::MethodCall {
            return Ok(());
        }
        if message.data().len() > MAX_REQUEST_BYTES {
            return self
                .error(message, "org.freedesktop.DBus.Error.LimitsExceeded")
                .await;
        }
        let header = message.header();
        if header.path().map(|p| p.as_str()) != Some(PATH) {
            return self
                .error(message, "org.freedesktop.DBus.Error.UnknownObject")
                .await;
        }
        let interface = header.interface().map(|v| v.as_str());
        let member = header.member().map(|v| v.as_str()).unwrap_or("");
        let empty = message.body().deserialize::<()>().is_ok();
        if interface == Some("org.freedesktop.DBus.Introspectable")
            && member == "Introspect"
            && empty
        {
            return self.reply(message, &XML).await;
        }
        if interface == Some("org.freedesktop.DBus.Peer") && member == "Ping" && empty {
            return self.reply(message, &()).await;
        }
        if interface.is_some_and(|i| i != NAME) {
            return self
                .error(message, "org.freedesktop.DBus.Error.UnknownInterface")
                .await;
        }
        match member {
            // No visible UI yet: do not advertise body/actions prematurely.
            "GetCapabilities" if empty => self.reply(message, &Vec::<String>::new()).await,
            "GetServerInformation" if empty => {
                self.reply(
                    message,
                    &("Cabby", "Cabby Shell", env!("CARGO_PKG_VERSION"), "1.3"),
                )
                .await
            }
            "Notify" => {
                type Arguments = (
                    String,
                    u32,
                    String,
                    String,
                    String,
                    Vec<String>,
                    HashMap<String, OwnedValue>,
                    i32,
                );
                let Ok((
                    app_name,
                    replaces_id,
                    _icon,
                    summary,
                    body,
                    actions,
                    hints,
                    expire_timeout,
                )) = message.body().deserialize::<Arguments>()
                else {
                    return self
                        .error(message, "org.freedesktop.DBus.Error.InvalidArgs")
                        .await;
                };
                let urgency = match hints.get("urgency").and_then(|v| u8::try_from(v).ok()) {
                    Some(0) => Urgency::Low,
                    Some(2) => Urgency::Critical,
                    _ => Urgency::Normal,
                };
                let resident = hints
                    .get("resident")
                    .and_then(|v| bool::try_from(v).ok())
                    .unwrap_or(false);
                match self.store.notify(Request {
                    app_name,
                    replaces_id,
                    summary,
                    body,
                    actions,
                    urgency,
                    resident,
                    expire_timeout,
                }) {
                    Ok(token) => {
                        self.publish();
                        self.reply(message, &token.id()).await
                    }
                    Err(
                        cabby_notifications::Error::Capacity
                        | cabby_notifications::Error::RevisionExhausted,
                    ) => {
                        self.error(message, "org.freedesktop.DBus.Error.LimitsExceeded")
                            .await
                    }
                    Err(_) => {
                        self.error(message, "org.freedesktop.DBus.Error.InvalidArgs")
                            .await
                    }
                }
            }
            "CloseNotification" => {
                let Ok(id) = message.body().deserialize::<u32>() else {
                    return self
                        .error(message, "org.freedesktop.DBus.Error.InvalidArgs")
                        .await;
                };
                match self.store.close(id) {
                    Ok(event) => {
                        self.publish();
                        self.emit(event).await?;
                        self.reply(message, &()).await
                    }
                    Err(_) => {
                        self.error(message, "org.freedesktop.DBus.Error.InvalidArgs")
                            .await
                    }
                }
            }
            "GetCapabilities" | "GetServerInformation" => {
                self.error(message, "org.freedesktop.DBus.Error.InvalidArgs")
                    .await
            }
            _ => {
                self.error(message, "org.freedesktop.DBus.Error.UnknownMethod")
                    .await
            }
        }
    }
}
impl Drop for Service {
    fn drop(&mut self) {
        // Also clear the retained view when the owning task is cancelled.
        self.snapshots.send_replace(Vec::new());
    }
}
async fn wait_deadline(deadline: Option<(Token, Instant)>) {
    match deadline {
        Some((_, when)) => tokio::time::sleep_until(when.into()).await,
        None => pending::<()>().await,
    }
}
