#[path = "../tests/support.rs"]
mod support;
#[tokio::main(flavor = "current_thread")]
async fn main() {
    let address = std::env::var("CABBY_TEST_BLUEZ_BUS").expect("private bus required");
    let guard = std::env::var("CABBY_TEST_WAYLAND").expect("disposable display required");
    assert!(!guard.is_empty());
    assert_eq!(Some(guard), std::env::var("WAYLAND_DISPLAY").ok());
    let fixture = support::Fixture::start(&address).await;
    println!("fixture-ready");
    std::future::pending::<()>().await;
    drop(fixture);
}
