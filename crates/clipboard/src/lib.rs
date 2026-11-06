//! In-memory clipboard policy. Payloads never enter logs, files or Debug output.
mod fd;
mod transport;
pub use fd::{copy_png, receive_request};
use serde::{Deserialize, Serialize};
use std::{collections::VecDeque, sync::Arc};
pub use transport::Service;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    Text,
    Png,
}
#[derive(Clone)]
struct Item {
    id: u64,
    kind: Kind,
    data: Arc<Vec<u8>>,
    preview: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Entry {
    pub id: u64,
    pub kind: Kind,
    pub preview: String,
    pub bytes: usize,
}
impl std::fmt::Debug for Entry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Entry")
            .field("id", &self.id)
            .field("kind", &self.kind)
            .field("bytes", &self.bytes)
            .finish_non_exhaustive()
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Snapshot {
    pub state: String,
    pub entries: Vec<Entry>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "clipboard", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Request {
    List { query: String },
    Select { id: u64 },
    Delete { id: u64 },
    Clear,
    ImportPng,
}
impl std::fmt::Debug for Request {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ClipboardRequest")
    }
}
struct History {
    config: cabby_config::Clipboard,
    items: VecDeque<Item>,
    next: u64,
}
impl History {
    fn new(config: cabby_config::Clipboard) -> Self {
        Self {
            config,
            items: VecDeque::new(),
            next: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as u64,
        }
    }
    fn trim(&mut self) {
        self.items
            .retain(|i| i.data.len() <= self.config.max_item_bytes as usize);
        while self.items.len() > self.config.max_items as usize
            || self.items.iter().map(|i| i.data.len()).sum::<usize>()
                > self.config.memory_bytes as usize
        {
            self.items.pop_back();
        }
    }
    fn add(&mut self, kind: Kind, data: Vec<u8>) -> Option<Item> {
        if !self.config.enabled
            || data.is_empty()
            || data.len() > self.config.max_item_bytes as usize
        {
            return None;
        }
        let preview = match kind {
            Kind::Text => {
                let s = std::str::from_utf8(&data).ok()?;
                if s.contains('\0') {
                    return None;
                }
                s.chars()
                    .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
                    .take(160)
                    .collect()
            }
            Kind::Png => {
                use image::ImageDecoder;
                let mut decoder =
                    image::codecs::png::PngDecoder::new(std::io::Cursor::new(&data)).ok()?;
                let mut limits = image::Limits::default();
                limits.max_alloc = Some(67_108_864);
                decoder.set_limits(limits).ok()?;
                if decoder.total_bytes() > 67_108_864 {
                    return None;
                }
                let (w, h) = decoder.dimensions();
                if w == 0
                    || h == 0
                    || w > 8192
                    || h > 8192
                    || u64::from(w) * u64::from(h) > 16_777_216
                {
                    return None;
                }
                image::DynamicImage::from_decoder(decoder).ok()?;
                format!("PNG image · {w} × {h}")
            }
        };
        if let Some(index) = self
            .items
            .iter()
            .position(|i| i.kind == kind && *i.data == data)
        {
            let item = self.items.remove(index).unwrap();
            self.items.push_front(item.clone());
            return Some(item);
        }
        let item = Item {
            id: self.next,
            kind,
            data: Arc::new(data),
            preview,
        };
        self.next = self.next.checked_add(1)?;
        self.items.push_front(item.clone());
        self.trim();
        Some(item)
    }
    fn entries(&self, query: &str) -> Vec<Entry> {
        let query = query.to_lowercase();
        self.items
            .iter()
            .filter(|i| {
                query.is_empty()
                    || (i.kind == Kind::Text
                        && String::from_utf8_lossy(&i.data)
                            .to_lowercase()
                            .contains(&query))
                    || i.preview.to_lowercase().contains(&query)
            })
            .take(50)
            .map(|i| Entry {
                id: i.id,
                kind: i.kind,
                preview: i.preview.clone(),
                bytes: i.data.len(),
            })
            .collect()
    }
}

/// Private coordinator IPC; responses are bounded and never logged by this client.
pub fn client(root: &std::path::Path, request: &Request) -> Result<serde_json::Value, String> {
    use std::io::{BufRead, Read, Write};
    let mut stream = std::os::unix::net::UnixStream::connect(root.join("control.sock"))
        .map_err(|_| "clipboard coordinator unavailable")?;
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(4)))
        .map_err(|_| "clipboard request unavailable")?;
    stream
        .set_write_timeout(Some(std::time::Duration::from_secs(2)))
        .map_err(|_| "clipboard request unavailable")?;
    writeln!(
        stream,
        "{}",
        serde_json::to_string(request).map_err(|_| "invalid clipboard request")?
    )
    .map_err(|_| "clipboard request failed")?;
    let mut response = String::new();
    std::io::BufReader::new(stream.take(65537))
        .read_line(&mut response)
        .map_err(|_| "clipboard request failed")?;
    if response.len() > 65536 {
        return Err("clipboard response oversized".into());
    }
    let value: serde_json::Value =
        serde_json::from_str(&response).map_err(|_| "invalid clipboard response")?;
    if value["ok"] != true {
        return Err("clipboard operation failed".into());
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_deduplicated_history_reselection_and_private_previews() {
        let mut h = History::new(cabby_config::Clipboard {
            max_items: 2,
            max_item_bytes: 10,
            memory_bytes: 12,
            ..Default::default()
        });
        let first = h.add(Kind::Text, b"first".to_vec()).unwrap();
        h.add(Kind::Text, b"second".to_vec());
        assert_eq!(h.add(Kind::Text, b"first".to_vec()).unwrap().id, first.id);
        assert_eq!(h.entries("FIR").len(), 1);
        h.add(Kind::Text, b"third".to_vec());
        assert_eq!(h.items.len(), 2);
        assert!(!h.entries("").iter().any(|i| i.preview == "second"));
        assert!(h.add(Kind::Text, vec![b'a'; 11]).is_none());
        assert!(h.add(Kind::Text, vec![255]).is_none());
        assert!(h.add(Kind::Text, b"bad\0".to_vec()).is_none());
        assert!(h.add(Kind::Png, b"invalid".to_vec()).is_none());
        h.config.memory_bytes = 5;
        h.trim();
        assert_eq!(h.items.len(), 1);
        h.config.enabled = false;
        assert!(h.add(Kind::Text, b"new".to_vec()).is_none());
        assert!(!format!("{:?}", h.entries("")).contains("third"));
    }
}
