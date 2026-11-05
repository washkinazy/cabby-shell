use cabby_notification_service::{Command, Error, Handle, NAME, PATH, Service};
use cabby_notifications::{Request, Urgency};
use futures_util::StreamExt;
use std::{
    collections::HashMap,
    process::{Child, Command as Process, Stdio},
    time::Duration,
};
use zbus::{
    Connection, MatchRule, MessageStream, Proxy, connection::Builder, fdo::DBusProxy,
    message::Type, zvariant::OwnedValue,
};

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
            .expect("dbus-daemon test prerequisite");
        let mut bus = Self {
            child,
            _directory: directory,
            address,
        };
        tokio::time::timeout(Duration::from_secs(3), async {
            while !socket.exists() {
                assert!(
                    bus.child.try_wait().unwrap().is_none(),
                    "private bus exited"
                );
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("private bus readiness timeout");
        bus
    }
    async fn client(&self) -> Connection {
        Builder::address(self.address.as_str())
            .unwrap()
            .method_timeout(Duration::from_secs(3))
            .build()
            .await
            .unwrap()
    }
}
fn request() -> Request {
    Request {
        app_name: "Synthetic test".into(),
        summary: "Synthetic summary".into(),
        ..Request::default()
    }
}
async fn notify(
    proxy: &Proxy<'_>,
    request: Request,
    hints: HashMap<&str, OwnedValue>,
) -> zbus::Result<u32> {
    proxy
        .call(
            "Notify",
            &(
                request.app_name,
                request.replaces_id,
                "",
                request.summary,
                request.body,
                request.actions,
                hints,
                request.expire_timeout,
            ),
        )
        .await
}
fn error_name<T>(result: zbus::Result<T>) -> String {
    match result {
        Err(zbus::Error::MethodError(name, _, _)) => name.to_string(),
        _ => panic!("expected D-Bus method error"),
    }
}
async fn signals(connection: &Connection) -> MessageStream {
    let rule = MatchRule::builder()
        .msg_type(Type::Signal)
        .interface(NAME)
        .unwrap()
        .path(PATH)
        .unwrap()
        .build();
    MessageStream::for_match_rule(rule, connection, Some(16))
        .await
        .unwrap()
}
async fn signal(stream: &mut MessageStream, member: &str) -> zbus::Message {
    let message = tokio::time::timeout(Duration::from_secs(3), stream.next())
        .await
        .expect("signal timeout")
        .unwrap()
        .unwrap();
    assert_eq!(message.header().member().unwrap().as_str(), member);
    message
}
fn token(handle: &Handle, id: u32) -> cabby_notifications::Token {
    handle
        .snapshots
        .borrow()
        .iter()
        .find(|e| e.token().id() == id)
        .unwrap()
        .token()
}

#[tokio::test]
async fn methods_validation_bounds_and_exclusive_ownership() {
    tokio::time::timeout(Duration::from_secs(15), async {
        let bus = Bus::start().await;
        let (service, handle) = Service::connect(&bus.address).await.unwrap();
        let task = tokio::spawn(service.run());
        let connection = bus.client().await;
        let proxy = Proxy::new(&connection, NAME, PATH, NAME).await.unwrap();
        let dbus = DBusProxy::new(&connection).await.unwrap();
        let owner = dbus.get_name_owner(NAME.try_into().unwrap()).await.unwrap();
        assert!(matches!(
            Service::connect(&bus.address).await,
            Err(Error::Conflict)
        ));
        assert_eq!(
            dbus.get_name_owner(NAME.try_into().unwrap()).await.unwrap(),
            owner
        );
        let capabilities: Vec<String> = proxy.call("GetCapabilities", &()).await.unwrap();
        assert!(capabilities.is_empty());
        let info: (String, String, String, String) =
            proxy.call("GetServerInformation", &()).await.unwrap();
        assert_eq!(info.0, "Cabby");
        assert_eq!(info.3, "1.3");
        let introspect = Proxy::new(
            &connection,
            NAME,
            PATH,
            "org.freedesktop.DBus.Introspectable",
        )
        .await
        .unwrap();
        let xml: String = introspect.call("Introspect", &()).await.unwrap();
        assert!(xml.contains("type=\"a{sv}\""));
        assert!(xml.contains("NotificationClosed"));
        assert_eq!(
            error_name(proxy.call::<_, _, u32>("Notify", &()).await),
            "org.freedesktop.DBus.Error.InvalidArgs"
        );
        assert_eq!(
            error_name(proxy.call::<_, _, ()>("Unknown", &()).await),
            "org.freedesktop.DBus.Error.UnknownMethod"
        );
        let id = notify(&proxy, request(), HashMap::new()).await.unwrap();
        let original = token(&handle, id);
        let replacement = Request {
            replaces_id: id,
            body: "<b>literal</b>\ntext".into(),
            ..request()
        };
        assert_eq!(
            notify(&proxy, replacement, HashMap::new()).await.unwrap(),
            id
        );
        assert_ne!(token(&handle, id), original);
        assert_eq!(handle.snapshots.borrow()[0].body, "<b>literal</b>\ntext");
        let malformed = Request {
            replaces_id: id,
            actions: vec!["unpaired".into()],
            ..request()
        };
        assert_eq!(
            error_name(notify(&proxy, malformed, HashMap::new()).await),
            "org.freedesktop.DBus.Error.InvalidArgs"
        );
        assert_eq!(handle.snapshots.borrow().len(), 1);
        let oversized = Request {
            body: "x".repeat(65 * 1024),
            ..request()
        };
        assert_eq!(
            error_name(notify(&proxy, oversized, HashMap::new()).await),
            "org.freedesktop.DBus.Error.LimitsExceeded"
        );
        let oversized_field = Request {
            body: "x".repeat(17 * 1024),
            ..request()
        };
        assert_eq!(
            error_name(notify(&proxy, oversized_field, HashMap::new()).await),
            "org.freedesktop.DBus.Error.InvalidArgs"
        );
        // Unknown variants and wrongly typed optional hints are ignored.
        let hints = HashMap::from([
            ("urgency", OwnedValue::from(22i32)),
            ("resident", OwnedValue::from(3u8)),
            ("future", OwnedValue::from(true)),
        ]);
        let other = notify(&proxy, request(), hints).await.unwrap();
        let snapshots = handle.snapshots.borrow();
        let entry = snapshots.iter().find(|e| e.token().id() == other).unwrap();
        assert_eq!(entry.urgency, Urgency::Normal);
        assert!(!entry.resident);
        drop(snapshots);
        for _ in 2..64 {
            notify(&proxy, request(), HashMap::new()).await.unwrap();
        }
        assert_eq!(
            error_name(notify(&proxy, request(), HashMap::new()).await),
            "org.freedesktop.DBus.Error.LimitsExceeded"
        );
        assert_eq!(
            notify(
                &proxy,
                Request {
                    replaces_id: id,
                    ..request()
                },
                HashMap::new()
            )
            .await
            .unwrap(),
            id
        );
        assert_eq!(handle.snapshots.borrow().len(), 64);
        drop(handle);
        task.await.unwrap().unwrap();
        // The failed contender did not queue and take over on shutdown.
        assert!(dbus.get_name_owner(NAME.try_into().unwrap()).await.is_err());
        let (fresh, fresh_handle) = Service::connect(&bus.address).await.unwrap();
        assert!(fresh_handle.snapshots.borrow().is_empty());
        drop(fresh_handle);
        fresh.run().await.unwrap();
    })
    .await
    .expect("private-bus test timeout");
}

#[tokio::test]
async fn signals_presentation_actions_replacement_and_disconnect() {
    tokio::time::timeout(Duration::from_secs(15), async {
        let mut bus = Bus::start().await;
        let (service, mut handle) = Service::connect(&bus.address).await.unwrap();
        let task = tokio::spawn(service.run());
        let connection = bus.client().await;
        let proxy = Proxy::new(&connection, NAME, PATH, NAME).await.unwrap();
        let mut stream = signals(&connection).await;
        let id = notify(
            &proxy,
            Request {
                expire_timeout: 10,
                ..request()
            },
            HashMap::new(),
        )
        .await
        .unwrap();
        assert!(!handle.snapshots.borrow()[0].presented());
        let old = token(&handle, id);
        notify(
            &proxy,
            Request {
                replaces_id: id,
                expire_timeout: 10,
                ..request()
            },
            HashMap::new(),
        )
        .await
        .unwrap();
        assert!(matches!(
            handle.command(Command::Dismiss(old)).await,
            Err(Error::Policy(_))
        ));
        handle
            .command(Command::Presented(token(&handle, id)))
            .await
            .unwrap();
        let expired = signal(&mut stream, "NotificationClosed").await;
        assert_eq!(expired.body().deserialize::<(u32, u32)>().unwrap(), (id, 1));
        assert_eq!(
            error_name(proxy.call::<_, _, ()>("CloseNotification", &id).await),
            "org.freedesktop.DBus.Error.InvalidArgs"
        );
        let action = Request {
            actions: vec!["default".into(), "Open".into()],
            ..request()
        };
        let id = notify(&proxy, action.clone(), HashMap::new())
            .await
            .unwrap();
        let current = token(&handle, id);
        handle.command(Command::Presented(current)).await.unwrap();
        handle
            .command(Command::Invoke(current, "default".into()))
            .await
            .unwrap();
        assert_eq!(
            signal(&mut stream, "ActionInvoked")
                .await
                .body()
                .deserialize::<(u32, String)>()
                .unwrap(),
            (id, "default".into())
        );
        assert_eq!(
            signal(&mut stream, "NotificationClosed")
                .await
                .body()
                .deserialize::<(u32, u32)>()
                .unwrap(),
            (id, 2)
        );
        let id = notify(
            &proxy,
            action,
            HashMap::from([
                ("resident", OwnedValue::from(true)),
                ("urgency", OwnedValue::from(2u8)),
            ]),
        )
        .await
        .unwrap();
        let current = token(&handle, id);
        handle.command(Command::Presented(current)).await.unwrap();
        handle
            .command(Command::Invoke(current, "default".into()))
            .await
            .unwrap();
        signal(&mut stream, "ActionInvoked").await;
        assert_eq!(handle.snapshots.borrow()[0].urgency, Urgency::Critical);
        assert_eq!(handle.snapshots.borrow().len(), 1);
        proxy
            .call::<_, _, ()>("CloseNotification", &id)
            .await
            .unwrap();
        assert_eq!(
            signal(&mut stream, "NotificationClosed")
                .await
                .body()
                .deserialize::<(u32, u32)>()
                .unwrap(),
            (id, 3)
        );
        notify(&proxy, request(), HashMap::new()).await.unwrap();
        handle.snapshots.borrow_and_update();
        bus.child.kill().unwrap();
        bus.child.wait().unwrap();
        assert_eq!(task.await.unwrap(), Err(Error::Transport));
        assert!(handle.snapshots.borrow().is_empty());
        assert_eq!(
            handle.command(Command::Dismiss(current)).await,
            Err(Error::Stopped)
        );
    })
    .await
    .expect("private-bus test timeout");
}

#[tokio::test]
async fn no_reply_calls_and_task_cancellation_release_resources() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let bus = Bus::start().await;
        let (service, handle) = Service::connect(&bus.address).await.unwrap();
        let task = tokio::spawn(service.run());
        let connection = bus.client().await;
        let proxy = Proxy::new(&connection, NAME, PATH, NAME).await.unwrap();
        let dbus = DBusProxy::new(&connection).await.unwrap();
        let owner = dbus.get_name_owner(NAME.try_into().unwrap()).await.unwrap();
        let id = notify(&proxy, request(), HashMap::new()).await.unwrap();
        let rule = MatchRule::builder()
            .msg_type(Type::MethodReturn)
            .sender(owner.as_str())
            .unwrap()
            .build();
        let mut replies = MessageStream::for_match_rule(rule, &connection, Some(1))
            .await
            .unwrap();
        let mut events = signals(&connection).await;
        proxy.call_noreply("CloseNotification", &id).await.unwrap();
        assert_eq!(
            signal(&mut events, "NotificationClosed")
                .await
                .body()
                .deserialize::<(u32, u32)>()
                .unwrap(),
            (id, 3)
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(50), replies.next())
                .await
                .is_err()
        );
        drop(replies);
        notify(&proxy, request(), HashMap::new()).await.unwrap();
        assert_eq!(handle.snapshots.borrow().len(), 1);
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        assert!(handle.snapshots.borrow().is_empty());
        // A round trip may race bus cleanup after the client socket closes.
        tokio::time::timeout(Duration::from_secs(2), async {
            while dbus.get_name_owner(NAME.try_into().unwrap()).await.is_ok() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("cancelled service retained name");
    })
    .await
    .expect("private-bus cancellation test timeout");
}
