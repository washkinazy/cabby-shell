#[path = "../tests/support.rs"]
mod support;
#[tokio::main(flavor = "current_thread")]
async fn main() {
    let guard = std::env::var("CABBY_TEST_WAYLAND").expect("disposable display required");
    assert!(!guard.is_empty());
    assert_eq!(Some(guard), std::env::var("WAYLAND_DISPLAY").ok());
    let address = std::env::var("DBUS_SESSION_BUS_ADDRESS").expect("private bus required");
    let (connection, data) = support::fixture(&address).await;
    {
        let mut data = data.lock().unwrap();
        data.icon = std::env::var("CABBY_TEST_TRAY_ICON").ok();
        data.activate_unsupported = true;
        data.require_open = true;
    }
    connection
        .request_name("org.cabby.TrayFixture")
        .await
        .unwrap();
    // The harness starts the host before this fixture.
    let proxy = zbus::Proxy::new(
        &connection,
        cabby_tray::WATCHER,
        cabby_tray::WATCHER_PATH,
        cabby_tray::WATCHER,
    )
    .await
    .unwrap();
    let _: () = proxy
        .call("RegisterStatusNotifierItem", &("/StatusNotifierItem",))
        .await
        .unwrap();
    println!("fixture-ready");
    std::future::pending::<()>().await;
}
