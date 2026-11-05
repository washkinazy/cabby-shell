//! Synthetic private-session probe; never use against the normal session bus.
use cabby_notification_service::{NAME, PATH};
use futures_util::StreamExt;
use std::{collections::HashMap, time::Duration};
use zbus::{Proxy, connection::Builder, zvariant::OwnedValue};
#[tokio::main(flavor = "current_thread")]
async fn main() {
    if std::env::var("CABBY_TEST_WAYLAND")
        .ok()
        .filter(|s| !s.is_empty())
        != std::env::var("WAYLAND_DISPLAY").ok()
        || std::env::var("CABBY_TEST_WAYLAND").is_err()
    {
        eprintln!("requires disposable compositor guard");
        std::process::exit(2);
    }
    if tokio::time::timeout(Duration::from_secs(10), run())
        .await
        .is_err()
    {
        panic!("notification probe timed out");
    }
}
async fn run() {
    let address = std::env::var("DBUS_SESSION_BUS_ADDRESS").expect("private bus address");
    let connection = Builder::address(address.as_str())
        .unwrap()
        .method_timeout(Duration::from_secs(5))
        .build()
        .await
        .unwrap();
    let proxy = Proxy::new(&connection, NAME, PATH, NAME).await.unwrap();
    let args: Vec<_> = std::env::args().skip(1).collect();
    let caps: Vec<String> = proxy.call("GetCapabilities", &()).await.unwrap();
    assert_eq!(caps, ["body", "actions"]);
    match args.first().map(String::as_str) {
        Some("notify") | Some("expire") => {
            let replaces: u32 = args.get(1).map(|s| s.parse().unwrap()).unwrap_or(0);
            let expiry = if args[0] == "expire" { 100i32 } else { 0 };
            let mut signals = proxy.receive_signal("NotificationClosed").await.unwrap();
            let id: u32 = proxy
                .call(
                    "Notify",
                    &(
                        "Cabby synthetic fixture",
                        replaces,
                        "",
                        "Notification test",
                        "Plain text <b>fixture</b>\nBody scrolls inside the card.",
                        vec!["default", "Open fixture"],
                        HashMap::<&str, OwnedValue>::new(),
                        expiry,
                    ),
                )
                .await
                .unwrap();
            if expiry > 0 {
                let message = signals.next().await.unwrap();
                assert_eq!(message.body().deserialize::<(u32, u32)>().unwrap(), (id, 1));
            }
            println!("{id}");
        }
        Some("close") => {
            let id: u32 = args[1].parse().unwrap();
            proxy
                .call::<_, _, ()>("CloseNotification", &id)
                .await
                .unwrap();
        }
        _ => panic!("expected notify/expire/close"),
    }
}
