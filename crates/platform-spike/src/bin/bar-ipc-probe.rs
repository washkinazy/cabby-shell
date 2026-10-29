//! Destructive only to a guarded disposable compositor; never the live session.
use cabby_platform_spike::niri::{Events, Model, focus};
use std::{path::PathBuf, time::Duration};
#[tokio::main(flavor = "current_thread")]
async fn main() {
    let guard = std::env::var("CABBY_TEST_WAYLAND").expect("disposable session guard required");
    assert!(!guard.is_empty());
    assert_eq!(Some(guard), std::env::var("WAYLAND_DISPLAY").ok());
    let path = PathBuf::from(std::env::var_os("NIRI_SOCKET").expect("nested socket"));
    tokio::time::timeout(Duration::from_secs(10), async {
        let mut events = Events::connect(&path).await.unwrap();
        let mut model = Model::default();
        while !model.connected {
            model.apply(events.next().await.unwrap());
        }
        let target = model
            .workspaces
            .values()
            .find(|w| !w.is_focused)
            .expect("two test workspaces")
            .id;
        focus(&path, target).await.unwrap();
        while !model.workspaces[&target].is_focused {
            model.apply(events.next().await.unwrap());
        }
        println!("workspace event snapshot and ID-based action passed");
    })
    .await
    .expect("IPC probe timed out");
}
