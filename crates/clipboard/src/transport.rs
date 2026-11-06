use super::*;
use std::{
    collections::HashMap,
    fs::File,
    io::{Read, Write},
    os::{
        fd::{AsFd, AsRawFd, FromRawFd, OwnedFd},
        unix::net::UnixStream,
    },
    sync::{Mutex, mpsc},
    time::{Duration, Instant},
};
use wayland_client::{
    Connection, Dispatch, Proxy, QueueHandle,
    protocol::{wl_registry, wl_seat},
};
use wayland_protocols::ext::data_control::v1::client::{
    ext_data_control_device_v1::{self, ExtDataControlDeviceV1 as Device},
    ext_data_control_manager_v1::ExtDataControlManagerV1 as Manager,
    ext_data_control_offer_v1::{self, ExtDataControlOfferV1 as Offer},
    ext_data_control_source_v1::{self, ExtDataControlSourceV1 as Source},
};
const TEXT_MIMES: [&str; 4] = [
    "text/plain;charset=utf-8",
    "text/plain;charset=UTF-8",
    "text/plain",
    "UTF8_STRING",
];
struct Shared {
    history: History,
    state: &'static str,
    stop: bool,
    commands: VecDeque<(Request, mpsc::SyncSender<bool>, Instant)>,
    path: std::path::PathBuf,
    revision: u64,
}
pub struct Service {
    shared: Arc<Mutex<Shared>>,
    wake: UnixStream,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Shared {
    fn notify(&mut self) {
        use std::os::unix::fs::OpenOptionsExt;
        self.revision = self.revision.wrapping_add(1);
        let value = serde_json::json!({"revision":self.revision,"state":self.state,"items":self.history.items.len()});
        let tmp = self.path.with_extension("tmp");
        let result = (|| -> std::io::Result<()> {
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW)
                .open(&tmp)?;
            write!(f, "{value}")?;
            std::fs::rename(&tmp, &self.path)
        })();
        if result.is_err() {
            eprintln!("Clipboard status unavailable");
        }
    }
}
impl Service {
    pub fn new(config: &cabby_config::Clipboard, root: &std::path::Path) -> Self {
        let (wake, reader) = UnixStream::pair().expect("clipboard wake");
        wake.set_nonblocking(true).unwrap();
        reader.set_nonblocking(true).unwrap();
        let shared = Arc::new(Mutex::new(Shared {
            history: History::new(config.clone()),
            state: "starting",
            stop: false,
            commands: VecDeque::new(),
            path: root.join("clipboard.json"),
            revision: 0,
        }));
        let h = shared.clone();
        let thread = std::thread::spawn(move || run(h, reader));
        Self {
            shared,
            wake,
            thread: Some(thread),
        }
    }
    pub fn configure(&self, config: &cabby_config::Clipboard) {
        let mut s = self.shared.lock().unwrap();
        if s.history.config == *config {
            return;
        }
        s.history.config = config.clone();
        if !config.enabled {
            s.history.items.clear();
        }
        s.history.trim();
        s.notify();
        drop(s);
        let _ = (&self.wake).write(&[1]);
    }
    pub fn status(&self) -> serde_json::Value {
        let s = self.shared.lock().unwrap();
        serde_json::json!({"state":s.state,"items":s.history.items.len(),"bytes":s.history.items.iter().map(|i|i.data.len()).sum::<usize>()})
    }
    pub fn import_png(&self, fd: OwnedFd) -> serde_json::Value {
        use std::os::unix::fs::FileExt;
        let file = File::from(fd);
        let needed = libc::F_SEAL_WRITE | libc::F_SEAL_GROW | libc::F_SEAL_SHRINK;
        let seals = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GET_SEALS) };
        let mut h = self.shared.lock().unwrap();
        if !h.history.config.enabled {
            return serde_json::json!({"ok":false,"error":"Clipboard history is disabled"});
        }
        if h.state != "watching" {
            return serde_json::json!({"ok":false,"error":"Clipboard backend unavailable"});
        }
        if seals < 0 || seals & needed != needed {
            return serde_json::json!({"ok":false,"error":"Invalid clipboard image descriptor"});
        }
        let Ok(metadata) = file.metadata() else {
            return serde_json::json!({"ok":false,"error":"Clipboard image unavailable"});
        };
        if !metadata.is_file() || metadata.len() == 0 {
            return serde_json::json!({"ok":false,"error":"Invalid clipboard image"});
        }
        if metadata.len() > u64::from(h.history.config.max_item_bytes) {
            return serde_json::json!({"ok":false,"error":"Image exceeds clipboard.max_item_bytes"});
        }
        let mut data = vec![0; metadata.len() as usize];
        if file.read_exact_at(&mut data, 0).is_err() {
            return serde_json::json!({"ok":false});
        }
        let Some(item) = h.history.add(Kind::Png, data) else {
            return serde_json::json!({"ok":false});
        };
        h.notify();
        drop(h);
        self.request(Request::Select { id: item.id })
    }
    pub fn request(&self, request: Request) -> serde_json::Value {
        let (tx, rx) = mpsc::sync_channel(1);
        {
            let mut s = self.shared.lock().unwrap();
            if let Request::List { query } = &request {
                if query.len() > 256 {
                    return serde_json::json!({"ok":false,"error":"query too long"});
                }
                return serde_json::json!({"ok":true,"clipboard":Snapshot{state:s.state.into(),entries:s.history.entries(query)}});
            }
            if s.commands.len() >= 8 || !s.history.config.enabled {
                return serde_json::json!({"ok":false,"error":"clipboard unavailable"});
            }
            s.commands
                .push_back((request, tx, Instant::now() + Duration::from_millis(1500)));
        }
        let _ = (&self.wake).write(&[1]);
        serde_json::json!({"ok":rx.recv_timeout(Duration::from_secs(2)).unwrap_or(false)})
    }
}
impl Drop for Service {
    fn drop(&mut self) {
        self.shared.lock().unwrap().stop = true;
        let _ = (&self.wake).write(&[1]);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
struct Receive {
    file: File,
    data: Vec<u8>,
    kind: Kind,
    due: Instant,
}
struct Send {
    file: File,
    data: Arc<Vec<u8>>,
    offset: usize,
    due: Instant,
}
struct Data {
    shared: Arc<Mutex<Shared>>,
    manager: Option<(u32, Manager)>,
    seat: Option<(u32, wl_seat::WlSeat)>,
    device: Option<Device>,
    offers: HashMap<wayland_client::backend::ObjectId, (Offer, Vec<String>)>,
    selection: Option<Offer>,
    receiving: Option<Receive>,
    sending: Vec<Send>,
    source: Option<(Source, Item)>,
    marker: String,
    failed: bool,
}
fn nonblock(fd: i32) {
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL);
        libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
    }
}
impl Data {
    fn offer(&mut self, item: Item, q: &QueueHandle<Self>) -> bool {
        let (Some((_, m)), Some(d)) = (&self.manager, &self.device) else {
            return false;
        };
        let source = m.create_data_source(q, item.clone());
        match item.kind {
            Kind::Text => {
                for mime in TEXT_MIMES {
                    source.offer(mime.into());
                }
            }
            Kind::Png => source.offer("image/png".into()),
        }
        source.offer(self.marker.clone());
        d.set_selection(Some(&source));
        if let Some((old, _)) = self.source.replace((source, item)) {
            old.destroy();
        }
        true
    }
    fn setup(&mut self, q: &QueueHandle<Self>) {
        if self.device.is_none()
            && let (Some((_, m)), Some((_, seat))) = (&self.manager, &self.seat)
        {
            self.device = Some(m.get_data_device(seat, q, ()));
        }
        let mut h = self.shared.lock().unwrap();
        let state = if self.device.is_some() {
            "watching"
        } else {
            "unsupported"
        };
        if h.state != state {
            h.state = state;
            h.notify();
        }
    }
    fn commands(&mut self, q: &QueueHandle<Self>) {
        loop {
            let command = self.shared.lock().unwrap().commands.pop_front();
            let Some((cmd, reply, deadline)) = command else {
                break;
            };
            if Instant::now() >= deadline {
                let _ = reply.send(false);
                continue;
            }
            let ok = match cmd {
                Request::Select { id } => {
                    let item = self
                        .shared
                        .lock()
                        .unwrap()
                        .history
                        .items
                        .iter()
                        .find(|i| i.id == id)
                        .cloned();
                    let accepted = item.is_some_and(|item| self.offer(item, q));
                    if accepted {
                        let mut h = self.shared.lock().unwrap();
                        if let Some(index) = h.history.items.iter().position(|i| i.id == id) {
                            let item = h.history.items.remove(index).unwrap();
                            h.history.items.push_front(item);
                        }
                    }
                    accepted
                }
                Request::Delete { id } => {
                    let removed = {
                        let mut s = self.shared.lock().unwrap();
                        let before = s.history.items.len();
                        s.history.items.retain(|i| i.id != id);
                        before != s.history.items.len()
                    };
                    if self.source.as_ref().is_some_and(|(_, item)| item.id == id) {
                        self.clear_source();
                    }
                    removed
                }
                Request::Clear => {
                    self.shared.lock().unwrap().history.items.clear();
                    self.receiving = None;
                    self.clear_source();
                    true
                }
                Request::List { .. } | Request::ImportPng => false,
            };
            self.shared.lock().unwrap().notify();
            let _ = reply.send(ok);
        }
    }
    fn clear_source(&mut self) {
        if let Some((source, _)) = self.source.take() {
            // Destroy only our source. An unconditional set_selection(None) could
            // clear a newer external selection whose event is still queued.
            source.destroy();
        }
        self.sending.clear();
    }
    fn transfers(&mut self, q: &QueueHandle<Self>) {
        let max = self.shared.lock().unwrap().history.config.max_item_bytes as usize;
        if let Some(mut transfer) = self.receiving.take() {
            let mut done = false;
            let mut failed = Instant::now() >= transfer.due || transfer.data.len() > max;
            let mut buf = [0; 16384];
            // Bound work per dispatch so a producer cannot starve control/config.
            for _ in 0..64 {
                match transfer.file.read(&mut buf) {
                    Ok(0) => {
                        done = true;
                        break;
                    }
                    Ok(n) => {
                        transfer.data.extend_from_slice(&buf[..n]);
                        if transfer.data.len() > max {
                            failed = true;
                            break;
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => {
                        failed = true;
                        break;
                    }
                }
            }
            if done && !failed {
                let item = {
                    let mut h = self.shared.lock().unwrap();
                    let item = h.history.add(transfer.kind, transfer.data);
                    h.notify();
                    item
                };
                if let Some(item) = item {
                    self.offer(item, q);
                }
            } else if !failed {
                self.receiving = Some(transfer);
            }
        }
        self.sending.retain_mut(|t| {
            if Instant::now() >= t.due {
                return false;
            }
            for _ in 0..64 {
                match t.file.write(&t.data[t.offset..]) {
                    Ok(0) => return false,
                    Ok(n) => {
                        t.offset += n;
                        if t.offset == t.data.len() {
                            return false;
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => return true,
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => return false,
                }
            }
            true
        });
    }
}
impl Dispatch<wl_registry::WlRegistry, ()> for Data {
    fn event(
        s: &mut Self,
        r: &wl_registry::WlRegistry,
        e: wl_registry::Event,
        _: &(),
        _: &Connection,
        q: &QueueHandle<Self>,
    ) {
        match e {
            wl_registry::Event::Global {
                name,
                interface,
                version,
            } => match interface.as_str() {
                "ext_data_control_manager_v1" if s.manager.is_none() => {
                    s.manager = Some((name, r.bind(name, 1, q, ())))
                }
                "wl_seat" if s.seat.is_none() => {
                    s.seat = Some((name, r.bind(name, version.min(7), q, ())))
                }
                _ => (),
            },
            wl_registry::Event::GlobalRemove { name }
                if s.manager.as_ref().is_some_and(|(n, _)| *n == name)
                    || s.seat.as_ref().is_some_and(|(n, _)| *n == name) =>
            {
                s.failed = true;
            }
            _ => (),
        }
        s.setup(q);
    }
}
wayland_client::delegate_noop!(Data: ignore wl_seat::WlSeat);
wayland_client::delegate_noop!(Data: ignore Manager);
impl Dispatch<Device, ()> for Data {
    fn event(
        s: &mut Self,
        _: &Device,
        e: ext_data_control_device_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match e {
            ext_data_control_device_v1::Event::DataOffer { id } => {
                if s.offers.len() >= 16 {
                    s.failed = true;
                } else {
                    s.offers.insert(id.id(), (id, Vec::new()));
                }
            }
            ext_data_control_device_v1::Event::Selection { id } => {
                if id.is_some() {
                    s.receiving = None;
                }
                if let Some(old) = s.selection.take() {
                    s.offers.remove(&old.id());
                    old.destroy();
                }
                let Some(offer) = id else {
                    return;
                };
                let Some((_, mimes)) = s.offers.get(&offer.id()) else {
                    return;
                };
                s.selection = Some(offer.clone());
                let choice = choose(mimes, &s.marker);
                let Some((mime, kind)) = choice else {
                    return;
                };
                let mut fds = [-1; 2];
                if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
                    return;
                }
                let reader = unsafe { File::from_raw_fd(fds[0]) };
                let writer = unsafe { OwnedFd::from_raw_fd(fds[1]) };
                nonblock(reader.as_raw_fd());
                offer.receive(mime.into(), writer.as_fd());
                drop(writer);
                s.receiving = Some(Receive {
                    file: reader,
                    data: Vec::new(),
                    kind,
                    due: Instant::now() + Duration::from_secs(3),
                });
            }
            ext_data_control_device_v1::Event::PrimarySelection { id: Some(offer) } => {
                s.offers.remove(&offer.id());
                offer.destroy();
            }
            ext_data_control_device_v1::Event::Finished => s.failed = true,
            _ => (),
        }
    }
    wayland_client::event_created_child!(Data,Device,[0=>(Offer,())]);
}
impl Dispatch<Offer, ()> for Data {
    fn event(
        s: &mut Self,
        o: &Offer,
        e: ext_data_control_offer_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let ext_data_control_offer_v1::Event::Offer { mime_type } = e
            && let Some((_, m)) = s.offers.get_mut(&o.id())
        {
            if m.len() >= 64 || mime_type.len() > 256 {
                s.failed = true;
            } else {
                m.push(mime_type);
            }
        }
    }
}
impl Dispatch<Source, Item> for Data {
    fn event(
        s: &mut Self,
        source: &Source,
        e: ext_data_control_source_v1::Event,
        item: &Item,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match e {
            ext_data_control_source_v1::Event::Send { mime_type, fd } => {
                let allowed = match item.kind {
                    Kind::Text => TEXT_MIMES.contains(&mime_type.as_str()),
                    Kind::Png => mime_type == "image/png",
                };
                if allowed && s.sending.len() < 8 {
                    nonblock(fd.as_raw_fd());
                    s.sending.push(Send {
                        file: File::from(fd),
                        data: item.data.clone(),
                        offset: 0,
                        due: Instant::now() + Duration::from_secs(3),
                    });
                }
            }
            ext_data_control_source_v1::Event::Cancelled => {
                if s.source.as_ref().is_some_and(|(src, _)| src == source) {
                    s.source = None;
                }
                source.destroy();
            }
            _ => (),
        }
    }
}
fn run(shared: Arc<Mutex<Shared>>, wake: UnixStream) {
    loop {
        {
            let mut s = shared.lock().unwrap();
            if s.stop {
                return;
            }
            if !s.history.config.enabled {
                if s.state != "disabled" {
                    s.state = "disabled";
                    s.notify();
                }
                for (_, reply, _) in s.commands.drain(..) {
                    let _ = reply.send(false);
                }
                drop(s);
                let _ = wait(&wake, -1, &[], None);
                continue;
            }
        }
        if connected(shared.clone(), &wake).is_err() {
            let mut s = shared.lock().unwrap();
            s.state = "unavailable";
            s.notify();
            for (_, reply, _) in s.commands.drain(..) {
                let _ = reply.send(false);
            }
            drop(s);
            let _ = wait(
                &wake,
                -1,
                &[],
                Some(Instant::now() + Duration::from_secs(2)),
            );
        }
    }
}
fn wait(
    wake: &UnixStream,
    wayland: i32,
    extra: &[(i32, i16)],
    due: Option<Instant>,
) -> std::io::Result<bool> {
    let mut fds = vec![
        libc::pollfd {
            fd: wake.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        },
        libc::pollfd {
            fd: wayland,
            events: libc::POLLIN,
            revents: 0,
        },
    ];
    fds.extend(extra.iter().map(|(fd, e)| libc::pollfd {
        fd: *fd,
        events: *e,
        revents: 0,
    }));
    let timeout = due.map_or(-1, |d| {
        d.saturating_duration_since(Instant::now())
            .as_millis()
            .min(i32::MAX as u128) as i32
    });
    if unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as _, timeout) } < 0 {
        return Err(std::io::Error::last_os_error());
    }
    if fds[0].revents != 0 {
        let _ = (&*wake).read(&mut [0; 128]);
    }
    Ok(fds[1].revents != 0)
}
fn connected(
    shared: Arc<Mutex<Shared>>,
    wake: &UnixStream,
) -> Result<(), Box<dyn std::error::Error>> {
    let conn = Connection::connect_to_env()?;
    let mut queue = conn.new_event_queue();
    let q = queue.handle();
    conn.display().get_registry(&q, ());
    let mut s = Data {
        shared: shared.clone(),
        manager: None,
        seat: None,
        device: None,
        offers: HashMap::new(),
        selection: None,
        receiving: None,
        sending: Vec::new(),
        source: None,
        marker: format!("application/x-cabby-clipboard-{}", std::process::id()),
        failed: false,
    };
    loop {
        {
            let h = shared.lock().unwrap();
            if h.stop || !h.history.config.enabled {
                s.clear_source();
                conn.flush()?;
                return Ok(());
            }
        }
        queue.dispatch_pending(&mut s)?;
        if s.failed {
            return Err("clipboard connection lost".into());
        }
        s.commands(&q);
        s.transfers(&q);
        conn.flush()?;
        let extra: Vec<_> = s
            .receiving
            .iter()
            .map(|t| (t.file.as_raw_fd(), libc::POLLIN))
            .chain(
                s.sending
                    .iter()
                    .map(|t| (t.file.as_raw_fd(), libc::POLLOUT)),
            )
            .collect();
        let due = s
            .receiving
            .iter()
            .map(|t| t.due)
            .chain(s.sending.iter().map(|t| t.due))
            .min();
        let Some(reader) = queue.prepare_read() else {
            continue;
        };
        if wait(wake, conn.as_fd().as_raw_fd(), &extra, due)? {
            match reader.read() {
                Ok(_) => (),
                Err(wayland_client::backend::WaylandError::Io(e))
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                    ) => {}
                Err(e) => return Err(e.into()),
            }
        }
    }
}

fn choose(mimes: &[String], marker: &str) -> Option<(&'static str, Kind)> {
    if mimes
        .iter()
        .any(|m| m == marker || m == "x-kde-passwordManagerHint")
    {
        return None;
    }
    if mimes.iter().any(|m| m == "image/png") {
        return Some(("image/png", Kind::Png));
    }
    TEXT_MIMES
        .into_iter()
        .find(|m| mimes.iter().any(|v| v == m))
        .map(|m| (m, Kind::Text))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mime_boundary_rejects_sensitive_own_and_unsupported_offers() {
        let strings = |s: &[&str]| s.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            choose(&strings(&["text/html", "text/plain"]), "own"),
            Some(("text/plain", Kind::Text))
        );
        assert_eq!(
            choose(&strings(&["image/png", "text/plain"]), "own"),
            Some(("image/png", Kind::Png))
        );
        for types in [
            &["x-kde-passwordManagerHint", "text/plain"][..],
            &["text/plain", "own"],
            &["text/html"],
        ] {
            assert!(choose(&strings(types), "own").is_none());
        }
    }
}
