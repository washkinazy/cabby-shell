mod support;
use cabby_bluetooth::{Command, Endpoint, PromptKind, Reply, State};
use std::{
    process::{Child, Command as Process, Stdio},
    time::Duration,
};
use tokio::sync::{mpsc, watch};
use zbus::{Proxy, connection::Builder, zvariant::OwnedObjectPath};
struct Bus {
    child: Child,
    _directory: tempfile::TempDir,
    address: String,
}
impl Drop for Bus {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl Bus {
    async fn start() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let socket = directory.path().join("bus");
        let address = format!("unix:path={}", socket.display());
        let child = Process::new("dbus-daemon")
            .args([
                "--session",
                "--nofork",
                "--nopidfile",
                &format!("--address={address}"),
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let bus = Self {
            child,
            _directory: directory,
            address,
        };
        tokio::time::timeout(Duration::from_secs(3), async {
            while !socket.exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        bus
    }
}
async fn state(rx: &mut watch::Receiver<State>, predicate: impl Fn(&State) -> bool) -> State {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let s = rx.borrow_and_update().clone();
            if predicate(&s) {
                return s;
            }
            rx.changed().await.unwrap();
        }
    })
    .await
    .expect("state deadline")
}
async fn command(
    tx: &mpsc::Sender<Command>,
    rx: &mut watch::Receiver<State>,
    cmd: Command,
) -> State {
    tx.send(cmd).await.unwrap();
    state(rx, |s| s.busy).await;
    state(rx, |s| !s.busy).await
}
#[tokio::test]
async fn device_controls_pairing_prompts_and_graceful_cleanup() {
    let bus = Bus::start().await;
    let fixture = support::Fixture::start(&bus.address).await;
    let (tx, commands) = mpsc::channel(8);
    let (states, mut rx) = watch::channel(State::default());
    let task = tokio::spawn(cabby_bluetooth::run(
        Endpoint::Private(bus.address.clone()),
        commands,
        states,
    ));
    state(&mut rx, |s| s.available).await;
    let s = command(&tx, &mut rx, Command::Scan(support::ADAPTER.into())).await;
    assert!(s.scanning.is_some());
    assert_eq!(fixture.data.lock().unwrap().scans.len(), 1);
    command(&tx, &mut rx, Command::StopScan).await;
    assert!(fixture.data.lock().unwrap().scans.is_empty());
    for (mode, reply) in [
        ("confirm", Reply::Accept),
        ("pin", Reply::Text("1234".into())),
        ("passkey", Reply::Text("123456".into())),
        ("authorize", Reply::Accept),
    ] {
        fixture.data.lock().unwrap().mode = mode.into();
        tx.send(Command::Pair(support::DEVICE.into()))
            .await
            .unwrap();
        let s = state(&mut rx, |s| s.prompt.is_some()).await;
        let id = s.prompt.unwrap().id;
        tx.send(Command::Reply(id + 1, Reply::Reject))
            .await
            .unwrap();
        tx.send(Command::Reply(id, reply)).await.unwrap();
        let s = state(&mut rx, |s| !s.busy && s.prompt.is_none()).await;
        assert!(s.devices[0].paired);
        assert!(s.error.is_none());
    }
    fixture.data.lock().unwrap().mode = "display".into();
    tx.send(Command::Pair(support::DEVICE.into()))
        .await
        .unwrap();
    let s = state(&mut rx, |s| s.prompt.is_some()).await;
    assert!(matches!(s.prompt.unwrap().kind, PromptKind::Display(_)));
    state(&mut rx, |s| !s.busy).await;
    let s = command(&tx, &mut rx, Command::Connect(support::DEVICE.into())).await;
    assert!(s.devices[0].connected);
    let s = command(&tx, &mut rx, Command::Disconnect(support::DEVICE.into())).await;
    assert!(!s.devices[0].connected);
    fixture.data.lock().unwrap().reject = true;
    let s = command(&tx, &mut rx, Command::Connect(support::DEVICE.into())).await;
    assert!(s.error.is_some());
    assert!(!s.devices[0].connected);
    fixture.data.lock().unwrap().reject = false;
    fixture.data.lock().unwrap().connected = true;
    fixture.changed().await;
    state(&mut rx, |s| s.devices.first().is_some_and(|d| d.connected)).await;
    let s = command(&tx, &mut rx, Command::Remove(support::DEVICE.into())).await;
    assert!(s.devices.is_empty());
    let s = command(&tx, &mut rx, Command::Power(support::ADAPTER.into(), false)).await;
    assert!(!s.adapters[0].powered);
    drop(tx);
    tokio::time::timeout(Duration::from_secs(3), task)
        .await
        .unwrap()
        .unwrap();
    assert!(fixture.data.lock().unwrap().agent.is_none());
    assert!(fixture.data.lock().unwrap().scans.is_empty());
}
#[tokio::test]
async fn rejects_foreign_agent_calls_cancels_and_recovers_after_owner_loss() {
    let bus = Bus::start().await;
    let fixture = support::Fixture::start(&bus.address).await;
    let (tx, commands) = mpsc::channel(8);
    let (states, mut rx) = watch::channel(State::default());
    let task = tokio::spawn(cabby_bluetooth::run(
        Endpoint::Private(bus.address.clone()),
        commands,
        states,
    ));
    state(&mut rx, |s| s.available).await;
    let foreign = Builder::address(bus.address.as_str())
        .unwrap()
        .build()
        .await
        .unwrap();
    let (owner, path) = fixture.data.lock().unwrap().agent.clone().unwrap();
    let proxy = Proxy::new(&foreign, owner.as_str(), path.as_str(), "org.bluez.Agent1")
        .await
        .unwrap();
    let result: zbus::Result<()> = proxy
        .call(
            "RequestConfirmation",
            &(
                OwnedObjectPath::try_from(support::DEVICE).unwrap(),
                123456u32,
            ),
        )
        .await;
    assert!(result.is_err());
    for reject in [true, false] {
        tx.send(Command::Pair(support::DEVICE.into()))
            .await
            .unwrap();
        let s = state(&mut rx, |s| s.prompt.is_some()).await;
        if reject {
            tx.send(Command::Reply(s.prompt.unwrap().id, Reply::Reject))
                .await
                .unwrap();
        } else {
            tx.send(Command::CancelPair).await.unwrap();
        }
        let s = state(&mut rx, |s| !s.busy).await;
        assert!(s.prompt.is_none());
        assert!(s.pairing.is_none());
    }
    assert!(fixture.data.lock().unwrap().cancels >= 2);
    command(&tx, &mut rx, Command::Scan(support::ADAPTER.into())).await;
    fixture.connection.clone().close().await.unwrap();
    drop(fixture);
    state(&mut rx, |s| !s.available && s.devices.is_empty()).await;
    let fixture = support::Fixture::start(&bus.address).await;
    state(&mut rx, |s| s.available).await;
    assert!(rx.borrow().scanning.is_none());
    command(&tx, &mut rx, Command::Scan(support::ADAPTER.into())).await;
    // Abrupt UI subscription cancellation must close its unique connection as well.
    task.abort();
    let _ = task.await;
    drop(proxy);
    drop(foreign);
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if fixture.data.lock().unwrap().scans.is_empty()
                && fixture.data.lock().unwrap().agent.is_none()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("discovery or agent survived client cancellation");
}

#[tokio::test]
async fn discovery_expires_without_user_action_and_bus_loss_clears_state() {
    let mut bus = Bus::start().await;
    let fixture = support::Fixture::start(&bus.address).await;
    let (tx, commands) = mpsc::channel(8);
    let (states, mut rx) = watch::channel(State::default());
    let task = tokio::spawn(cabby_bluetooth::run(
        Endpoint::Private(bus.address.clone()),
        commands,
        states,
    ));
    state(&mut rx, |s| s.available).await;
    command(&tx, &mut rx, Command::Scan(support::ADAPTER.into())).await;
    tokio::time::timeout(Duration::from_secs(65), async {
        loop {
            if rx.borrow_and_update().scanning.is_none() {
                break;
            }
            rx.changed().await.unwrap();
        }
    })
    .await
    .expect("discovery exceeded its deadline");
    assert!(fixture.data.lock().unwrap().scans.is_empty());
    assert_eq!(fixture.data.lock().unwrap().stops, 1);
    bus.child.kill().unwrap();
    bus.child.wait().unwrap();
    state(&mut rx, |s| !s.available && s.devices.is_empty()).await;
    drop(tx);
    tokio::time::timeout(Duration::from_secs(6), task)
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn passive_observation_has_battery_without_agent_or_control() {
    let bus = Bus::start().await;
    let fixture = support::Fixture::start(&bus.address).await;
    let (tx, commands) = mpsc::channel(8);
    let (states, mut rx) = watch::channel(State::default());
    let task = tokio::spawn(cabby_bluetooth::run_mode(
        Endpoint::Private(bus.address.clone()),
        commands,
        states,
        false,
    ));
    let s = state(&mut rx, |s| s.available).await;
    assert_eq!(s.devices[0].battery, Some(83));
    assert!(fixture.data.lock().unwrap().agent.is_none());
    tx.send(Command::Scan(support::ADAPTER.into()))
        .await
        .unwrap();
    state(&mut rx, |s| {
        s.error == Some(cabby_bluetooth::Error::Rejected)
    })
    .await;
    assert!(fixture.data.lock().unwrap().scans.is_empty());
    drop(tx);
    tokio::time::timeout(Duration::from_secs(3), task)
        .await
        .unwrap()
        .unwrap();
}
