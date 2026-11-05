//! Runtime status belongs to the coordinator; this contribution only observes
//! atomic snapshots and requests a manual-inhibitor toggle.
use iced::futures::SinkExt;
use serde::Deserialize;
use std::{
    io::Read,
    path::{Path, PathBuf},
};
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub(crate) struct Status {
    state: String,
    inhibited: bool,
    lock_seconds: u32,
}
impl Status {
    pub fn label(&self) -> &'static str {
        if self.state.is_empty() {
            "Idle ?"
        } else if self.inhibited {
            "Keep awake"
        } else if self.lock_seconds == 0 {
            "Idle disabled"
        } else {
            match self.state.as_str() {
                "watching" => "Idle lock",
                "locking" => "Locking",
                "lock-failed" => "Lock failed",
                _ => "Idle ?",
            }
        }
    }
}
fn load(path: &Path) -> Status {
    use std::os::unix::fs::OpenOptionsExt;
    let Ok(file) = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW)
        .open(path)
    else {
        return Status::default();
    };
    if !file.metadata().is_ok_and(|m| m.is_file()) {
        return Status::default();
    }
    let mut bytes = Vec::new();
    if file.take(1025).read_to_end(&mut bytes).is_err() || bytes.len() > 1024 {
        return Status::default();
    }
    serde_json::from_slice(&bytes).unwrap_or_default()
}
pub(super) fn events() -> impl iced::futures::Stream<Item = Status> {
    let path =
        std::env::var_os("XDG_RUNTIME_DIR").map(|p| PathBuf::from(p).join("cabby-shell/idle.json"));
    updates(path)
}
fn updates(path: Option<PathBuf>) -> impl iced::futures::Stream<Item = Status> {
    iced::stream::channel(
        1,
        move |mut output: iced::futures::channel::mpsc::Sender<Status>| async move {
            let Some(path) = path else {
                let _ = output.send(Status::default()).await;
                std::future::pending::<()>().await;
                return;
            };
            loop {
                let Ok(watch) = cabby_config::watch::Watch::new([path.clone()]) else {
                    return;
                };
                let Ok(mut watch) = (unsafe { tokio::io::unix::AsyncFd::register(watch) }) else {
                    return;
                };
                let input = path.clone();
                let state = tokio::task::spawn_blocking(move || load(&input))
                    .await
                    .unwrap_or_default();
                if output.send(state).await.is_err() {
                    return;
                }
                loop {
                    let Ok(mut ready) = watch.readable_mut().await else {
                        return;
                    };
                    ready.clear_ready();
                    drop(ready);
                    match watch.get_mut().drain() {
                        Ok(true) => break,
                        Ok(false) => (),
                        Err(_) => return,
                    }
                }
                tokio::time::sleep(cabby_config::watch::DEBOUNCE).await;
            }
        },
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    use iced::futures::StreamExt;
    #[tokio::test]
    async fn runtime_replacements_inhibition_invalid_and_removal_update_labels() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("idle.json");
        let mut stream = Box::pin(updates(Some(path.clone())));
        assert_eq!(stream.next().await.unwrap().label(), "Idle ?");
        let replace = |value: &str| {
            std::fs::write(root.path().join("temp"), value).unwrap();
            std::fs::rename(root.path().join("temp"), &path).unwrap();
        };
        replace(r#"{"state":"watching","inhibited":false,"lock_seconds":600}"#);
        assert_eq!(stream.next().await.unwrap().label(), "Idle lock");
        replace(r#"{"state":"inhibited","inhibited":true,"lock_seconds":600}"#);
        assert_eq!(stream.next().await.unwrap().label(), "Keep awake");
        replace("invalid");
        assert_eq!(stream.next().await.unwrap().label(), "Idle ?");
        std::fs::remove_file(path).unwrap();
        assert_eq!(stream.next().await.unwrap().label(), "Idle ?");
    }
}
