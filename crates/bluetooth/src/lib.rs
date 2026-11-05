//! BlueZ client with an application-scoped pairing agent and owned discovery.
//! No widgets, subprocesses, default agent takeover, or persistent device data.
use futures_util::StreamExt;
use std::{collections::HashMap, future::Future, pin::Pin, time::Duration};
use tokio::{
    sync::{mpsc, watch},
    time::Instant,
};
use zbus::{
    Connection, MatchRule, Message, MessageStream, Proxy,
    connection::Builder,
    fdo::{DBusProxy, ManagedObjects},
    message::Type,
    zvariant::{OwnedObjectPath, OwnedValue},
};

pub const AGENT: &str = "/org/cabby/BluetoothAgent";
static PROMPT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
const ADAPTER: &str = "org.bluez.Adapter1";
const DEVICE: &str = "org.bluez.Device1";
const TIMEOUT: Duration = Duration::from_secs(5);
const SCAN_TIME: Duration = Duration::from_secs(60);
const PAIR_TIME: Duration = Duration::from_secs(120);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Unavailable,
    Rejected,
    Timeout,
    InvalidData,
    Busy,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Unavailable => "Bluetooth service unavailable",
            Self::Rejected => "Bluetooth request rejected; check adapter power and permissions",
            Self::Timeout => "Bluetooth request timed out",
            Self::InvalidData => "Bluetooth device data is unavailable or unsupported",
            Self::Busy => "A Bluetooth request is already running",
        })
    }
}
impl std::error::Error for Error {}
type Result<T> = std::result::Result<T, Error>;
#[derive(Clone, Default)]
pub struct Adapter {
    pub path: String,
    pub name: String,
    pub powered: bool,
}
#[derive(Clone, Default)]
pub struct Device {
    pub path: String,
    pub adapter: String,
    pub name: String,
    pub paired: bool,
    pub connected: bool,
    pub battery: Option<u8>,
}
#[derive(Clone, PartialEq, Eq)]
pub enum PromptKind {
    Confirm(u32),
    Pin,
    Passkey,
    Display(String),
    Authorize,
}
#[derive(Clone)]
pub struct Prompt {
    pub id: u64,
    pub device: String,
    pub kind: PromptKind,
}
#[derive(Clone, Default)]
pub struct State {
    pub available: bool,
    pub adapters: Vec<Adapter>,
    pub devices: Vec<Device>,
    pub scanning: Option<String>,
    pub pairing: Option<String>,
    pub busy: bool,
    pub prompt: Option<Prompt>,
    pub error: Option<Error>,
}
impl std::fmt::Debug for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BluetoothState")
            .field("available", &self.available)
            .field("busy", &self.busy)
            .finish_non_exhaustive()
    }
}
#[derive(Clone)]
pub enum Reply {
    Accept,
    Reject,
    Text(String),
}
#[derive(Clone)]
pub enum Command {
    Refresh,
    Power(String, bool),
    Scan(String),
    StopScan,
    Pair(String),
    CancelPair,
    Connect(String),
    Disconnect(String),
    Remove(String),
    Reply(u64, Reply),
}
impl std::fmt::Debug for Command {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("BluetoothCommand(redacted)")
    }
}
#[derive(Clone, Default, Debug, PartialEq, Eq, Hash)]
pub enum Endpoint {
    #[default]
    System,
    Private(String),
}

