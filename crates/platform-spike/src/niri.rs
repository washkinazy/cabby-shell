//! Event-driven subset of niri's public JSON IPC. Unknown events are ignored;
//! malformed known events disconnect so a fresh snapshot can restore consistency.
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

#[derive(Debug, Clone, Deserialize)]
pub struct Workspace {
    pub id: u64,
    pub idx: u8,
    pub name: Option<String>,
    pub output: Option<String>,
    pub is_active: bool,
    pub is_focused: bool,
    pub active_window_id: Option<u64>,
    #[serde(default)]
    pub is_urgent: bool,
}
#[derive(Debug, Clone, Deserialize)]
pub struct Window {
    pub id: u64,
    pub title: Option<String>,
    pub app_id: Option<String>,
    pub is_focused: bool,
}
#[derive(Debug, Clone, Deserialize)]
pub enum Event {
    WorkspacesChanged {
        workspaces: Vec<Workspace>,
    },
    WorkspaceActivated {
        id: u64,
        focused: bool,
    },
    WorkspaceUrgencyChanged {
        id: u64,
        urgent: bool,
    },
    WorkspaceActiveWindowChanged {
        workspace_id: u64,
        active_window_id: Option<u64>,
    },
    WindowsChanged {
        windows: Vec<Window>,
    },
    WindowOpenedOrChanged {
        window: Window,
    },
    WindowClosed {
        id: u64,
    },
    WindowFocusChanged {
        id: Option<u64>,
    },
    Ignored,
}
impl Event {
    pub fn parse(line: &str) -> Result<Self> {
        let value: serde_json::Value =
            serde_json::from_str(line).map_err(|_| "invalid niri JSON")?;
        let map = value
            .as_object()
            .filter(|m| m.len() == 1)
            .ok_or("invalid event envelope")?;
        let name = map.keys().next().unwrap().as_str();
        if !matches!(
            name,
            "WorkspacesChanged"
                | "WorkspaceActivated"
                | "WorkspaceUrgencyChanged"
                | "WorkspaceActiveWindowChanged"
                | "WindowsChanged"
                | "WindowOpenedOrChanged"
                | "WindowClosed"
                | "WindowFocusChanged"
        ) {
            return Ok(Self::Ignored);
        }
        let name = name.to_owned();
        serde_json::from_value(value).map_err(|_| format!("invalid niri event: {name}"))
    }
}
#[derive(Debug, Clone, Default)]
pub struct Model {
    pub connected: bool,
    pub workspaces: BTreeMap<u64, Workspace>,
    pub windows: BTreeMap<u64, Window>,
}
impl Model {
    pub fn disconnect(&mut self) {
        *self = Self::default();
    }
    pub fn apply(&mut self, event: Event) {
        match event {
            Event::WorkspacesChanged { workspaces } => {
                self.workspaces = workspaces.into_iter().map(|w| (w.id, w)).collect();
                self.connected = true;
            }
            Event::WorkspaceActivated { id, focused } => {
                if let Some(selected) = self.workspaces.get(&id) {
                    let output = selected.output.clone();
                    for w in self.workspaces.values_mut() {
                        if w.output == output {
                            w.is_active = w.id == id;
                        }
                        if focused {
                            w.is_focused = w.id == id;
                        }
                    }
                }
            }
            Event::WorkspaceUrgencyChanged { id, urgent } => {
                if let Some(w) = self.workspaces.get_mut(&id) {
                    w.is_urgent = urgent;
                }
            }
            Event::WorkspaceActiveWindowChanged {
                workspace_id,
                active_window_id,
            } => {
                if let Some(w) = self.workspaces.get_mut(&workspace_id) {
                    w.active_window_id = active_window_id;
                }
            }
            Event::WindowsChanged { windows } => {
                self.windows = windows.into_iter().map(|w| (w.id, w)).collect();
            }
            Event::WindowOpenedOrChanged { window } => {
                if window.is_focused {
                    for w in self.windows.values_mut() {
                        w.is_focused = false;
                    }
                }
                self.windows.insert(window.id, window);
            }
            Event::WindowClosed { id } => {
                self.windows.remove(&id);
            }
            Event::WindowFocusChanged { id } => {
                for w in self.windows.values_mut() {
                    w.is_focused = Some(w.id) == id;
                }
            }
            Event::Ignored => (),
        }
    }
    pub fn on_output(&self, output: &str) -> Vec<&Workspace> {
        let mut workspaces: Vec<_> = self
            .workspaces
            .values()
            .filter(|w| w.output.as_deref() == Some(output))
            .collect();
        workspaces.sort_by_key(|w| (w.idx, w.id));
        workspaces
    }
    pub fn title(&self, output: &str) -> String {
        self.on_output(output)
            .into_iter()
            .find(|w| w.is_active)
            .and_then(|w| w.active_window_id)
            .and_then(|id| self.windows.get(&id))
            .map(|w| {
                label(
                    w.title.as_deref().or(w.app_id.as_deref()).unwrap_or(""),
                    100,
                )
            })
            .unwrap_or_default()
    }
}
/// Bound display width and remove control characters without slicing UTF-8.
pub fn label(value: &str, max: usize) -> String {
    let mut chars = value.chars().filter(|c| !c.is_control());
    let mut result: String = chars.by_ref().take(max).collect();
    if chars.next().is_some() {
        result.push('…');
    }
    result
}
const MAX_FRAME: u64 = 1024 * 1024;
type Result<T> = std::result::Result<T, String>;
pub struct Events {
    reader: BufReader<UnixStream>,
}
async fn frame(reader: &mut BufReader<UnixStream>) -> Result<String> {
    let mut bytes = Vec::new();
    reader
        .take(MAX_FRAME + 1)
        .read_until(b'\n', &mut bytes)
        .await
        .map_err(|_| "IPC read failed")?;
    if bytes.len() as u64 > MAX_FRAME || bytes.last() != Some(&b'\n') {
        return Err("IPC ended or frame exceeded 1 MiB".into());
    }
    String::from_utf8(bytes).map_err(|_| "IPC frame was not UTF-8".into())
}
async fn open(path: &Path, request: &serde_json::Value) -> Result<BufReader<UnixStream>> {
    let mut stream = UnixStream::connect(path)
        .await
        .map_err(|_| "niri unavailable")?;
    stream
        .write_all(format!("{request}\n").as_bytes())
        .await
        .map_err(|_| "IPC write failed")?;
    let mut reader = BufReader::new(stream);
    let response: serde_json::Value =
        serde_json::from_str(&frame(&mut reader).await?).map_err(|_| "invalid IPC response")?;
    if response.get("Ok") != Some(&serde_json::json!("Handled")) {
        return Err("niri rejected request".into());
    }
    Ok(reader)
}
impl Events {
    pub async fn connect(path: &Path) -> Result<Self> {
        tokio::time::timeout(
            Duration::from_secs(3),
            open(path, &serde_json::json!("EventStream")),
        )
        .await
        .map_err(|_| "niri handshake timed out")?
        .map(|reader| Self { reader })
    }
    pub async fn next(&mut self) -> Result<Event> {
        Event::parse(&frame(&mut self.reader).await?)
    }
}
pub async fn focus(path: &Path, id: u64) -> Result<()> {
    let request = serde_json::json!({"Action":{"FocusWorkspace":{"reference":{"Id":id}}}});
    tokio::time::timeout(Duration::from_secs(3), open(path, &request))
        .await
        .map_err(|_| "workspace action timed out")??;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn workspace(id: u64, idx: u8, output: &str, active: bool) -> Workspace {
        Workspace {
            id,
            idx,
            name: None,
            output: Some(output.into()),
            is_active: active,
            is_focused: active && output == "A",
            active_window_id: None,
            is_urgent: false,
        }
    }
    #[test]
    fn output_changes_and_activation_preserve_other_outputs() {
        let mut m = Model::default();
        m.apply(Event::WorkspacesChanged {
            workspaces: vec![
                workspace(20, 2, "A", false),
                workspace(10, 1, "A", true),
                workspace(30, 1, "B", true),
            ],
        });
        assert_eq!(
            m.on_output("A").iter().map(|w| w.id).collect::<Vec<_>>(),
            vec![10, 20]
        );
        m.apply(Event::WorkspaceActivated {
            id: 20,
            focused: true,
        });
        assert!(!m.workspaces[&10].is_active);
        assert!(m.workspaces[&20].is_focused && m.workspaces[&30].is_active);
        m.apply(Event::WorkspaceActivated {
            id: 30,
            focused: false,
        });
        assert!(m.workspaces[&20].is_focused);
        m.apply(Event::WorkspaceActivated {
            id: 999,
            focused: true,
        });
        assert!(m.workspaces[&20].is_focused);
        m.apply(Event::WorkspacesChanged {
            workspaces: vec![workspace(30, 1, "A", true)],
        });
        assert!(m.on_output("B").is_empty());
        assert_eq!(m.on_output("A")[0].id, 30);
        m.disconnect();
        assert!(!m.connected && m.workspaces.is_empty() && m.windows.is_empty());
    }
    #[test]
    fn titles_handle_out_of_order_changes_closure_and_reconnect() {
        let mut m = Model::default();
        let mut w = workspace(1, 1, "A", true);
        w.active_window_id = Some(7);
        m.apply(Event::WorkspacesChanged {
            workspaces: vec![w],
        });
        assert_eq!(m.title("A"), "");
        m.apply(Event::WindowOpenedOrChanged {
            window: Window {
                id: 7,
                title: Some("hello\nworld".into()),
                app_id: None,
                is_focused: true,
            },
        });
        assert_eq!(m.title("A"), "helloworld");
        m.apply(Event::WindowOpenedOrChanged {
            window: Window {
                id: 7,
                title: None,
                app_id: Some("app".into()),
                is_focused: true,
            },
        });
        assert_eq!(m.title("A"), "app");
        m.apply(Event::WindowClosed { id: 7 });
        assert_eq!(m.title("A"), "");
        m.disconnect();
        m.apply(Event::WorkspacesChanged {
            workspaces: vec![workspace(2, 1, "B", true)],
        });
        assert!(m.connected && m.on_output("A").is_empty());
        assert_eq!(label("ééé", 2), "éé…");
    }
    #[test]
    fn unknown_events_are_forward_compatible_but_bad_known_events_fail() {
        assert!(matches!(
            Event::parse(r#"{"FutureEvent":{"secret":"ignored"}}"#).unwrap(),
            Event::Ignored
        ));
        assert!(Event::parse(r#"{"WorkspaceActivated":{"id":"wrong"}}"#).is_err());
    }
    #[tokio::test(flavor = "current_thread")]
    async fn socket_handshake_action_and_disconnect() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("niri.sock");
        let listener = tokio::net::UnixListener::bind(&path).unwrap();
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut reader = BufReader::new(socket);
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&frame(&mut reader).await.unwrap())
                    .unwrap(),
                serde_json::json!("EventStream")
            );
            reader
                .get_mut()
                .write_all(b"{\"Ok\":\"Handled\"}\n{\"FutureEvent\":{}}\n{\"WorkspacesChanged\":{\"workspaces\":[]}}\n")
                .await
                .unwrap();
            drop(reader);
            let (socket, _) = listener.accept().await.unwrap();
            let mut reader = BufReader::new(socket);
            let request: serde_json::Value =
                serde_json::from_str(&frame(&mut reader).await.unwrap()).unwrap();
            assert_eq!(request["Action"]["FocusWorkspace"]["reference"]["Id"], 42);
            reader
                .get_mut()
                .write_all(b"{\"Ok\":\"Handled\"}\n")
                .await
                .unwrap();
        });
        let mut events = Events::connect(&path).await.unwrap();
        assert!(matches!(events.next().await.unwrap(), Event::Ignored));
        assert!(matches!(
            events.next().await.unwrap(),
            Event::WorkspacesChanged { .. }
        ));
        assert!(events.next().await.is_err());
        focus(&path, 42).await.unwrap();
        server.await.unwrap();
    }
    #[tokio::test(flavor = "current_thread")]
    async fn rejected_action_and_oversize_frame_are_errors() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("niri.sock");
        let listener = tokio::net::UnixListener::bind(&path).unwrap();
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut reader = BufReader::new(socket);
            frame(&mut reader).await.unwrap();
            reader
                .get_mut()
                .write_all(b"{\"Err\":\"missing workspace\"}\n")
                .await
                .unwrap();
        });
        assert!(focus(&path, 42).await.is_err());
        server.await.unwrap();
        let (a, mut b) = UnixStream::pair().unwrap();
        let writer = tokio::spawn(async move {
            let _ = b.write_all(&vec![b'x'; MAX_FRAME as usize + 1]).await;
        });
        assert!(frame(&mut BufReader::new(a)).await.is_err());
        writer.await.unwrap();
    }
}
