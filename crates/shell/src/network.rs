//! Read-only NetworkManager status. No UI, network probing, or radio changes.
use futures_util::StreamExt;
use std::{collections::HashMap, time::Duration};
use zbus::{
    Connection, MatchRule, MessageStream, Proxy, connection::Builder, fdo::DBusProxy,
    message::Type, zvariant::OwnedValue,
};
const NAME: &str = "org.freedesktop.NetworkManager";
const PATH: &str = "/org/freedesktop/NetworkManager";
const TIMEOUT: Duration = Duration::from_secs(3);
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    pub label: String,
    pub detail: &'static str,
}
impl Default for Status {
    fn default() -> Self {
        Self {
            label: "Network ?".into(),
            detail: "NetworkManager unavailable",
        }
    }
}
pub struct Client {
    connection: Connection,
    properties: MessageStream,
    owners: MessageStream,
    owner: String,
}
impl Client {
    pub async fn connect(address: Option<&str>) -> Result<Self, ()> {
        tokio::time::timeout(TIMEOUT, async {
            let builder = match address {
                Some(a) => Builder::address(a),
                None => Builder::system(),
            }
            .map_err(|_| ())?;
            let connection = builder
                .method_timeout(TIMEOUT)
                .build()
                .await
                .map_err(|_| ())?;
            let owners = MessageStream::for_match_rule(
                MatchRule::builder()
                    .msg_type(Type::Signal)
                    .sender("org.freedesktop.DBus")
                    .unwrap()
                    .interface("org.freedesktop.DBus")
                    .unwrap()
                    .member("NameOwnerChanged")
                    .unwrap()
                    .arg(0, NAME)
                    .unwrap()
                    .build(),
                &connection,
                Some(8),
            )
            .await
            .map_err(|_| ())?;
            let owner = DBusProxy::new(&connection)
                .await
                .map_err(|_| ())?
                .get_name_owner(NAME.try_into().unwrap())
                .await
                .map_err(|_| ())?
                .to_string();
            let properties = MessageStream::for_match_rule(
                MatchRule::builder()
                    .msg_type(Type::Signal)
                    .sender(owner.as_str())
                    .map_err(|_| ())?
                    .path(PATH)
                    .unwrap()
                    .interface("org.freedesktop.DBus.Properties")
                    .unwrap()
                    .member("PropertiesChanged")
                    .unwrap()
                    .build(),
                &connection,
                Some(8),
            )
            .await
            .map_err(|_| ())?;
            Ok(Self {
                connection,
                properties,
                owners,
                owner,
            })
        })
        .await
        .map_err(|_| ())?
    }
    pub async fn snapshot(&self) -> Result<Status, ()> {
        let proxy = Proxy::new(
            &self.connection,
            self.owner.as_str(),
            PATH,
            "org.freedesktop.DBus.Properties",
        )
        .await
        .map_err(|_| ())?;
        let reply = proxy
            .call_method("GetAll", &(NAME,))
            .await
            .map_err(|_| ())?;
        if reply.body().len() > 65536 {
            return Err(());
        }
        decode(reply.body().deserialize().map_err(|_| ())?)
    }
    pub async fn next(&mut self) -> Result<Status, ()> {
        loop {
            tokio::select! {
                _ = self.connection.closed() => return Err(()),
                signal = self.owners.next() => {
                    let signal = signal.ok_or(())?.map_err(|_| ())?;
                    let (_,old,new): (String,String,String) = signal.body().deserialize().map_err(|_| ())?;
                    if old == self.owner && new != old { return Err(()); }
                }
                signal = self.properties.next() => {
                    let signal = signal.ok_or(())?.map_err(|_| ())?;
                    if signal.body().len() > 65536 { return Err(()); }
                    let (interface,_,_): (String,HashMap<String,OwnedValue>,Vec<String>) = signal.body().deserialize().map_err(|_| ())?;
                    if interface == NAME { return self.snapshot().await; }
                }
            }
        }
    }
}
fn decode(props: HashMap<String, OwnedValue>) -> Result<Status, ()> {
    let state = props
        .get("State")
        .and_then(|v| u32::try_from(v).ok())
        .ok_or(())?;
    let connectivity = props
        .get("Connectivity")
        .and_then(|v| u32::try_from(v).ok())
        .unwrap_or(0);
    let kind = props
        .get("PrimaryConnectionType")
        .and_then(|v| <&str>::try_from(v).ok())
        .unwrap_or("");
    let transport = match kind {
        "802-11-wireless" => "Wi-Fi",
        "802-3-ethernet" => "Wired",
        "vpn" | "wireguard" => "VPN",
        _ => "Network",
    };
    let (label, detail) = match state {
        10 => (
            "Network off".into(),
            "NetworkManager networking is disabled",
        ),
        20 => ("Offline".into(), "No active network connection"),
        30 => ("Disconnecting".into(), "Network connection is stopping"),
        40 => ("Connecting".into(), "Network connection is starting"),
        50 => (
            format!("{transport} local"),
            "Local network only; no default route",
        ),
        60 | 70 if connectivity == 2 => (
            format!("{transport} portal"),
            "NetworkManager reports a captive portal",
        ),
        60 | 70 if connectivity == 3 || state == 60 => (
            format!("{transport} limited"),
            "NetworkManager reports limited connectivity",
        ),
        70 => (
            transport.into(),
            "NetworkManager reports a connected network",
        ),
        _ => (
            "Network ?".into(),
            "NetworkManager reports an unknown state",
        ),
    };
    Ok(Status { label, detail })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{BufRead, BufReader},
        process::{Child, Command, Stdio},
        sync::{Arc, Mutex},
    };
    use zbus::zvariant::Value;
    struct Bus {
        child: Child,
        address: String,
    }
    impl Drop for Bus {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
    fn bus() -> Bus {
        let mut child = Command::new("dbus-daemon")
            .args(["--session", "--nofork", "--print-address=1"])
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut address = String::new();
        BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut address)
            .unwrap();
        Bus {
            child,
            address: address.trim().into(),
        }
    }
    fn properties(state: u32, connectivity: u32, kind: &str) -> HashMap<String, OwnedValue> {
        HashMap::from([
            ("State".into(), state.into()),
            ("Connectivity".into(), connectivity.into()),
            (
                "PrimaryConnectionType".into(),
                Value::from(kind).try_to_owned().unwrap(),
            ),
        ])
    }
    #[test]
    fn distinguishes_offline_local_portal_limited_and_unknown() {
        for (state, connectivity, kind, label) in [
            (20, 1, "", "Offline"),
            (50, 1, "802-3-ethernet", "Wired local"),
            (60, 2, "802-11-wireless", "Wi-Fi portal"),
            (60, 3, "802-11-wireless", "Wi-Fi limited"),
            (70, 4, "802-3-ethernet", "Wired"),
            (70, 0, "wireguard", "VPN"),
            (0, 0, "", "Network ?"),
        ] {
            assert_eq!(
                decode(properties(state, connectivity, kind)).unwrap().label,
                label
            );
        }
        assert!(decode(HashMap::new()).is_err());
    }
    struct Fixture(Arc<Mutex<(u32, u32, String)>>);
    #[zbus::interface(name = "org.freedesktop.NetworkManager")]
    impl Fixture {
        #[zbus(property)]
        fn state(&self) -> u32 {
            self.0.lock().unwrap().0
        }
        #[zbus(property)]
        fn connectivity(&self) -> u32 {
            self.0.lock().unwrap().1
        }
        #[zbus(property)]
        fn primary_connection_type(&self) -> String {
            self.0.lock().unwrap().2.clone()
        }
    }
    #[tokio::test(flavor = "current_thread")]
    async fn private_status_events_owner_restart_and_bus_loss() {
        let mut bus = bus();
        assert!(Client::connect(Some(&bus.address)).await.is_err());
        let state = Arc::new(Mutex::new((20, 1, String::new())));
        let owner = Builder::address(bus.address.as_str())
            .unwrap()
            .name(NAME)
            .unwrap()
            .serve_at(PATH, Fixture(state.clone()))
            .unwrap()
            .build()
            .await
            .unwrap();
        let mut client = Client::connect(Some(&bus.address)).await.unwrap();
        assert_eq!(client.snapshot().await.unwrap().label, "Offline");
        *state.lock().unwrap() = (70, 4, "802-3-ethernet".into());
        owner
            .emit_signal(
                None::<&str>,
                PATH,
                "org.freedesktop.DBus.Properties",
                "PropertiesChanged",
                &(
                    NAME,
                    properties(70, 4, "802-3-ethernet"),
                    Vec::<String>::new(),
                ),
            )
            .await
            .unwrap();
        assert_eq!(
            tokio::time::timeout(TIMEOUT, client.next())
                .await
                .unwrap()
                .unwrap()
                .label,
            "Wired"
        );
        owner.close().await.unwrap();
        assert!(
            tokio::time::timeout(TIMEOUT, client.next())
                .await
                .unwrap()
                .is_err()
        );
        *state.lock().unwrap() = (60, 2, "802-11-wireless".into());
        let _owner = Builder::address(bus.address.as_str())
            .unwrap()
            .name(NAME)
            .unwrap()
            .serve_at(PATH, Fixture(state))
            .unwrap()
            .build()
            .await
            .unwrap();
        let mut client = Client::connect(Some(&bus.address)).await.unwrap();
        assert_eq!(client.snapshot().await.unwrap().label, "Wi-Fi portal");
        bus.child.kill().unwrap();
        bus.child.wait().unwrap();
        assert!(
            tokio::time::timeout(TIMEOUT, client.next())
                .await
                .unwrap()
                .is_err()
        );
    }
}