fn string(properties: &HashMap<String, OwnedValue>, key: &str) -> String {
    properties
        .get(key)
        .and_then(|v| <&str>::try_from(v).ok())
        .unwrap_or("")
        .chars()
        .filter(|c| !c.is_control())
        .take(160)
        .collect()
}
fn boolean(properties: &HashMap<String, OwnedValue>, key: &str) -> bool {
    properties
        .get(key)
        .and_then(|v| bool::try_from(v).ok())
        .unwrap_or(false)
}
fn parse(objects: ManagedObjects) -> Result<(Vec<Adapter>, Vec<Device>)> {
    if objects.len() > 4096 {
        return Err(Error::InvalidData);
    }
    let mut adapters = Vec::new();
    let mut devices = Vec::new();
    for (path, interfaces) in objects {
        if let Some(p) = interfaces.get(ADAPTER) {
            adapters.push(Adapter {
                path: path.to_string(),
                name: string(p, "Alias"),
                powered: boolean(p, "Powered"),
            });
        }
        if let Some(p) = interfaces.get(DEVICE) {
            let adapter = p
                .get("Adapter")
                .and_then(|v| <&zbus::zvariant::ObjectPath<'_>>::try_from(v).ok())
                .ok_or(Error::InvalidData)?
                .to_string();
            let name = string(p, "Alias");
            devices.push(Device {
                path: path.to_string(),
                adapter,
                name: if name.is_empty() {
                    "Unnamed device".into()
                } else {
                    name
                },
                paired: boolean(p, "Paired"),
                connected: boolean(p, "Connected"),
                battery: interfaces
                    .get("org.bluez.Battery1")
                    .and_then(|p| p.get("Percentage"))
                    .and_then(|v| u8::try_from(v).ok())
                    .filter(|v| *v <= 100),
            });
        }
    }
    if adapters.len() > 16 || devices.len() > 256 {
        return Err(Error::InvalidData);
    }
    adapters.sort_by(|a, b| a.path.cmp(&b.path));
    devices.sort_by(|a, b| a.name.cmp(&b.name).then(a.path.cmp(&b.path)));
    devices.retain(|d| adapters.iter().any(|a| a.path == d.adapter));
    Ok((adapters, devices))
}
async fn call(
    connection: &Connection,
    owner: &str,
    path: &str,
    interface: &str,
    method: &str,
) -> Result<()> {
    let proxy = Proxy::new(connection, owner, path, interface)
        .await
        .map_err(|_| Error::Unavailable)?;
    proxy.call(method, &()).await.map_err(|_| Error::Rejected)
}
async fn snapshot(connection: &Connection, owner: &str) -> Result<(Vec<Adapter>, Vec<Device>)> {
    let proxy = Proxy::new(connection, owner, "/", "org.freedesktop.DBus.ObjectManager")
        .await
        .map_err(|_| Error::Unavailable)?;
    let objects = proxy
        .call("GetManagedObjects", &())
        .await
        .map_err(|_| Error::Unavailable)?;
    parse(objects)
}
async fn wait(deadline: Option<Instant>) {
    if let Some(deadline) = deadline {
        tokio::time::sleep_until(deadline).await;
    } else {
        std::future::pending::<()>().await;
    }
}
type Operation = Pin<Box<dyn Future<Output = Result<()>> + Send>>;
struct Session {
    connection: Connection,
    owner: String,
    messages: MessageStream,
    changes: MessageStream,
    owners: MessageStream,
    state: State,
    scan_deadline: Option<Instant>,
    pending: Option<Message>,
    interactive: bool,
}
impl Session {
    async fn connect(endpoint: &Endpoint, interactive: bool) -> Result<Self> {
        let builder = match endpoint {
            Endpoint::System => Builder::system(),
            Endpoint::Private(address) => Builder::address(address.as_str()),
        }
        .map_err(|_| Error::Unavailable)?;
        let connection = builder
            .max_queued(16)
            .method_timeout(PAIR_TIME)
            .build()
            .await
            .map_err(|_| Error::Unavailable)?;
        let owners = MessageStream::for_match_rule(
            MatchRule::builder()
                .msg_type(Type::Signal)
                .sender("org.freedesktop.DBus")
                .unwrap()
                .interface("org.freedesktop.DBus")
                .unwrap()
                .member("NameOwnerChanged")
                .unwrap()
                .add_arg("org.bluez")
                .unwrap()
                .build(),
            &connection,
            Some(8),
        )
        .await
        .map_err(|_| Error::Unavailable)?;
        let owner = DBusProxy::new(&connection)
            .await
            .map_err(|_| Error::Unavailable)?
            .get_name_owner("org.bluez".try_into().unwrap())
            .await
            .map_err(|_| Error::Unavailable)?
            .to_string();
        let changes = MessageStream::for_match_rule(
            MatchRule::builder()
                .msg_type(Type::Signal)
                .sender(owner.as_str())
                .map_err(|_| Error::Unavailable)?
                .build(),
            &connection,
            Some(16),
        )
        .await
        .map_err(|_| Error::Unavailable)?;
        let messages = MessageStream::for_match_rule(
            MatchRule::builder().msg_type(Type::MethodCall).build(),
            &connection,
            Some(8),
        )
        .await
        .map_err(|_| Error::Unavailable)?;
        if interactive {
            let proxy = Proxy::new(
                &connection,
                owner.as_str(),
                "/org/bluez",
                "org.bluez.AgentManager1",
            )
            .await
            .map_err(|_| Error::Unavailable)?;
            let _: () = proxy
                .call(
                    "RegisterAgent",
                    &(
                        zbus::zvariant::ObjectPath::try_from(AGENT).unwrap(),
                        "KeyboardDisplay",
                    ),
                )
                .await
                .map_err(|_| Error::Rejected)?;
            drop(proxy);
        }
        let (adapters, devices) = snapshot(&connection, &owner).await?;
        Ok(Self {
            connection,
            owner,
            messages,
            changes,
            owners,
            state: State {
                available: true,
                adapters,
                devices,
                ..Default::default()
            },
            scan_deadline: None,
            pending: None,
            interactive,
        })
    }
    async fn reject(&self, message: &Message) {
        let _ = self
            .connection
            .reply_error(
                &message.header(),
                "org.bluez.Error.Rejected",
                &"Pairing request rejected",
            )
            .await;
    }
    async fn clear_prompt(&mut self) {
        if let Some(message) = self.pending.take() {
            self.reject(&message).await;
        }
        self.state.prompt = None;
    }
    async fn agent(&mut self, message: Message) {
        let header = message.header();
        if header.sender().map(|s| s.as_str()) != Some(self.owner.as_str())
            || header.path().map(|p| p.as_str()) != Some(AGENT)
            || header.interface().map(|i| i.as_str()) != Some("org.bluez.Agent1")
            || message.body().len() > 4096
        {
            self.reject(&message).await;
            return;
        }
        let member = header.member().map(|m| m.as_str()).unwrap_or("");
        if matches!(member, "Cancel" | "Release") {
            self.clear_prompt().await;
            let _ = self.connection.reply(&header, &()).await;
            if member == "Release" {
                self.state.error = Some(Error::Unavailable);
                self.state.available = false;
            }
            return;
        }
        let decoded: Option<(OwnedObjectPath, PromptKind)> = match member {
            "RequestConfirmation" => message
                .body()
                .deserialize::<(OwnedObjectPath, u32)>()
                .ok()
                .filter(|(_, p)| *p <= 999999)
                .map(|(d, p)| (d, PromptKind::Confirm(p))),
            "RequestPinCode" | "RequestPasskey" | "RequestAuthorization" => message
                .body()
                .deserialize::<(OwnedObjectPath,)>()
                .ok()
                .map(|(d,)| {
                    (
                        d,
                        match member {
                            "RequestPinCode" => PromptKind::Pin,
                            "RequestPasskey" => PromptKind::Passkey,
                            _ => PromptKind::Authorize,
                        },
                    )
                }),
            "DisplayPasskey" => message
                .body()
                .deserialize::<(OwnedObjectPath, u32, u16)>()
                .ok()
                .filter(|(_, p, e)| *p <= 999999 && *e <= 6)
                .map(|(d, p, e)| {
                    (
                        d,
                        PromptKind::Display(format!("Type {p:06} on the device ({e}/6 entered)")),
                    )
                }),
            "DisplayPinCode" => message
                .body()
                .deserialize::<(OwnedObjectPath, String)>()
                .ok()
                .filter(|(_, p)| {
                    !p.is_empty() && p.len() <= 16 && p.chars().all(|c| c.is_ascii_alphanumeric())
                })
                .map(|(d, p)| (d, PromptKind::Display(format!("Type {p} on the device")))),
            _ => None,
        };
        let Some((device, kind)) = decoded else {
            self.reject(&message).await;
            return;
        };
        if self.state.pairing.as_deref() != Some(device.as_str()) {
            self.reject(&message).await;
            return;
        }
        self.clear_prompt().await;
        let id = PROMPT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let display = matches!(kind, PromptKind::Display(_));
        self.state.prompt = Some(Prompt {
            id,
            device: device.to_string(),
            kind,
        });
        if display {
            let _ = self.connection.reply(&header, &()).await;
        } else {
            self.pending = Some(message);
        }
    }
    async fn reply(&mut self, id: u64, response: Reply) {
        let Some(prompt) = &self.state.prompt else {
            return;
        };
        if prompt.id != id {
            return;
        }
        let Some(message) = self.pending.take() else {
            return;
        };
        match (&prompt.kind, response) {
            (PromptKind::Confirm(_) | PromptKind::Authorize, Reply::Accept) => {
                let _ = self.connection.reply(&message.header(), &()).await;
            }
            (PromptKind::Pin, Reply::Text(text))
                if !text.is_empty()
                    && text.len() <= 16
                    && text.chars().all(|c| c.is_ascii_alphanumeric()) =>
            {
                let _ = self.connection.reply(&message.header(), &text).await;
            }
            (PromptKind::Passkey, Reply::Text(text))
                if !text.is_empty()
                    && text.len() <= 6
                    && text.bytes().all(|c| c.is_ascii_digit()) =>
            {
                let _ = self
                    .connection
                    .reply(&message.header(), &text.parse::<u32>().unwrap())
                    .await;
            }
            _ => self.reject(&message).await,
        }
        self.state.prompt = None;
    }
    fn validate(&self, command: &Command) -> bool {
        match command {
            Command::Power(path, _) => self.state.adapters.iter().any(|a| a.path == *path),
            Command::Scan(path) => {
                self.state.scanning.is_none()
                    && self
                        .state
                        .adapters
                        .iter()
                        .any(|a| a.path == *path && a.powered)
            }
            Command::Pair(path)
            | Command::Connect(path)
            | Command::Disconnect(path)
            | Command::Remove(path) => self.state.devices.iter().any(|d| d.path == *path),
            Command::Refresh | Command::StopScan => true,
            _ => false,
        }
    }
    fn operation(&self, command: Command) -> Operation {
        let connection = self.connection.clone();
        let owner = self.owner.clone();
        let scan = self.state.scanning.clone();
        let adapter = match &command {
            Command::Remove(path) => self
                .state
                .devices
                .iter()
                .find(|d| d.path == *path)
                .map(|d| d.adapter.clone()),
            _ => None,
        };
        Box::pin(async move {
            let timeout = if matches!(command, Command::Pair(_)) {
                PAIR_TIME
            } else {
                TIMEOUT
            };
            tokio::time::timeout(timeout, async {
                match command {
                    Command::Refresh => Ok(()),
                    Command::Scan(path) => {
                        call(&connection, &owner, &path, ADAPTER, "StartDiscovery").await
                    }
                    Command::StopScan => {
                        if let Some(path) = scan {
                            call(&connection, &owner, &path, ADAPTER, "StopDiscovery").await
                        } else {
                            Ok(())
                        }
                    }
                    Command::Pair(path) => call(&connection, &owner, &path, DEVICE, "Pair").await,
                    Command::Connect(path) => {
                        call(&connection, &owner, &path, DEVICE, "Connect").await
                    }
                    Command::Disconnect(path) => {
                        call(&connection, &owner, &path, DEVICE, "Disconnect").await
                    }
                    Command::Power(path, powered) => {
                        let proxy = Proxy::new(&connection, owner.as_str(), path.as_str(), ADAPTER)
                            .await
                            .map_err(|_| Error::Unavailable)?;
                        proxy
                            .set_property("Powered", powered)
                            .await
                            .map_err(|_| Error::Rejected)
                    }
                    Command::Remove(path) => {
                        let proxy = Proxy::new(
                            &connection,
                            owner.as_str(),
                            adapter.as_deref().ok_or(Error::InvalidData)?,
                            ADAPTER,
                        )
                        .await
                        .map_err(|_| Error::Unavailable)?;
                        proxy
                            .call(
                                "RemoveDevice",
                                &(OwnedObjectPath::try_from(path)
                                    .map_err(|_| Error::InvalidData)?,),
                            )
                            .await
                            .map_err(|_| Error::Rejected)
                    }
                    _ => Err(Error::InvalidData),
                }
            })
            .await
            .map_err(|_| Error::Timeout)?
        })
    }
    async fn stop_scan(&mut self) -> Result<()> {
        self.scan_deadline = None;
        if let Some(path) = self.state.scanning.take() {
            tokio::time::timeout(
                TIMEOUT,
                call(
                    &self.connection,
                    &self.owner,
                    &path,
                    ADAPTER,
                    "StopDiscovery",
                ),
            )
            .await
            .map_err(|_| Error::Timeout)??;
        }
        Ok(())
    }
    async fn cancel_pair(&mut self) {
        self.clear_prompt().await;
        if let Some(path) = self.state.pairing.take() {
            let _ = tokio::time::timeout(
                TIMEOUT,
                call(
                    &self.connection,
                    &self.owner,
                    &path,
                    DEVICE,
                    "CancelPairing",
                ),
            )
            .await;
        }
    }
    async fn run(
        &mut self,
        commands: &mut mpsc::Receiver<Command>,
        states: &watch::Sender<State>,
    ) -> Result<()> {
        let mut operation: Option<(Command, Operation)> = None;
        loop {
            states.send_replace(self.state.clone());
            tokio::select! {
                _ = self.connection.closed() => return Err(Error::Unavailable),
                _ = self.owners.next() => return Err(Error::Unavailable),
                message = self.messages.next() => {
                    let message = message.ok_or(Error::Unavailable)?.map_err(|_| Error::Unavailable)?;
                    self.agent(message).await;
                    if !self.state.available { return Err(Error::Unavailable); }
                }
                message = self.changes.next() => {
                    let message = message.ok_or(Error::Unavailable)?.map_err(|_| Error::Unavailable)?;
                    if matches!(message.header().interface().map(|i|i.as_str()), Some("org.freedesktop.DBus.Properties" | "org.freedesktop.DBus.ObjectManager")) {
                        let (adapters,devices) = tokio::time::timeout(TIMEOUT,snapshot(&self.connection,&self.owner)).await.map_err(|_| Error::Timeout)??;
                        self.state.adapters = adapters; self.state.devices = devices;
                    }
                }
                _ = wait(self.scan_deadline) => {
                    // A failed stop closes the entire connection, releasing our discovery lease.
                    self.stop_scan().await?;
                }
                result = async { match operation.as_mut() { Some((_,f)) => f.await, None => std::future::pending().await } } => {
                    let (command,_) = operation.take().unwrap();
                    if matches!(command,Command::Pair(_)) {
                        if result.is_err() { self.cancel_pair().await; } else { self.state.pairing = None; self.clear_prompt().await; }
                    }
                    if let Command::Scan(path) = &command {
                        result?; // uncertain lease: disconnect on failure
                        self.state.scanning = Some(path.clone()); self.scan_deadline = Some(Instant::now()+SCAN_TIME);
                    }
                    if matches!(command,Command::StopScan) {
                        result?;
                        self.state.scanning = None; self.scan_deadline = None;
                    }
                    self.state.busy = false; self.state.error = result.err();
                    let (adapters,devices) = tokio::time::timeout(TIMEOUT,snapshot(&self.connection,&self.owner)).await.map_err(|_| Error::Timeout)??;
                    self.state.adapters = adapters; self.state.devices = devices;
                }
                command = commands.recv() => {
                    let Some(command) = command else { return Ok(()); };
                    if !self.interactive { self.state.error = Some(Error::Rejected); continue; }
                    match command {
                        Command::Reply(id,response) => self.reply(id,response).await,
                        Command::CancelPair if self.state.pairing.is_some() => { operation = None; self.cancel_pair().await; self.state.busy = false; }
                        command if operation.is_some() => { let _ = command; self.state.error = Some(Error::Busy); }
                        command if self.validate(&command) => {
                            if let Command::Pair(path) = &command { self.state.pairing = Some(path.clone()); }
                            self.state.error = None; self.state.busy = true;
                            operation = Some((command.clone(),self.operation(command)));
                        }
                        _ => self.state.error = Some(Error::InvalidData),
                    }
                }
            }
        }
    }
    async fn shutdown(mut self) {
        self.cancel_pair().await;
        let _ = self.stop_scan().await;
        if self.interactive {
            let _ = tokio::time::timeout(TIMEOUT, async {
                if let Ok(proxy) = Proxy::new(
                    &self.connection,
                    self.owner.as_str(),
                    "/org/bluez",
                    "org.bluez.AgentManager1",
                )
                .await
                {
                    let _: zbus::Result<()> = proxy
                        .call(
                            "UnregisterAgent",
                            &(zbus::zvariant::ObjectPath::try_from(AGENT).unwrap(),),
                        )
                        .await;
                }
            })
            .await;
        }
        let _ = self.connection.close().await;
    }
}
/// Run inside the owning UI subscription. Dropping it drops the private connection,
/// which releases BlueZ's caller-scoped agent/discovery. Closing commands is graceful.
pub async fn run(
    endpoint: Endpoint,
    commands: mpsc::Receiver<Command>,
    states: watch::Sender<State>,
) {
    run_mode(endpoint, commands, states, true).await;
}
/// Passive mode only observes BlueZ; it registers no agent and rejects all commands.
/// Changing mode replaces the caller-owned connection, releasing interaction leases.
pub async fn run_mode(
    endpoint: Endpoint,
    mut commands: mpsc::Receiver<Command>,
    states: watch::Sender<State>,
    interactive: bool,
) {
    loop {
        let result = tokio::time::timeout(TIMEOUT, Session::connect(&endpoint, interactive)).await;
        let error = match result {
            Ok(Ok(mut session)) => {
                while commands.try_recv().is_ok() {} // never replay queued commands after recovery
                let result = session.run(&mut commands, &states).await;
                if let Err(error) = result {
                    states.send_replace(State {
                        error: Some(error),
                        ..Default::default()
                    });
                }
                session.shutdown().await;
                if result.is_ok() {
                    states.send_replace(State::default());
                    return;
                }
                result.err().unwrap()
            }
            Ok(Err(error)) => error,
            Err(_) => Error::Timeout,
        };
        states.send_replace(State {
            error: Some(error),
            ..Default::default()
        });
        let delay = tokio::time::sleep(Duration::from_secs(2));
        tokio::pin!(delay);
        loop {
            tokio::select! { _ = &mut delay => break, command = commands.recv() => if command.is_none() { return; } }
        }
    }
}
