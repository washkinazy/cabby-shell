//! Runtime status belongs to the coordinator; this contribution only observes
//! atomic snapshots and requests a manual-inhibitor toggle.
use serde::Deserialize;
#[cfg(test)]
use std::path::PathBuf;
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
                "suspending" => "Suspending",
                "suspend-requested" => "Sleep requested",
                "suspend-failed" => "Suspend failed",
                "locking" => "Locking",
                "lock-failed" | "display-failed" => "Idle failed",
                _ => "Idle ?",
            }
        }
    }
}
pub(super) fn events() -> impl iced::futures::Stream<Item = Status> {
    crate::runtime_status::events("idle.json")
}
#[cfg(test)]
fn updates(path: Option<PathBuf>) -> impl iced::futures::Stream<Item = Status> {
    crate::runtime_status::updates(path)
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
