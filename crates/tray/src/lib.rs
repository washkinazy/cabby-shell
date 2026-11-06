//! Session-bus StatusNotifier host. Protocol/state ownership is independent of Iced.
use futures_util::StreamExt;
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    sync::{mpsc, watch},
    time::Instant,
};
use zbus::{
    Connection, MatchRule, MessageStream, Proxy,
    connection::Builder,
    fdo::{DBusProxy, RequestNameFlags, RequestNameReply},
    message::{Header, Type},
    object_server::SignalEmitter,
    zvariant::{OwnedObjectPath, OwnedValue, Value},
};
pub mod menu;

pub const WATCHER: &str = "org.kde.StatusNotifierWatcher";
pub const WATCHER_PATH: &str = "/StatusNotifierWatcher";
pub const ITEM: &str = "org.kde.StatusNotifierItem";
const TIMEOUT: Duration = Duration::from_secs(3);
const MAX_ITEMS: usize = 64;
pub type Properties = HashMap<String, OwnedValue>;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    Unavailable,
    Rejected,
    InvalidData,
}
pub type Result<T> = std::result::Result<T, Error>;
impl From<zbus::Error> for Error {
    fn from(_: zbus::Error) -> Self {
        Self::Unavailable
    }
}
impl From<zbus::fdo::Error> for Error {
    fn from(_: zbus::fdo::Error) -> Self {
        Self::Unavailable
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Token {
    pub owner: String,
    pub path: String,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pixmap {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub token: Token,
    pub title: String,
    pub attention: bool,
    pub passive: bool,
    pub icon_name: String,
    pub icon_theme_path: String,
    pub pixmap: Option<Pixmap>,
    pub menu_path: Option<String>,
    pub is_menu: bool,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State {
    pub available: bool,
    pub items: Vec<Item>,
    pub menu: Option<menu::Menu>,
    pub failed: bool,
}
#[derive(Debug, Clone)]
pub enum Command {
    Activate(Token),
    Secondary(Token),
    Menu(Token, i32),
    Click(Token, i32),
    CloseMenu,
}
#[derive(Debug, Clone)]
pub struct Handle {
    pub commands: mpsc::Sender<Command>,
    pub snapshots: watch::Receiver<State>,
}
#[derive(Clone)]
struct Registration {
    name: String,
    token: Token,
}
type Registry = Arc<Mutex<Vec<Registration>>>;
struct Watcher {
    registry: Registry,
    changes: mpsc::Sender<Registration>,
}
#[zbus::interface(name = "org.kde.StatusNotifierWatcher")]
impl Watcher {
    async fn register_status_notifier_item(
        &self,
        service: &str,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &Connection,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<()> {
        let sender = header
            .sender()
            .ok_or_else(|| zbus::fdo::Error::InvalidArgs("sender".into()))?
            .as_str();
        let registration = resolve(connection, service, Some(sender))
            .await
            .map_err(|_| zbus::fdo::Error::InvalidArgs("invalid item or owner".into()))?;
        {
            let mut entries = self.registry.lock().unwrap();
            if entries.iter().any(|r| r.name == registration.name) {
                return Ok(());
            }
            if entries.len() >= MAX_ITEMS {
                return Err(zbus::fdo::Error::LimitsExceeded("tray full".into()));
            }
            self.changes
                .try_send(registration.clone())
                .map_err(|_| zbus::fdo::Error::LimitsExceeded("tray busy".into()))?;
            entries.push(registration.clone());
        }
        Self::status_notifier_item_registered(&emitter, &registration.name).await?;
        Ok(())
    }
    fn register_status_notifier_host(&self, _service: &str) {}
    #[zbus(property)]
    fn registered_status_notifier_items(&self) -> Vec<String> {
        self.registry
            .lock()
            .unwrap()
            .iter()
            .map(|r| r.name.clone())
            .collect()
    }
    #[zbus(property)]
    fn is_status_notifier_host_registered(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn protocol_version(&self) -> i32 {
        0
    }
    #[zbus(signal)]
    async fn status_notifier_item_registered(
        emitter: &SignalEmitter<'_>,
        service: &str,
    ) -> zbus::Result<()>;
    #[zbus(signal)]
    async fn status_notifier_item_unregistered(
        emitter: &SignalEmitter<'_>,
        service: &str,
    ) -> zbus::Result<()>;
    #[zbus(signal)]
    async fn status_notifier_host_registered(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;
    #[zbus(signal)]
    async fn status_notifier_host_unregistered(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;
}
async fn resolve(
    connection: &Connection,
    service: &str,
    sender: Option<&str>,
) -> Result<Registration> {
    if service.len() > 1024 {
        return Err(Error::InvalidData);
    }
    let (name, path) = if service.starts_with('/') {
        (sender.ok_or(Error::InvalidData)?, service)
    } else if let Some((name, path)) = service.split_once('/') {
        (name, &service[name.len()..name.len() + path.len() + 1])
    } else {
        (service, "/StatusNotifierItem")
    };
    let _: OwnedObjectPath = path.try_into().map_err(|_| Error::InvalidData)?;
    let owner = DBusProxy::new(connection)
        .await?
        .get_name_owner(name.try_into().map_err(|_| Error::InvalidData)?)
        .await?
        .to_string();
    if sender.is_some_and(|sender| sender != owner) {
        return Err(Error::Rejected);
    }
    Ok(Registration {
        name: format!("{name}{path}"),
        token: Token {
            owner,
            path: path.into(),
        },
    })
}

struct Retry {
    attempt: u8,
    due: Instant,
}
pub struct Service {
    retries: HashMap<Token, Retry>,
    connection: Connection,
    signals: futures_util::stream::SelectAll<MessageStream>,
    registry: Registry,
    registrations: mpsc::Receiver<Registration>,
    commands: mpsc::Receiver<Command>,
    snapshots: watch::Sender<State>,
    state: State,
    owns_watcher: bool,
    watcher_owner: String,
    primed_menus: HashSet<(Token, String)>,
}
impl Service {
    pub async fn connect(address: &str) -> Result<(Self, Handle)> {
        tokio::time::timeout(TIMEOUT, Self::connect_inner(address))
            .await
            .map_err(|_| Error::Unavailable)?
    }
    async fn connect_inner(address: &str) -> Result<(Self, Handle)> {
        let registry = Registry::default();
        let (changes, registrations) = mpsc::channel(MAX_ITEMS);
        let connection = Builder::address(address)?
            .method_timeout(TIMEOUT)
            .max_queued(64)
            .serve_at(
                WATCHER_PATH,
                Watcher {
                    registry: registry.clone(),
                    changes,
                },
            )?
            .build()
            .await?;
        let mut signals = futures_util::stream::SelectAll::new();
        for interface in [
            WATCHER,
            ITEM,
            "org.freedesktop.DBus.Properties",
            "org.freedesktop.DBus",
            "com.canonical.dbusmenu",
        ] {
            signals.push(
                MessageStream::for_match_rule(
                    MatchRule::builder()
                        .msg_type(Type::Signal)
                        .interface(interface)?
                        .build(),
                    &connection,
                    Some(64),
                )
                .await?,
            );
        }
        let bus = DBusProxy::new(&connection).await?;
        let owns_watcher = bus
            .request_name(
                WATCHER.try_into().unwrap(),
                RequestNameFlags::DoNotQueue.into(),
            )
            .await?
            == RequestNameReply::PrimaryOwner;
        let watcher_owner = bus
            .get_name_owner(WATCHER.try_into().unwrap())
            .await?
            .to_string();
        let state = State {
            available: true,
            ..State::default()
        };
        if !owns_watcher {
            let proxy =
                Proxy::new(&connection, watcher_owner.as_str(), WATCHER_PATH, WATCHER).await?;
            let _: () = proxy
                .call(
                    "RegisterStatusNotifierHost",
                    &(connection.unique_name().unwrap().as_str(),),
                )
                .await?;
            let names: Vec<String> = proxy.get_property("RegisteredStatusNotifierItems").await?;
            for name in names.into_iter().take(MAX_ITEMS) {
                if let Ok(r) = resolve(&connection, &name, None).await {
                    registry.lock().unwrap().push(r);
                }
            }
        }
        let retries = registry
            .lock()
            .unwrap()
            .iter()
            .map(|r| {
                (
                    r.token.clone(),
                    Retry {
                        attempt: 0,
                        due: Instant::now(),
                    },
                )
            })
            .collect();
        let (commands_tx, commands) = mpsc::channel(8);
        let (snapshots, snapshots_rx) = watch::channel(state.clone());
        Ok((
            Self {
                retries,
                connection,
                signals,
                registry,
                registrations,
                commands,
                snapshots,
                state,
                owns_watcher,
                watcher_owner,
                primed_menus: HashSet::new(),
            },
            Handle {
                commands: commands_tx,
                snapshots: snapshots_rx,
            },
        ))
    }
    pub async fn run(mut self) -> Result<()> {
        let result = self.run_inner().await;
        self.close_menu().await;
        self.snapshots.send_replace(State::default());
        let _ = self.connection.clone().close().await;
        result
    }
    async fn run_inner(&mut self) -> Result<()> {
        loop {
            let next = self
                .retries
                .iter()
                .min_by_key(|(_, r)| r.due)
                .map(|(t, r)| (t.clone(), r.due));
            tokio::select! {
                _ = async { match &next { Some((_,due)) => tokio::time::sleep_until(*due).await, None => std::future::pending().await } } => {
                    if let Some((token,_)) = next { self.refresh(&token).await; }
                }
                _ = self.connection.closed() => return Err(Error::Unavailable),
                registration = self.registrations.recv() => {
                    if let Some(r) = registration { self.schedule(&r.token); }
                }
                command = self.commands.recv() => {
                    let Some(command) = command else { return Ok(()); };
                    self.state.failed = self.command(command).await.is_err();
                }
                signal = self.signals.next() => {
                    let signal = signal.ok_or(Error::Unavailable)??;
                    self.signal(&signal).await?;
                }
            }
            self.primed_menus.retain(|(token, path)| {
                self.state
                    .items
                    .iter()
                    .any(|i| &i.token == token && i.menu_path.as_ref() == Some(path))
            });
            self.snapshots.send_if_modified(|current| {
                if *current == self.state {
                    return false;
                }
                *current = self.state.clone();
                true
            });
        }
    }
    fn schedule(&mut self, token: &Token) {
        self.retries.entry(token.clone()).or_insert(Retry {
            attempt: 0,
            due: Instant::now() + Duration::from_millis(100),
        });
    }
    async fn refresh(&mut self, token: &Token) {
        let Some(mut retry) = self.retries.remove(token) else {
            return;
        };
        if !self
            .registry
            .lock()
            .unwrap()
            .iter()
            .any(|r| r.token == *token)
        {
            return;
        }
        if let Ok(item) = read_item(&self.connection, token).await {
            self.state.items.retain(|i| i.token != *token);
            self.state.items.push(item);
            self.state.items.sort_by(|a, b| {
                a.token
                    .owner
                    .cmp(&b.token.owner)
                    .then(a.token.path.cmp(&b.token.path))
            });
        } else if retry.attempt < 3 {
            retry.attempt += 1;
            retry.due = Instant::now() + Duration::from_millis(250 * (1 << (retry.attempt - 1)));
            self.retries.insert(token.clone(), retry);
        }
    }
    async fn signal(&mut self, signal: &zbus::Message) -> Result<()> {
        let header = signal.header();
        let sender = header.sender().map(|s| s.as_str()).unwrap_or("");
        let member = header.member().map(|s| s.as_str()).unwrap_or("");
        let interface = header.interface().map(|s| s.as_str()).unwrap_or("");
        if interface == "org.freedesktop.DBus"
            && sender == "org.freedesktop.DBus"
            && member == "NameOwnerChanged"
        {
            let (name, old, new): (String, String, String) = signal.body().deserialize()?;
            if name == WATCHER && old == self.watcher_owner && new != old {
                return Err(Error::Unavailable);
            }
            if old.is_empty() || old == new {
                return Ok(());
            }
            let removed: Vec<_> = {
                let mut entries = self.registry.lock().unwrap();
                let removed = entries
                    .iter()
                    .filter(|r| {
                        r.token.owner == old
                            && (name == old || r.name.starts_with(&format!("{name}/")))
                    })
                    .cloned()
                    .collect::<Vec<_>>();
                entries.retain(|r| !removed.iter().any(|x| x.name == r.name));
                removed
            };
            for r in removed {
                self.retries.remove(&r.token);
                self.state.items.retain(|i| i.token != r.token);
                if self.state.menu.as_ref().is_some_and(|m| m.token == r.token) {
                    self.state.menu = None;
                }
                if self.owns_watcher {
                    self.connection
                        .emit_signal(
                            None::<&str>,
                            WATCHER_PATH,
                            WATCHER,
                            "StatusNotifierItemUnregistered",
                            &(r.name,),
                        )
                        .await?;
                }
            }
        } else if !self.owns_watcher && sender == self.watcher_owner && interface == WATCHER {
            if member == "StatusNotifierItemRegistered" {
                let name: String = signal.body().deserialize()?;
                if self.registry.lock().unwrap().len() < MAX_ITEMS
                    && let Ok(r) = resolve(&self.connection, &name, None).await
                {
                    {
                        let mut entries = self.registry.lock().unwrap();
                        entries.retain(|x| x.name != r.name);
                        entries.push(r.clone());
                    }
                    self.schedule(&r.token);
                }
            } else if member == "StatusNotifierItemUnregistered" {
                let name: String = signal.body().deserialize()?;
                let token = self
                    .registry
                    .lock()
                    .unwrap()
                    .iter()
                    .find(|r| r.name == name)
                    .map(|r| r.token.clone());
                self.registry.lock().unwrap().retain(|r| r.name != name);
                if let Some(token) = token {
                    self.retries.remove(&token);
                    self.state.items.retain(|i| i.token != token);
                    if self.state.menu.as_ref().is_some_and(|m| m.token == token) {
                        self.state.menu = None;
                    }
                }
            }
        } else if interface == ITEM || interface == "org.freedesktop.DBus.Properties" {
            let path = header.path().map(|p| p.as_str()).unwrap_or("");
            let token = self
                .registry
                .lock()
                .unwrap()
                .iter()
                .find(|r| r.token.owner == sender && r.token.path == path)
                .map(|r| r.token.clone());
            if let Some(token) = token {
                self.schedule(&token);
            }
        } else if interface == "com.canonical.dbusmenu"
            && matches!(member, "LayoutUpdated" | "ItemsPropertiesUpdated")
            && let Some(menu) = &self.state.menu
            && menu.token.owner == sender
            && header.path().is_some_and(|p| p.as_str() == menu.path)
        {
            let next = menu::load(&self.connection, &menu.token, &menu.path, menu.parent).await;
            match next {
                Ok(menu) => self.state.menu = Some(menu),
                Err(_) => {
                    self.close_menu().await;
                    self.state.failed = true;
                }
            }
        }
        Ok(())
    }
    async fn command(&mut self, command: Command) -> Result<()> {
        if matches!(command, Command::CloseMenu) {
            self.close_menu().await;
            return Ok(());
        }
        let token = match &command {
            Command::Activate(t)
            | Command::Secondary(t)
            | Command::Menu(t, _)
            | Command::Click(t, _) => t,
            Command::CloseMenu => unreachable!(),
        };
        let item = self
            .state
            .items
            .iter()
            .find(|i| &i.token == token)
            .ok_or(Error::Rejected)?
            .clone();
        let proxy = Proxy::new(
            &self.connection,
            token.owner.as_str(),
            token.path.as_str(),
            ITEM,
        )
        .await?;
        match command {
            Command::Activate(_) if !item.is_menu => {
                self.close_menu().await;
                let activation = tokio::time::timeout(
                    Duration::from_secs(1),
                    proxy.call::<_, _, ()>("Activate", &(0i32, 0i32)),
                )
                .await;
                match activation {
                    Ok(Ok(())) => (),
                    _ if item.menu_path.is_some() => self.open_menu(&item, 0).await?,
                    Ok(Err(error)) => return Err(error.into()),
                    Err(_) => return Err(Error::Unavailable),
                }
            }
            Command::Secondary(_) => {
                let _: () = proxy.call("SecondaryActivate", &(0i32, 0i32)).await?;
            }
            Command::Menu(_, parent) => self.open_menu(&item, parent).await?,
            Command::Activate(_) => self.open_menu(&item, 0).await?,
            Command::Click(_, id) => {
                let menu = self
                    .state
                    .menu
                    .as_ref()
                    .filter(|m| m.token == item.token)
                    .ok_or(Error::Rejected)?;
                let entry = menu
                    .entries
                    .iter()
                    .find(|e| e.id == id && e.enabled && !e.separator)
                    .ok_or(Error::Rejected)?;
                if entry.submenu {
                    self.open_menu(&item, id).await?;
                } else {
                    menu::event(&self.connection, &item.token, &menu.path, id, "clicked").await?;
                    self.close_menu().await;
                }
            }
            Command::CloseMenu => (),
        }
        Ok(())
    }
    async fn open_menu(&mut self, item: &Item, parent: i32) -> Result<()> {
        if let Some(path) = &item.menu_path {
            let previous = self
                .state
                .menu
                .as_ref()
                .filter(|m| m.token == item.token && &m.path == path)
                .map(|m| m.parent);
            if let Some(previous) = previous {
                if previous != parent && previous != 0 {
                    let _ =
                        menu::event(&self.connection, &item.token, path, previous, "closed").await;
                }
            } else {
                self.close_menu().await;
                let _ = menu::event(&self.connection, &item.token, path, 0, "opened").await;
            }
            if previous != Some(parent) && parent != 0 {
                let _ = menu::event(&self.connection, &item.token, path, parent, "opened").await;
            }
            // Track the open lifecycle before fetching, so failure and shutdown
            // still send matching closed events. Only run_inner publishes snapshots.
            self.state.menu = Some(menu::Menu {
                token: item.token.clone(),
                path: path.clone(),
                parent,
                entries: Vec::new(),
            });
            let next =
                if parent == 0 && self.primed_menus.insert((item.token.clone(), path.clone())) {
                    // Fetch immediately even when root AboutToShow never replies.
                    // A true reply requests a fresh layout; signals use load alone.
                    let (layout, changed) = tokio::join!(
                        menu::load(&self.connection, &item.token, path, parent),
                        menu::prepare(&self.connection, &item.token, path, parent),
                    );
                    if changed {
                        menu::load(&self.connection, &item.token, path, parent).await
                    } else {
                        layout
                    }
                } else {
                    if parent != 0 {
                        menu::prepare(&self.connection, &item.token, path, parent).await;
                    }
                    menu::load(&self.connection, &item.token, path, parent).await
                };
            match next {
                Ok(menu) => self.state.menu = Some(menu),
                Err(error) => {
                    self.close_menu().await;
                    return Err(error);
                }
            }
        } else {
            self.close_menu().await;
            let proxy = Proxy::new(
                &self.connection,
                item.token.owner.as_str(),
                item.token.path.as_str(),
                ITEM,
            )
            .await?;
            let _: () = proxy.call("ContextMenu", &(0i32, 0i32)).await?;
        }
        Ok(())
    }
    async fn close_menu(&mut self) {
        if let Some(menu) = self.state.menu.take() {
            if menu.parent != 0 {
                let _ = menu::event(
                    &self.connection,
                    &menu.token,
                    &menu.path,
                    menu.parent,
                    "closed",
                )
                .await;
            }
            let _ = menu::event(&self.connection, &menu.token, &menu.path, 0, "closed").await;
        }
    }
}
pub(crate) fn string(props: &Properties, key: &str) -> String {
    props
        .get(key)
        .and_then(|v| <&str>::try_from(v).ok())
        .unwrap_or("")
        .chars()
        .filter(|c| !c.is_control())
        .take(256)
        .collect()
}
pub(crate) fn boolean(props: &Properties, key: &str, default: bool) -> bool {
    props
        .get(key)
        .and_then(|v| bool::try_from(v).ok())
        .unwrap_or(default)
}
async fn read_item(connection: &Connection, token: &Token) -> Result<Item> {
    let proxy = Proxy::new(
        connection,
        token.owner.as_str(),
        token.path.as_str(),
        "org.freedesktop.DBus.Properties",
    )
    .await?;
    let reply = proxy.call_method("GetAll", &(ITEM,)).await?;
    if reply.body().len() > 4 * 1024 * 1024 {
        return Err(Error::InvalidData);
    }
    let props: Properties = reply.body().deserialize()?;
    if props.is_empty() {
        return Err(Error::InvalidData);
    }
    let status = string(&props, "Status");
    let attention = status == "NeedsAttention";
    let mut icon_name = string(
        &props,
        if attention {
            "AttentionIconName"
        } else {
            "IconName"
        },
    );
    if icon_name.is_empty() {
        icon_name = string(&props, "IconName");
    }
    let pixmap = props
        .get(if attention {
            "AttentionIconPixmap"
        } else {
            "IconPixmap"
        })
        .and_then(decode_pixmap)
        .or_else(|| props.get("IconPixmap").and_then(decode_pixmap));
    let menu_path = props
        .get("Menu")
        .and_then(|v| <&zbus::zvariant::ObjectPath<'_>>::try_from(v).ok())
        .filter(|p| p.as_str() != "/")
        .map(|p| p.as_str().into());
    let mut title = string(&props, "Title");
    if title.is_empty() {
        title = string(&props, "Id");
    }
    if title.is_empty() {
        title = "Application".into();
    }
    Ok(Item {
        token: token.clone(),
        title,
        attention,
        passive: status == "Passive",
        icon_name,
        icon_theme_path: string(&props, "IconThemePath"),
        pixmap,
        menu_path,
        is_menu: boolean(&props, "ItemIsMenu", false),
    })
}
fn decode_pixmap(value: &OwnedValue) -> Option<Pixmap> {
    let values: Vec<(i32, i32, Vec<u8>)> = value.try_clone().ok()?.try_into().ok()?;
    let (width, height, bytes) = values
        .into_iter()
        .filter(|(w, h, b)| {
            *w > 0
                && *h > 0
                && *w <= 512
                && *h <= 512
                && b.len() == (*w as usize) * (*h as usize) * 4
        })
        .min_by_key(|(w, h, _)| (w.abs_diff(24) + h.abs_diff(24), *w))
        .map(|(w, h, b)| (w as u32, h as u32, b))?;
    let rgba = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| [p[1], p[2], p[3], p[0]])
        .collect();
    Some(Pixmap {
        width,
        height,
        rgba,
    })
}
