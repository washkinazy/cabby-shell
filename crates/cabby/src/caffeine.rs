//! One policy owner for independent manual, audio and fullscreen reasons.
use futures_util::StreamExt;
use std::{
    collections::BTreeSet,
    io::Write,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::watch;
#[derive(Default)]
struct State {
    config: cabby_config::Caffeine,
    manual: bool,
    audio: bool,
    unknown_fullscreen: bool,
    outputs: BTreeSet<String>,
    audio_state: &'static str,
    fullscreen_state: &'static str,
    sleep_state: &'static str,
    stop: bool,
    last: String,
}
#[derive(Debug, PartialEq)]
struct Policy {
    reasons: Vec<&'static str>,
    idle: bool,
    sleep: bool,
    night_outputs: BTreeSet<String>,
    night_all: bool,
}
impl State {
    fn policy(&self) -> Policy {
        let audio = self.config.audio && self.audio;
        let full = self.config.fullscreen && (!self.outputs.is_empty() || self.unknown_fullscreen);
        let active = self.manual || audio || full;
        Policy {
            reasons: [
                ("manual", self.manual),
                ("audio", audio),
                ("fullscreen", full),
            ]
            .into_iter()
            .filter_map(|(name, on)| on.then_some(name))
            .collect(),
            idle: active && self.config.inhibit_idle,
            sleep: active && self.config.inhibit_sleep,
            night_outputs: if full && self.config.fullscreen_pause_nightlight {
                self.outputs.clone()
            } else {
                BTreeSet::new()
            },
            night_all: self.manual && self.config.manual_pause_nightlight
                || full && self.config.fullscreen_pause_nightlight && self.unknown_fullscreen,
        }
    }
}
struct Shared {
    state: Mutex<State>,
    idle: crate::idle::Inhibit,
    night: crate::night::Inhibit,
    block: watch::Sender<Option<bool>>,
    path: PathBuf,
}
#[derive(Clone)]
pub(super) struct Handle(Arc<Shared>);
impl Handle {
    fn update(&self, change: impl FnOnce(&mut State)) {
        use std::os::unix::fs::OpenOptionsExt;
        let mut s = self.0.state.lock().unwrap();
        change(&mut s);
        if s.stop {
            s.manual = false;
            s.audio = false;
            s.outputs.clear();
            s.unknown_fullscreen = false;
        }
        let policy = s.policy();
        self.0.idle.active(policy.idle);
        self.0
            .night
            .apply(policy.night_outputs.clone(), policy.night_all);
        let desired = (!s.stop).then_some(policy.sleep);
        self.0.block.send_if_modified(|v| {
            if *v == desired {
                false
            } else {
                *v = desired;
                true
            }
        });
        let snapshot = serde_json::json!({"active":!policy.reasons.is_empty(),"manual":s.manual,"reasons":policy.reasons,"fullscreen_outputs":s.outputs,"audio_state":s.audio_state,"fullscreen_state":s.fullscreen_state,"sleep_state":s.sleep_state,"sleep_inhibited":policy.sleep&&s.sleep_state=="blocked","idle_inhibited":policy.idle,"nightlight_paused":policy.night_all||!policy.night_outputs.is_empty()});
        let value = snapshot.to_string();
        if s.last == value {
            return;
        }
        let tmp = self.0.path.with_extension("tmp");
        let result = (|| -> std::io::Result<()> {
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW)
                .open(&tmp)?;
            file.write_all(value.as_bytes())?;
            std::fs::rename(tmp, &self.0.path)
        })();
        if result.is_err() {
            eprintln!("Caffeine status snapshot unavailable");
        }
        s.last = value;
    }
    pub fn fullscreen(&self, outputs: BTreeSet<String>, unknown: bool, status: &'static str) {
        self.update(|s| {
            s.outputs = outputs;
            s.unknown_fullscreen = unknown;
            s.fullscreen_state = status;
        });
    }
    pub fn config(&self) -> Option<cabby_config::Caffeine> {
        let s = self.0.state.lock().unwrap();
        (!s.stop).then(|| s.config.clone())
    }
}
pub struct Caffeine {
    handle: Handle,
    config: watch::Sender<Option<cabby_config::Caffeine>>,
    wake: std::os::unix::net::UnixStream,
    threads: Vec<std::thread::JoinHandle<()>>,
}
impl Caffeine {
    pub fn new(
        config: &cabby_config::Caffeine,
        root: &std::path::Path,
        idle: crate::idle::Inhibit,
        night: crate::night::Inhibit,
    ) -> Self {
        let (block, rx) = watch::channel(Some(false));
        let handle = Handle(Arc::new(Shared {
            state: Mutex::new(State {
                config: config.clone(),
                ..State::default()
            }),
            idle,
            night,
            block,
            path: root.join("caffeine.json"),
        }));
        handle.update(|_| ());
        let (config, updates) = watch::channel(Some(config.clone()));
        let h = handle.clone();
        let background = std::thread::spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("Caffeine runtime")
                .block_on(async {
                    tokio::join!(audio(updates, h.clone()), sleep(rx, h));
                });
        });
        let (wake, reader) = std::os::unix::net::UnixStream::pair().expect("Caffeine wake");
        wake.set_nonblocking(true).unwrap();
        reader.set_nonblocking(true).unwrap();
        let h = handle.clone();
        let fullscreen = std::thread::spawn(move || crate::fullscreen::run(h, reader));
        Self {
            handle,
            config,
            wake,
            threads: vec![background, fullscreen],
        }
    }
    pub fn configure(&self, config: &cabby_config::Caffeine) {
        if self.handle.config().as_ref() == Some(config) {
            return;
        }
        self.handle.update(|s| {
            s.config = config.clone();
            if !config.audio {
                s.audio = false;
            }
            if !config.fullscreen {
                s.outputs.clear();
                s.unknown_fullscreen = false;
            }
        });
        let _ = self.config.send(Some(config.clone()));
        let _ = (&self.wake).write(&[1]);
    }
    pub fn control(&self, command: &str) {
        self.handle.update(|s| match command {
            "caffeine-on" | "idle-inhibit-on" => s.manual = true,
            "caffeine-off" | "idle-inhibit-off" => s.manual = false,
            "caffeine-toggle" | "idle-inhibit-toggle" => s.manual = !s.manual,
            _ => (),
        });
    }
    pub fn status(&self) -> serde_json::Value {
        serde_json::from_str(&self.handle.0.state.lock().unwrap().last).unwrap_or_default()
    }
}
impl Drop for Caffeine {
    fn drop(&mut self) {
        self.handle.update(|s| {
            s.stop = true;
            s.manual = false;
            s.audio = false;
            s.outputs.clear();
            s.unknown_fullscreen = false;
        });
        let _ = self.config.send(None);
        let _ = self.handle.0.block.send(None);
        let _ = (&self.wake).write(&[1]);
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}
async fn observe_audio(
    client: &cabby_audio::Client,
    config: &mut watch::Receiver<Option<cabby_config::Caffeine>>,
    mut update: impl FnMut(bool),
) -> Result<(), cabby_audio::Error> {
    let mut events = client.subscribe()?;
    let observe = async {
        update(client.snapshot().await?.active);
        loop {
            events.next().await?;
            tokio::time::sleep(Duration::from_millis(100)).await;
            update(client.snapshot().await?.active);
        }
    };
    let disabled = async {
        loop {
            if !config.borrow_and_update().as_ref().is_some_and(|c| c.audio)
                || config.changed().await.is_err()
            {
                return;
            }
        }
    };
    let result = tokio::select! { result = observe => result, _ = disabled => Ok(()) };
    events.close().await;
    result
}
async fn audio(mut config: watch::Receiver<Option<cabby_config::Caffeine>>, h: Handle) {
    let client = cabby_audio::Client::default();
    loop {
        let Some(c) = config.borrow_and_update().clone() else {
            return;
        };
        if !c.audio {
            h.update(|s| {
                s.audio = false;
                s.audio_state = "disabled";
            });
            if config.changed().await.is_err() {
                return;
            }
            continue;
        }
        let _ = observe_audio(&client, &mut config, |active| {
            h.update(|s| {
                s.audio = active;
                s.audio_state = "watching";
            })
        })
        .await;
        if !config.borrow().as_ref().is_some_and(|c| c.audio) {
            continue;
        }
        h.update(|s| {
            s.audio = false;
            s.audio_state = "unavailable";
        });
        tokio::select! {_ = tokio::time::sleep(Duration::from_secs(2))=>(),_ = config.changed()=>()}
    }
}
async fn sleep(mut active: watch::Receiver<Option<bool>>, h: Handle) {
    loop {
        let Some(on) = *active.borrow_and_update() else {
            return;
        };
        if !on {
            h.update(|s| s.sleep_state = "inactive");
            if active.changed().await.is_err() {
                return;
            }
            continue;
        }
        // Acquisition and lease lifetime are separate so status never claims a lease
        // before logind actually grants it.
        tokio::select! {_ = block_session(h.clone())=>h.update(|s|s.sleep_state="unavailable"),_ = active.changed()=>continue}
        tokio::select! {_ = tokio::time::sleep(Duration::from_secs(2))=>(),_ = active.changed()=>()}
    }
}
async fn block_session(h: Handle) {
    let _ = hold_block_with_status(h).await;
}
async fn hold_block_with_status(h: Handle) -> zbus::Result<()> {
    // The helper below sets the status only after acquiring the real FD.
    hold_block_inner(None, || h.update(|s| s.sleep_state = "blocked")).await
}
async fn hold_block_inner(address: Option<&str>, granted: impl FnOnce()) -> zbus::Result<()> {
    use std::os::fd::AsRawFd;
    let builder = match address {
        Some(address) => zbus::connection::Builder::address(address)?,
        None => zbus::connection::Builder::system()?,
    };
    let connection = builder
        .method_timeout(Duration::from_secs(3))
        .build()
        .await?;
    let dbus = zbus::fdo::DBusProxy::new(&connection).await?;
    let mut owners = dbus.receive_name_owner_changed().await?;
    let owner = dbus
        .get_name_owner("org.freedesktop.login1".try_into().unwrap())
        .await?;
    let proxy = zbus::Proxy::new(
        &connection,
        owner.as_str(),
        "/org/freedesktop/login1",
        "org.freedesktop.login1.Manager",
    )
    .await?;
    let fd: zbus::zvariant::OwnedFd = proxy
        .call(
            "Inhibit",
            &(
                "sleep",
                "Cabby Caffeine",
                "Caffeine inhibition is active",
                "block",
            ),
        )
        .await?;
    if unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
        return Err(zbus::Error::Failure("inhibitor FD unavailable".into()));
    }
    granted();
    loop {
        tokio::select! {_ = connection.closed()=>return Ok(()),signal=owners.next()=>{let Some(signal)=signal else{return Ok(());};let args=signal.args()?;if args.name().as_str()=="org.freedesktop.login1"{return Ok(());}}}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reasons_and_effects_compose_without_rewriting_preferences() {
        let mut s = State::default();
        s.config.audio = true;
        s.config.fullscreen = true;
        s.audio = true;
        s.manual = true;
        s.outputs.insert("TEST-1".into());
        assert_eq!(s.policy().reasons, ["manual", "audio", "fullscreen"]);
        assert!(s.policy().idle && s.policy().sleep);
        assert_eq!(s.policy().night_outputs, s.outputs);
        assert!(!s.policy().night_all);
        s.manual = false;
        assert_eq!(s.policy().reasons, ["audio", "fullscreen"]);
        s.config.fullscreen = false;
        assert_eq!(s.policy().reasons, ["audio"]);
        assert!(s.policy().night_outputs.is_empty());
        s.config.inhibit_idle = false;
        assert!(!s.policy().idle && s.policy().sleep);
        s.config.inhibit_sleep = false;
        assert!(!s.policy().sleep);
        s.audio = false;
        s.manual = true;
        s.config.manual_pause_nightlight = true;
        assert!(s.policy().night_all);
        s.manual = false;
        s.config.fullscreen = true;
        s.unknown_fullscreen = true;
        assert!(s.policy().night_all);
        s.config.fullscreen_pause_nightlight = false;
        assert!(!s.policy().night_all);
        assert!(s.policy().night_outputs.is_empty());
        assert_eq!(s.policy().reasons, ["fullscreen"]);
        s.outputs.clear();
        s.unknown_fullscreen = false;
        assert!(s.policy().reasons.is_empty());
    }

    #[test]
    fn shutdown_cannot_reactivate_effects_from_late_source_updates() {
        // Sources and sleep acquisition stay disabled: no host connection is made.
        let root = tempfile::tempdir().unwrap();
        let config = cabby_config::Config::default();
        let idle = crate::idle::Idle::new(0, 0, root.path());
        let night = crate::night::Night::new(&config, root.path());
        let caffeine_config = cabby_config::Caffeine {
            inhibit_sleep: false,
            ..Default::default()
        };
        let caffeine = Caffeine::new(
            &caffeine_config,
            root.path(),
            idle.policy_handle(),
            night.policy_handle(),
        );
        caffeine.control("caffeine-on");
        assert_eq!(caffeine.status()["manual"], true);
        assert_eq!(idle.status()["inhibited"], true);
        caffeine.control("idle-inhibit-off");
        assert_eq!(caffeine.status()["active"], false);
        let handle = caffeine.handle.clone();
        drop(caffeine);
        handle.update(|s| {
            s.manual = true;
            s.audio = true;
            s.outputs.insert("TEST-1".into());
        });
        let state: serde_json::Value =
            serde_json::from_str(&handle.0.state.lock().unwrap().last).unwrap();
        assert_eq!(state["active"], false);
        assert_eq!(*handle.0.block.borrow(), None);
        assert_eq!(idle.status()["inhibited"], false);
    }

    struct Bus(std::process::Child);
    impl Drop for Bus {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    struct Manager(Arc<Mutex<Vec<std::os::unix::net::UnixStream>>>);
    #[zbus::interface(name = "org.freedesktop.login1.Manager")]
    impl Manager {
        fn inhibit(
            &self,
            what: &str,
            _who: &str,
            _why: &str,
            mode: &str,
        ) -> zbus::zvariant::OwnedFd {
            assert_eq!((what, mode), ("sleep", "block"));
            let (reader, writer) = std::os::unix::net::UnixStream::pair().unwrap();
            reader.set_nonblocking(true).unwrap();
            self.0.lock().unwrap().push(reader);
            let fd: std::os::fd::OwnedFd = writer.into();
            fd.into()
        }
    }
    async fn wait(mut condition: impl FnMut() -> bool) {
        tokio::time::timeout(Duration::from_secs(5), async {
            while !condition() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
    }
    #[tokio::test]
    async fn private_sleep_block_fd_cancels_and_releases_on_owner_loss() {
        use std::io::{BufRead, BufReader, Read};
        let mut daemon = std::process::Command::new("dbus-daemon")
            .args(["--session", "--nofork", "--print-address=1"])
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let mut address = String::new();
        BufReader::new(daemon.stdout.take().unwrap())
            .read_line(&mut address)
            .unwrap();
        let _bus = Bus(daemon);
        let address = address.trim().to_owned();
        let readers = Arc::new(Mutex::new(Vec::new()));
        let server = zbus::connection::Builder::address(address.as_str())
            .unwrap()
            .name("org.freedesktop.login1")
            .unwrap()
            .serve_at("/org/freedesktop/login1", Manager(readers.clone()))
            .unwrap()
            .build()
            .await
            .unwrap();
        for cancel in [true, false] {
            let address = address.clone();
            let (granted, ready) = tokio::sync::oneshot::channel();
            let task = tokio::spawn(async move {
                hold_block_inner(Some(&address), || {
                    let _ = granted.send(());
                })
                .await
            });
            tokio::time::timeout(Duration::from_secs(5), ready)
                .await
                .unwrap()
                .unwrap();
            let index = readers.lock().unwrap().len() - 1;
            assert!(readers.lock().unwrap()[index].read(&mut [0]).is_err());
            if cancel {
                task.abort();
                let _ = task.await;
            } else {
                server.release_name("org.freedesktop.login1").await.unwrap();
                task.await.unwrap().unwrap();
            }
            wait(|| {
                readers.lock().unwrap()[index]
                    .read(&mut [0])
                    .is_ok_and(|n| n == 0)
            })
            .await;
        }
    }
}
