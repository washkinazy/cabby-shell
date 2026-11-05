#![allow(dead_code)]
use futures_util::StreamExt;
use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
    time::Duration,
};
use zbus::{
    Connection, MatchRule, MessageStream, Proxy,
    connection::Builder,
    message::{Header, Type},
    zvariant::OwnedObjectPath,
};
pub const ADAPTER: &str = "/org/bluez/hci0";
pub const DEVICE: &str = "/org/bluez/hci0/dev_00_11_22_33_44_55";
#[derive(Default)]
pub struct Data {
    pub agent: Option<(String, OwnedObjectPath)>,
    pub scans: HashSet<String>,
    pub powered: bool,
    pub paired: bool,
    pub connected: bool,
    pub mode: String,
    pub removed: bool,
    pub reject: bool,
    pub cancels: usize,
    pub stops: usize,
}
type Shared = Arc<Mutex<Data>>;
struct Manager(Shared);
#[zbus::interface(name = "org.bluez.AgentManager1")]
impl Manager {
    fn register_agent(
        &self,
        path: OwnedObjectPath,
        capability: &str,
        #[zbus(header)] header: Header<'_>,
    ) -> zbus::fdo::Result<()> {
        if capability != "KeyboardDisplay" {
            return Err(zbus::fdo::Error::InvalidArgs("capability".into()));
        }
        self.0.lock().unwrap().agent = Some((header.sender().unwrap().to_string(), path));
        Ok(())
    }
    fn unregister_agent(&self, _path: OwnedObjectPath) {
        self.0.lock().unwrap().agent = None;
    }
}
struct Adapter(Shared);
#[zbus::interface(name = "org.bluez.Adapter1")]
impl Adapter {
    fn start_discovery(&self, #[zbus(header)] header: Header<'_>) -> zbus::fdo::Result<()> {
        let mut data = self.0.lock().unwrap();
        if !data.powered || data.reject {
            return Err(zbus::fdo::Error::Failed("synthetic rejection".into()));
        }
        data.scans.insert(header.sender().unwrap().to_string());
        Ok(())
    }
    fn stop_discovery(&self, #[zbus(header)] header: Header<'_>) {
        let mut d = self.0.lock().unwrap();
        d.scans.remove(header.sender().unwrap().as_str());
        d.stops += 1;
    }
    async fn remove_device(
        &self,
        path: OwnedObjectPath,
        #[zbus(object_server)] server: &zbus::ObjectServer,
    ) -> zbus::fdo::Result<()> {
        if path.as_str() != DEVICE {
            return Err(zbus::fdo::Error::InvalidArgs("path".into()));
        }
        self.0.lock().unwrap().removed = true;
        server
            .remove::<Device, _>(DEVICE)
            .await
            .map_err(|_| zbus::fdo::Error::Failed("remove".into()))?;
        Ok(())
    }
    #[zbus(property)]
    fn alias(&self) -> &str {
        "Fixture adapter"
    }
    #[zbus(property)]
    fn powered(&self) -> bool {
        self.0.lock().unwrap().powered
    }
    #[zbus(property)]
    fn set_powered(&mut self, value: bool) {
        self.0.lock().unwrap().powered = value;
    }
}
struct Device(Shared);
#[zbus::interface(name = "org.bluez.Device1")]
impl Device {
    async fn pair(&self, #[zbus(connection)] connection: &Connection) -> zbus::fdo::Result<()> {
        let (agent, mode, reject) = {
            let d = self.0.lock().unwrap();
            (d.agent.clone(), d.mode.clone(), d.reject)
        };
        if reject {
            return Err(zbus::fdo::Error::Failed("synthetic rejection".into()));
        }
        let (owner, path) = agent.ok_or_else(|| zbus::fdo::Error::Failed("no agent".into()))?;
        let proxy = Proxy::new(
            connection,
            owner.as_str(),
            path.as_str(),
            "org.bluez.Agent1",
        )
        .await?;
        let device = OwnedObjectPath::try_from(DEVICE).unwrap();
        match mode.as_str() {
            "pin" => {
                let pin: String = proxy.call("RequestPinCode", &(device,)).await?;
                if pin != "1234" {
                    return Err(zbus::fdo::Error::Failed("pin".into()));
                }
            }
            "passkey" => {
                let key: u32 = proxy.call("RequestPasskey", &(device,)).await?;
                if key != 123456 {
                    return Err(zbus::fdo::Error::Failed("key".into()));
                }
            }
            "display" => {
                let _: () = proxy
                    .call("DisplayPasskey", &(device, 123456u32, 2u16))
                    .await?;
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
            "authorize" => {
                let _: () = proxy.call("RequestAuthorization", &(device,)).await?;
            }
            _ => {
                let _: () = proxy
                    .call("RequestConfirmation", &(device, 123456u32))
                    .await?;
            }
        }
        self.0.lock().unwrap().paired = true;
        Ok(())
    }
    fn cancel_pairing(&self) {
        self.0.lock().unwrap().cancels += 1;
    }
    fn connect(&self) -> zbus::fdo::Result<()> {
        let mut d = self.0.lock().unwrap();
        if d.reject {
            return Err(zbus::fdo::Error::Failed("rejected".into()));
        }
        d.connected = true;
        Ok(())
    }
    fn disconnect(&self) {
        self.0.lock().unwrap().connected = false;
    }
    #[zbus(property)]
    fn alias(&self) -> &str {
        "Fixture keyboard"
    }
    #[zbus(property)]
    fn adapter(&self) -> OwnedObjectPath {
        OwnedObjectPath::try_from(ADAPTER).unwrap()
    }
    #[zbus(property)]
    fn paired(&self) -> bool {
        self.0.lock().unwrap().paired
    }
    #[zbus(property)]
    fn connected(&self) -> bool {
        self.0.lock().unwrap().connected
    }
}
struct Probe(Shared);
#[zbus::interface(name = "org.cabby.Test")]
impl Probe {
    fn status(&self) -> (bool, u32) {
        let d = self.0.lock().unwrap();
        (d.agent.is_some(), d.scans.len() as u32)
    }
}
pub struct Fixture {
    pub connection: Connection,
    pub data: Shared,
    watcher: tokio::task::JoinHandle<()>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.watcher.abort();
    }
}
impl Fixture {
    pub async fn start(address: &str) -> Self {
        let data = Arc::new(Mutex::new(Data {
            powered: true,
            ..Default::default()
        }));
        let connection = Builder::address(address)
            .unwrap()
            .name("org.bluez")
            .unwrap()
            .serve_at("/test", Probe(data.clone()))
            .unwrap()
            .serve_at("/", zbus::fdo::ObjectManager)
            .unwrap()
            .serve_at("/org/bluez", Manager(data.clone()))
            .unwrap()
            .serve_at(ADAPTER, Adapter(data.clone()))
            .unwrap()
            .serve_at(DEVICE, Device(data.clone()))
            .unwrap()
            .build()
            .await
            .unwrap();
        let mut signals = MessageStream::for_match_rule(
            MatchRule::builder()
                .msg_type(Type::Signal)
                .sender("org.freedesktop.DBus")
                .unwrap()
                .interface("org.freedesktop.DBus")
                .unwrap()
                .member("NameOwnerChanged")
                .unwrap()
                .build(),
            &connection,
            Some(32),
        )
        .await
        .unwrap();
        let shared = data.clone();
        let watcher = tokio::spawn(async move {
            while let Some(Ok(message)) = signals.next().await {
                if let Ok((name, _, new)) = message.body().deserialize::<(String, String, String)>()
                    && new.is_empty()
                {
                    let mut d = shared.lock().unwrap();
                    d.scans.remove(&name);
                    if d.agent.as_ref().is_some_and(|(owner, _)| owner == &name) {
                        d.agent = None;
                    }
                }
            }
        });
        Self {
            connection,
            data,
            watcher,
        }
    }
    pub async fn changed(&self) {
        self.connection
            .emit_signal(
                None::<&str>,
                DEVICE,
                "org.freedesktop.DBus.Properties",
                "PropertiesChanged",
                &(
                    "org.bluez.Device1",
                    std::collections::HashMap::<String, zbus::zvariant::OwnedValue>::new(),
                    Vec::<String>::new(),
                ),
            )
            .await
            .unwrap();
    }
}
