//! Coordinator-owned, event-driven idle observation. No surface or UI ownership.
use std::{
    collections::{HashMap, HashSet},
    io::{Read, Write},
    os::{
        fd::{AsFd, AsRawFd},
        unix::{net::UnixStream, process::CommandExt},
    },
    process::{Child, Command},
    sync::{Arc, Mutex},
    thread::JoinHandle,
};
use wayland_client::{
    Connection, Dispatch, QueueHandle, delegate_noop,
    protocol::{wl_registry, wl_seat},
};
use wayland_protocols::ext::idle_notify::v1::client::{
    ext_idle_notification_v1::{self, ExtIdleNotificationV1},
    ext_idle_notifier_v1::ExtIdleNotifierV1,
};

#[derive(Default)]
struct Desired {
    epoch: u64,
    seconds: u32,
    display_seconds: u32,
    inhibited: bool,
    stop: bool,
    status: &'static str,
    path: std::path::PathBuf,
    published: String,
}
impl Desired {
    fn publish(&mut self) {
        use std::os::unix::fs::OpenOptionsExt;
        let value = serde_json::json!({"state": self.status, "inhibited": self.inhibited, "lock_seconds": self.seconds, "display_off_seconds": self.display_seconds}).to_string();
        if value == self.published {
            return;
        }
        let temporary = self.path.with_extension("tmp");
        let result = (|| -> std::io::Result<()> {
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW)
                .open(&temporary)?;
            file.write_all(value.as_bytes())?;
            std::fs::rename(&temporary, &self.path)
        })();
        if result.is_ok() {
            self.published = value;
        }
    }
}
pub struct Resume {
    desired: Arc<Mutex<Desired>>,
    wake: UnixStream,
}
impl Resume {
    pub fn resume(&self) {
        self.desired.lock().unwrap().epoch += 1;
        let _ = (&self.wake).write(&[1]);
    }
}
pub struct Idle {
    desired: Arc<Mutex<Desired>>,
    wake: UnixStream,
    thread: Option<JoinHandle<()>>,
}
impl Idle {
    pub fn new(seconds: u32, display_seconds: u32, root: &std::path::Path) -> Self {
        let (wake, reader) = UnixStream::pair().expect("idle wake socket");
        wake.set_nonblocking(true).expect("idle wake nonblocking");
        reader.set_nonblocking(true).expect("idle wake nonblocking");
        let desired = Arc::new(Mutex::new(Desired {
            seconds,
            display_seconds,
            path: root.join("idle.json"),
            status: "starting",
            ..Desired::default()
        }));
        let state = desired.clone();
        let executable = std::env::current_exe().expect("coordinator executable");
        let thread = std::thread::spawn(move || worker(state, reader, executable));
        Self {
            desired,
            wake,
            thread: Some(thread),
        }
    }
    pub fn resume_handle(&self) -> Resume {
        Resume {
            desired: self.desired.clone(),
            wake: self.wake.try_clone().expect("resume wake socket"),
        }
    }
    fn wake(&self) {
        let _ = (&self.wake).write(&[1]);
    }
    pub fn configure(&self, seconds: u32, display_seconds: u32) {
        let mut state = self.desired.lock().unwrap();
        if state.seconds == seconds && state.display_seconds == display_seconds {
            return;
        }
        state.seconds = seconds;
        state.display_seconds = display_seconds;
        state.publish();
        drop(state);
        self.wake();
    }
    pub fn inhibit(&self, value: Option<bool>) {
        let mut state = self.desired.lock().unwrap();
        state.inhibited = value.unwrap_or(!state.inhibited);
        state.publish();
        drop(state);
        self.wake();
    }
    pub fn status(&self) -> serde_json::Value {
        let state = self.desired.lock().unwrap();
        serde_json::json!({"state": state.status, "inhibited": state.inhibited, "lock_seconds": state.seconds})
    }
}
impl Drop for Idle {
    fn drop(&mut self) {
        self.desired.lock().unwrap().stop = true;
        self.wake();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
struct Action(Child);
impl Drop for Action {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
#[derive(Default)]
struct State {
    executable: std::path::PathBuf,
    manager: Option<(u32, ExtIdleNotifierV1)>,
    seats: HashMap<u32, wl_seat::WlSeat>,
    notifications: Vec<ExtIdleNotificationV1>,
    idle: HashSet<u32>,
    display_idle: HashSet<u32>,
    display_seconds: u32,
    display_fired: bool,
    power_touched: bool,
    action_kind: &'static str,
    generation: u64,
    seconds: u32,
    action: Option<Action>,
    fired: bool,
    error: bool,
}
impl State {
    fn rebuild(&mut self, qh: &QueueHandle<Self>) {
        self.generation += 1;
        for n in self.notifications.drain(..) {
            n.destroy();
        }
        self.restore();
        self.idle.clear();
        self.display_idle.clear();
        self.display_fired = false;
        self.fired = false;
        self.error = false;
        if let Some((_, manager)) = &self.manager {
            for (name, seat) in &self.seats {
                self.notifications.push(manager.get_idle_notification(
                    self.seconds * 1000,
                    seat,
                    qh,
                    (*name, self.generation, false),
                ));
                if self.display_seconds > 0 {
                    self.notifications.push(manager.get_idle_notification(
                        self.display_seconds * 1000,
                        seat,
                        qh,
                        (*name, self.generation, true),
                    ));
                }
            }
        }
    }
    fn fire(&mut self) {
        if self.action.is_some() || self.seats.is_empty() {
            return;
        }
        let command = if !self.display_fired && self.display_idle.len() == self.seats.len() {
            self.display_fired = true;
            self.fired = true;
            "display-off"
        } else if !self.fired && self.idle.len() == self.seats.len() {
            self.fired = true;
            "lock"
        } else {
            return;
        };
        self.action_kind = command;
        let result = {
            let parent = std::process::id() as libc::pid_t;
            let mut command = Command::new(&self.executable);
            command.arg(self.action_kind);
            unsafe {
                command.pre_exec(move || {
                    if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) == -1 {
                        return Err(std::io::Error::last_os_error());
                    }
                    if libc::getppid() != parent {
                        return Err(std::io::Error::other("idle owner exited"));
                    }
                    Ok(())
                });
            }
            command.spawn()
        };
        match result {
            Ok(child) => {
                self.action = Some(Action(child));
                if self.action_kind == "display-off" {
                    self.power_touched = true;
                }
            }
            Err(_) => self.error = true,
        }
    }
}
impl State {
    fn restore(&mut self) {
        if self.power_touched {
            self.action = None;
            self.error = crate::power::monitors(false).is_err();
            self.power_touched = false;
        }
    }
}
impl Drop for State {
    fn drop(&mut self) {
        self.restore();
    }
}
impl Dispatch<wl_registry::WlRegistry, ()> for State {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_registry::Event::Global {
                name,
                interface,
                version,
            } => {
                match interface.as_str() {
                    "ext_idle_notifier_v1" if state.manager.is_none() => {
                        state.manager = Some((name, registry.bind(name, version.min(2), qh, ())))
                    }
                    "wl_seat" => {
                        state
                            .seats
                            .insert(name, registry.bind(name, version.min(7), qh, ()));
                    }
                    _ => return,
                }
                state.rebuild(qh);
            }
            wl_registry::Event::GlobalRemove { name } => {
                if !state.seats.contains_key(&name)
                    && !state.manager.as_ref().is_some_and(|(id, _)| *id == name)
                {
                    return;
                }
                if state.manager.as_ref().is_some_and(|(id, _)| *id == name) {
                    state.manager.take().unwrap().1.destroy();
                }
                if let Some(seat) = state.seats.remove(&name)
                    && seat.version() >= 5
                {
                    seat.release();
                }
                state.rebuild(qh);
            }
            _ => (),
        }
    }
}
impl Dispatch<ExtIdleNotificationV1, (u32, u64, bool)> for State {
    fn event(
        state: &mut Self,
        _: &ExtIdleNotificationV1,
        event: ext_idle_notification_v1::Event,
        &(seat, generation, display): &(u32, u64, bool),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if generation != state.generation {
            return;
        }
        match event {
            ext_idle_notification_v1::Event::Idled => {
                if display {
                    state.display_idle.insert(seat);
                } else {
                    state.idle.insert(seat);
                }
                state.fire();
            }
            ext_idle_notification_v1::Event::Resumed => {
                state.restore();
                if display {
                    state.display_idle.remove(&seat);
                    state.display_fired = false;
                } else {
                    state.idle.remove(&seat);
                    state.fired = false;
                }
            }
            _ => (),
        }
    }
}
use wayland_client::Proxy;
delegate_noop!(State: ignore wl_seat::WlSeat);
delegate_noop!(State: ignore ExtIdleNotifierV1);
pub(crate) fn wait(wake: &UnixStream, fd: i32, timeout: i32) -> std::io::Result<bool> {
    let mut descriptors = [
        libc::pollfd {
            fd: wake.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        },
        libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        },
    ];
    let result = unsafe { libc::poll(descriptors.as_mut_ptr(), 2, timeout) };
    if result < 0 {
        return Err(std::io::Error::last_os_error());
    }
    if descriptors[0].revents != 0 {
        let mut bytes = [0; 64];
        while (&*wake).read(&mut bytes).is_ok_and(|n| n > 0) {}
    }
    Ok(descriptors[1].revents != 0)
}
fn worker(desired: Arc<Mutex<Desired>>, wake: UnixStream, executable: std::path::PathBuf) {
    loop {
        let (seconds, display_seconds, epoch) = {
            let mut d = desired.lock().unwrap();
            if d.stop {
                return;
            }
            if d.seconds == 0 || d.inhibited {
                d.status = if d.inhibited { "inhibited" } else { "disabled" };
                d.publish();
                drop(d);
                let _ = wait(&wake, -1, -1);
                continue;
            }
            d.status = "connecting";
            d.publish();
            (d.seconds, d.display_seconds, d.epoch)
        };
        if connected(
            &desired,
            &wake,
            seconds,
            display_seconds,
            epoch,
            &executable,
        )
        .is_err()
        {
            {
                let mut d = desired.lock().unwrap();
                d.status = "unavailable";
                d.publish();
            }
            let _ = wait(&wake, -1, 2000);
        }
    }
}
fn connected(
    desired: &Mutex<Desired>,
    wake: &UnixStream,
    seconds: u32,
    display_seconds: u32,
    epoch: u64,
    executable: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let connection = Connection::connect_to_env()?;
    let mut queue = connection.new_event_queue();
    let qh = queue.handle();
    connection.display().get_registry(&qh, ());
    let mut state = State {
        executable: executable.to_owned(),
        seconds,
        display_seconds,
        manager: None,
        seats: HashMap::new(),
        notifications: Vec::new(),
        idle: HashSet::new(),
        display_idle: HashSet::new(),
        display_fired: false,
        power_touched: false,
        action_kind: "",
        generation: 0,
        action: None,
        fired: false,
        error: false,
    };
    loop {
        {
            let d = desired.lock().unwrap();
            if d.stop
                || d.inhibited
                || d.seconds != seconds
                || d.display_seconds != display_seconds
                || d.epoch != epoch
            {
                return Ok(());
            }
        }
        queue.dispatch_pending(&mut state)?;
        if let Some(action) = &mut state.action
            && let Some(status) = action.0.try_wait()?
        {
            state.error = !status.success();
            state.action = None;
        }
        state.fire();
        let mut d = desired.lock().unwrap();
        d.status = if state.error {
            if state.action_kind == "display-off" {
                "display-failed"
            } else {
                "lock-failed"
            }
        } else if state.action.is_some() {
            "locking"
        } else if state.manager.is_none() || state.seats.is_empty() {
            "unsupported"
        } else {
            "watching"
        };
        d.publish();
        drop(d);
        connection.flush()?;
        let Some(reader) = queue.prepare_read() else {
            continue;
        };
        if wait(
            wake,
            connection.as_fd().as_raw_fd(),
            if state.action.is_some() { 100 } else { -1 },
        )? {
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn disabled_worker_inhibition_and_shutdown_are_owned() {
        let root = tempfile::tempdir().unwrap();
        let idle = Idle::new(0, 0, root.path());
        idle.inhibit(Some(true));
        assert_eq!(idle.status()["inhibited"], true);
        idle.configure(0, 0);
        idle.inhibit(None);
        assert_eq!(idle.status()["inhibited"], false);
        drop(idle);
    }
}
