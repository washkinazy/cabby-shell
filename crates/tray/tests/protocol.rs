mod support;
use cabby_tray::{Command, Handle, Service, State, WATCHER, WATCHER_PATH};
use std::{
    io::{BufRead, BufReader},
    process::{Child, Command as Process, Stdio},
    time::Duration,
};
use zbus::{Connection, Proxy, connection::Builder};
struct Bus {
    child: Child,
    address: String,
}
impl Bus {
    fn new() -> Self {
        let mut child = Process::new("dbus-daemon")
            .args(["--session", "--nofork", "--print-address=1"])
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut address = String::new();
        BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut address)
            .unwrap();
        Self {
            child,
            address: address.trim().into(),
        }
    }
}
impl Drop for Bus {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
async fn until(handle: &mut Handle, predicate: impl Fn(&State) -> bool) -> State {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let s = handle.snapshots.borrow_and_update().clone();
            if predicate(&s) {
                return s;
            }
            handle.snapshots.changed().await.unwrap();
        }
    })
    .await
    .expect("tray state timeout")
}
async fn register(c: &Connection) {
    let p = Proxy::new(c, WATCHER, WATCHER_PATH, WATCHER).await.unwrap();
    let _: () = p
        .call("RegisterStatusNotifierItem", &("/StatusNotifierItem",))
        .await
        .unwrap();
}
async fn events_until(data: &std::sync::Mutex<support::Data>, count: usize) {
    tokio::time::timeout(Duration::from_secs(2), async {
        while data.lock().unwrap().events.len() < count {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn unsupported_activation_uses_lazy_menu_with_paired_lifecycle() {
    let bus = Bus::new();
    let (service, mut handle) = Service::connect(&bus.address).await.unwrap();
    let worker = tokio::spawn(service.run());
    let (client, data) = support::fixture(&bus.address).await;
    {
        let mut data = data.lock().unwrap();
        data.activate_unsupported = true;
        data.require_open = true;
    }
    register(&client).await;
    let token = until(&mut handle, |s| s.items.len() == 1).await.items[0]
        .token
        .clone();
    handle
        .commands
        .send(Command::Activate(token.clone()))
        .await
        .unwrap();
    let state = until(&mut handle, |s| s.menu.is_some() || s.failed).await;
    assert!(!state.failed, "exported menu must replace failed Activate");
    assert_eq!(state.menu.unwrap().entries.len(), 3);
    handle
        .commands
        .send(Command::Click(token.clone(), 3))
        .await
        .unwrap();
    until(&mut handle, |s| {
        s.menu.as_ref().is_some_and(|m| m.parent == 3)
    })
    .await;
    handle
        .commands
        .send(Command::Menu(token.clone(), 0))
        .await
        .unwrap();
    until(&mut handle, |s| {
        s.menu.as_ref().is_some_and(|m| m.parent == 0)
    })
    .await;
    handle.commands.send(Command::CloseMenu).await.unwrap();
    until(&mut handle, |s| s.menu.is_none()).await;
    events_until(&data, 4).await;
    assert_eq!(
        data.lock().unwrap().events,
        [
            (0, "opened".into()),
            (3, "opened".into()),
            (3, "closed".into()),
            (0, "closed".into())
        ]
    );
    handle
        .commands
        .send(Command::Menu(token.clone(), 0))
        .await
        .unwrap();
    until(&mut handle, |s| s.menu.is_some()).await;
    handle
        .commands
        .send(Command::Click(token.clone(), 1))
        .await
        .unwrap();
    until(&mut handle, |s| s.menu.is_none()).await;
    events_until(&data, 7).await;
    assert_eq!(
        &data.lock().unwrap().events[4..],
        [
            (0, "opened".into()),
            (1, "clicked".into()),
            (0, "closed".into())
        ]
    );
    handle.commands.send(Command::Menu(token, 0)).await.unwrap();
    until(&mut handle, |s| s.menu.is_some()).await;
    drop(handle);
    worker.await.unwrap().unwrap();
    events_until(&data, 9).await;
    let data = data.lock().unwrap();
    assert_eq!(data.about_calls, [0, 3]);
    assert_eq!(
        &data.events[7..],
        [(0, "opened".into()), (0, "closed".into())]
    );
}

#[tokio::test]
async fn optional_preparation_cannot_stall_layouts_or_form_signal_loops() {
    let bus = Bus::new();
    let (service, mut handle) = Service::connect(&bus.address).await.unwrap();
    let worker = tokio::spawn(service.run());
    let (client, data) = support::fixture(&bus.address).await;
    data.lock().unwrap().about_stalled = true;
    register(&client).await;
    let token = until(&mut handle, |s| s.items.len() == 1).await.items[0]
        .token
        .clone();
    handle
        .commands
        .send(Command::Menu(token.clone(), 0))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_millis(400), async {
        while data.lock().unwrap().layout_calls.is_empty() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("root GetLayout must not wait for optional AboutToShow");
    tokio::time::timeout(
        Duration::from_secs(1),
        until(&mut handle, |s| s.menu.is_some()),
    )
    .await
    .unwrap();
    client
        .emit_signal(
            None::<&str>,
            "/Menu",
            "com.canonical.dbusmenu",
            "LayoutUpdated",
            &(2u32, 0i32),
        )
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(1), async {
        while data.lock().unwrap().layout_calls.len() < 2 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    handle
        .commands
        .send(Command::Click(token, 3))
        .await
        .unwrap();
    tokio::time::timeout(
        Duration::from_secs(1),
        until(&mut handle, |s| {
            s.menu.as_ref().is_some_and(|m| m.parent == 3)
        }),
    )
    .await
    .unwrap();
    drop(handle);
    worker.await.unwrap().unwrap();
    events_until(&data, 4).await;
    let data = data.lock().unwrap();
    assert_eq!(data.about_calls, [0, 3]);
    assert_eq!(data.layout_calls, [0, 0, 3]);
    assert_eq!(
        data.events,
        [
            (0, "opened".into()),
            (3, "opened".into()),
            (3, "closed".into()),
            (0, "closed".into())
        ]
    );
}

#[tokio::test]
async fn timed_out_activation_falls_back_and_failed_layout_closes_exporter() {
    let bus = Bus::new();
    let (service, mut handle) = Service::connect(&bus.address).await.unwrap();
    let worker = tokio::spawn(service.run());
    let (client, data) = support::fixture(&bus.address).await;
    data.lock().unwrap().activate_stalled = true;
    register(&client).await;
    let token = until(&mut handle, |s| s.items.len() == 1).await.items[0]
        .token
        .clone();
    handle
        .commands
        .send(Command::Activate(token.clone()))
        .await
        .unwrap();
    let state = tokio::time::timeout(
        Duration::from_secs(2),
        until(&mut handle, |s| s.menu.is_some() || s.failed),
    )
    .await
    .unwrap();
    assert!(!state.failed);
    data.lock().unwrap().fail_layout = true;
    handle
        .commands
        .send(Command::Click(token, 3))
        .await
        .unwrap();
    until(&mut handle, |s| s.failed && s.menu.is_none()).await;
    events_until(&data, 4).await;
    assert_eq!(
        data.lock().unwrap().events,
        [
            (0, "opened".into()),
            (3, "opened".into()),
            (3, "closed".into()),
            (0, "closed".into())
        ]
    );
    drop(handle);
    worker.await.unwrap().unwrap();
}
#[tokio::test]
async fn preparation_requesting_an_update_refreshes_the_initial_layout() {
    let bus = Bus::new();
    let (service, mut handle) = Service::connect(&bus.address).await.unwrap();
    let worker = tokio::spawn(service.run());
    let (client, data) = support::fixture(&bus.address).await;
    data.lock().unwrap().about_changes = true;
    register(&client).await;
    let token = until(&mut handle, |s| s.items.len() == 1).await.items[0]
        .token
        .clone();
    handle.commands.send(Command::Menu(token, 0)).await.unwrap();
    let state = until(&mut handle, |s| s.menu.is_some()).await;
    assert_eq!(state.menu.unwrap().entries.len(), 3);
    assert_eq!(data.lock().unwrap().layout_calls, [0, 0]);
    drop(handle);
    worker.await.unwrap().unwrap();
}
#[tokio::test(flavor = "current_thread")]
async fn registration_icons_actions_menus_updates_and_owner_loss() {
    let bus = Bus::new();
    let (service, mut handle) = Service::connect(&bus.address).await.unwrap();
    let worker = tokio::spawn(service.run());
    let (client, data) = support::fixture(&bus.address).await;
    register(&client).await;
    register(&client).await;
    let state = until(&mut handle, |s| s.items.len() == 1).await;
    let item = &state.items[0];
    assert_eq!(
        item.pixmap.as_ref().unwrap().rgba,
        [250, 30, 20, 128, 40, 50, 60, 255]
    );
    let token = item.token.clone();
    handle
        .commands
        .send(Command::Activate(token.clone()))
        .await
        .unwrap();
    handle
        .commands
        .send(Command::Secondary(token.clone()))
        .await
        .unwrap();
    handle
        .commands
        .send(Command::Menu(token.clone(), 0))
        .await
        .unwrap();
    let state = until(&mut handle, |s| s.menu.is_some()).await;
    assert_eq!(data.lock().unwrap().activates, 1);
    assert_eq!(data.lock().unwrap().secondary, 1);
    let entries = &state.menu.unwrap().entries;
    assert_eq!(entries[0].label, "Checked");
    assert_eq!(entries[0].checked, Some(true));
    assert!(!entries[1].enabled);
    handle
        .commands
        .send(Command::Click(token.clone(), 2))
        .await
        .unwrap();
    until(&mut handle, |s| s.failed).await;
    assert!(data.lock().unwrap().clicked.is_empty());
    handle
        .commands
        .send(Command::Click(token.clone(), 3))
        .await
        .unwrap();
    until(&mut handle, |s| {
        s.menu.as_ref().is_some_and(|m| m.parent == 3)
    })
    .await;
    handle
        .commands
        .send(Command::Click(token.clone(), 4))
        .await
        .unwrap();
    until(&mut handle, |s| s.menu.is_none()).await;
    tokio::time::timeout(Duration::from_secs(3), async {
        while data.lock().unwrap().clicked.is_empty() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(data.lock().unwrap().clicked, vec![4]);
    data.lock().unwrap().status = "Passive".into();
    client
        .emit_signal(
            None::<&str>,
            "/StatusNotifierItem",
            cabby_tray::ITEM,
            "NewStatus",
            &("Passive",),
        )
        .await
        .unwrap();
    until(&mut handle, |s| s.items[0].passive).await;
    client.close().await.unwrap();
    until(&mut handle, |s| s.items.is_empty()).await;
    handle
        .commands
        .send(Command::Activate(token))
        .await
        .unwrap();
    until(&mut handle, |s| s.failed).await;
    drop(handle);
    assert!(worker.await.unwrap().is_ok());
}
#[tokio::test(flavor = "current_thread")]
async fn uses_existing_watcher_and_rejects_forged_registration() {
    let bus = Bus::new();
    let (first, mut first_handle) = Service::connect(&bus.address).await.unwrap();
    let first_worker = tokio::spawn(first.run());
    let (client, _) = support::fixture(&bus.address).await;
    register(&client).await;
    until(&mut first_handle, |s| s.items.len() == 1).await;
    let (second, mut second_handle) = Service::connect(&bus.address).await.unwrap();
    let second_worker = tokio::spawn(second.run());
    until(&mut second_handle, |s| s.items.len() == 1).await;
    let outsider = Builder::address(bus.address.as_str())
        .unwrap()
        .build()
        .await
        .unwrap();
    let proxy = Proxy::new(&outsider, WATCHER, WATCHER_PATH, WATCHER)
        .await
        .unwrap();
    let result: zbus::Result<()> = proxy
        .call(
            "RegisterStatusNotifierItem",
            &(client.unique_name().unwrap().as_str(),),
        )
        .await;
    assert!(result.is_err());
    client.close().await.unwrap();
    until(&mut second_handle, |s| s.items.is_empty()).await;
    drop(first_handle);
    assert!(first_worker.await.unwrap().is_ok());
    assert!(second_worker.await.unwrap().is_err());
    assert!(!second_handle.snapshots.borrow().available);
    // A fresh host can acquire the released watcher immediately.
    let (third, handle) = Service::connect(&bus.address).await.unwrap();
    drop(handle);
    assert!(third.run().await.is_ok());
}

#[tokio::test]
async fn delayed_exports_retry_without_signals_and_removed_owners_stay_removed() {
    let bus = Bus::new();
    let (service, mut handle) = Service::connect(&bus.address).await.unwrap();
    let worker = tokio::spawn(service.run());
    let client = support::unready(&bus.address).await;
    register(&client).await;
    tokio::time::sleep(Duration::from_millis(400)).await;
    support::export_item(&client).await;
    let state = until(&mut handle, |s| s.items.len() == 1).await;
    assert_eq!(state.items[0].title, "Fixture");
    client.close().await.unwrap();
    until(&mut handle, |s| s.items.is_empty()).await;
    let gone = support::unready(&bus.address).await;
    register(&gone).await;
    gone.close().await.unwrap();
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert!(handle.snapshots.borrow().items.is_empty());
    drop(handle);
    worker.await.unwrap().unwrap();
}

#[tokio::test]
async fn exhausted_metadata_retries_wait_for_a_new_signal() {
    let bus = Bus::new();
    let (service, mut handle) = Service::connect(&bus.address).await.unwrap();
    let worker = tokio::spawn(service.run());
    let client = support::unready(&bus.address).await;
    register(&client).await;
    tokio::time::sleep(Duration::from_secs(3)).await;
    support::export_item(&client).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(handle.snapshots.borrow().items.is_empty());
    client
        .emit_signal(
            None::<&str>,
            "/StatusNotifierItem",
            cabby_tray::ITEM,
            "NewIcon",
            &(),
        )
        .await
        .unwrap();
    until(&mut handle, |s| s.items.len() == 1).await;
    drop(handle);
    worker.await.unwrap().unwrap();
}
