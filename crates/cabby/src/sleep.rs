//! Opt-in logind delay inhibitor. A delay can postpone sleep, never veto it.
use futures_util::StreamExt;
use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::watch;
use zbus::{
    MatchRule, MessageStream, Proxy, connection::Builder, fdo::DBusProxy, message::Type,
    zvariant::OwnedFd,
};
const NAME: &str = "org.freedesktop.login1";
const PATH: &str = "/org/freedesktop/login1";
const INTERFACE: &str = "org.freedesktop.login1.Manager";
const TIMEOUT: Duration = Duration::from_secs(3);
type Status = Arc<Mutex<&'static str>>;
fn set(status: &Status, value: &'static str) {
    *status.lock().unwrap() = value;
}
pub struct Sleep {
    control: watch::Sender<Option<bool>>,
    thread: Option<std::thread::JoinHandle<()>>,
    status: Status,
}
impl Sleep {
    pub fn new(enabled: bool, resume: crate::idle::Resume) -> Self {
        let (control, mut updates) = watch::channel(Some(enabled));
        let status = Arc::new(Mutex::new("starting"));
        let state = status.clone();
        let executable = std::env::current_exe().expect("coordinator executable");
        let thread = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("sleep runtime");
            runtime.block_on(async move {
                loop {
                    let enabled = *updates.borrow_and_update();
                    let Some(enabled) = enabled else { return; };
                    if !enabled {
                        set(&state, "disabled");
                        if updates.changed().await.is_err() { return; }
                        continue;
                    }
                    tokio::select! {
                        result = monitor(None, &executable, &state, || resume.resume()) => { if result.is_err() { set(&state, "unavailable"); } },
                        _ = updates.changed() => continue,
                    }
                    tokio::select! { _ = tokio::time::sleep(Duration::from_secs(2)) => (), _ = updates.changed() => () }
                }
            });
        });
        Self {
            control,
            thread: Some(thread),
            status,
        }
    }
    pub fn configure(&self, enabled: bool) {
        self.control.send_if_modified(|v| {
            if *v == Some(enabled) {
                false
            } else {
                *v = Some(enabled);
                true
            }
        });
    }
    pub fn status(&self) -> &'static str {
        *self.status.lock().unwrap()
    }
}
impl Drop for Sleep {
    fn drop(&mut self) {
        let _ = self.control.send(None);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

async fn lock(executable: &Path, budget: Duration) -> bool {
    use std::os::unix::process::CommandExt;
    let mut command = tokio::process::Command::new(executable);
    command
        .arg("lock")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    let parent = std::process::id() as libc::pid_t;
    unsafe {
        command.as_std_mut().pre_exec(move || {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::getppid() != parent {
                return Err(std::io::Error::other("sleep owner exited"));
            }
            Ok(())
        });
    }
    tokio::time::timeout(budget, command.status())
        .await
        .is_ok_and(|result| result.is_ok_and(|status| status.success()))
}
async fn inhibit(proxy: &Proxy<'_>) -> zbus::Result<OwnedFd> {
    use std::os::fd::AsRawFd;
    let fd: OwnedFd = proxy
        .call(
            "Inhibit",
            &(
                "sleep",
                "Cabby Shell",
                "Confirm session lock before sleep",
                "delay",
            ),
        )
        .await?;
    // Never let a lock/PAM child inherit the inhibitor lease.
    if unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) } == -1 {
        return Err(zbus::Error::Failure(
            "cannot protect inhibitor descriptor".into(),
        ));
    }
    Ok(fd)
}
async fn monitor(
    address: Option<&str>,
    executable: &Path,
    status: &Status,
    resume: impl Fn(),
) -> zbus::Result<()> {
    let (connection, mut owners, mut signals, owner, budget) =
        tokio::time::timeout(TIMEOUT, async {
            let builder = match address {
                Some(a) => Builder::address(a)?,
                None => Builder::system()?,
            };
            let connection = builder.method_timeout(TIMEOUT).build().await?;
            let owners = MessageStream::for_match_rule(
                MatchRule::builder()
                    .msg_type(Type::Signal)
                    .sender("org.freedesktop.DBus")?
                    .interface("org.freedesktop.DBus")?
                    .member("NameOwnerChanged")?
                    .arg(0, NAME)?
                    .build(),
                &connection,
                Some(8),
            )
            .await?;
            let owner = DBusProxy::new(&connection)
                .await?
                .get_name_owner(NAME.try_into().unwrap())
                .await?
                .to_string();
            let signals = MessageStream::for_match_rule(
                MatchRule::builder()
                    .msg_type(Type::Signal)
                    .sender(owner.as_str())?
                    .path(PATH)?
                    .interface(INTERFACE)?
                    .member("PrepareForSleep")?
                    .build(),
                &connection,
                Some(8),
            )
            .await?;
            let proxy = Proxy::new(&connection, owner.as_str(), PATH, INTERFACE).await?;
            let delay: u64 = proxy.get_property("InhibitDelayMaxUSec").await?;
            // Reserve 100ms for scheduling/FD release; never retain a client >30s.
            let budget = Duration::from_micros(delay)
                .saturating_sub(Duration::from_millis(100))
                .min(Duration::from_secs(30));
            if budget.is_zero() {
                return Err(zbus::Error::Failure("no usable sleep delay".into()));
            }
            Ok::<_, zbus::Error>((connection, owners, signals, owner, budget))
        })
        .await
        .map_err(|_| zbus::Error::Failure("sleep setup timed out".into()))??;
    let proxy = Proxy::new(&connection, owner.as_str(), PATH, INTERFACE).await?;
    let mut lease = Some(inhibit(&proxy).await?);
    let mut sleeping: bool = proxy.get_property("PreparingForSleep").await?;
    let mut pending = sleeping.then(|| Box::pin(lock(executable, budget)));
    set(status, if sleeping { "locking" } else { "watching" });
    loop {
        tokio::select! {
            _ = connection.closed() => return Err(zbus::Error::Failure("sleep bus disconnected".into())),
            signal = owners.next() => {
                let signal = signal.ok_or_else(|| zbus::Error::Failure("sleep owner stream ended".into()))??;
                let (_, old, new): (String,String,String) = signal.body().deserialize()?;
                if old == owner && old != new { return Err(zbus::Error::Failure("logind owner changed".into())); }
            }
            signal = signals.next() => {
                let signal = signal.ok_or_else(|| zbus::Error::Failure("sleep stream ended".into()))??;
                if signal.body().len() > 16 { return Err(zbus::Error::Failure("invalid sleep signal".into())); }
                let (start,): (bool,) = signal.body().deserialize()?;
                if start == sleeping { continue; }
                sleeping = start;
                if start { pending = Some(Box::pin(lock(executable, budget))); set(status, "locking"); }
                else {
                    pending = None; lease.take();
                    resume();
                    lease = Some(inhibit(&proxy).await?);
                    set(status, "watching");
                }
            }
            ready = async { pending.as_mut().unwrap().await }, if pending.is_some() => {
                pending = None;
                lease.take();
                set(status, if ready { "sleep-ready" } else { "lock-failed" });
                // logind owns the final deadline and may sleep despite failure.
            }
        }
        // Keep the FD alive explicitly until readiness, cancellation or resume.
        let _ = &lease;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{BufRead, BufReader, Read},
        os::unix::{fs::PermissionsExt, net::UnixStream},
        process::{Child, Command, Stdio},
        sync::atomic::{AtomicUsize, Ordering},
    };
    struct Bus(Child);
    impl Drop for Bus {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    struct Manager {
        readers: Arc<Mutex<Vec<UnixStream>>>,
        delay: u64,
    }
    #[zbus::interface(name = "org.freedesktop.login1.Manager")]
    impl Manager {
        fn inhibit(&self, what: &str, _who: &str, _why: &str, mode: &str) -> OwnedFd {
            assert_eq!((what, mode), ("sleep", "delay"));
            let (reader, writer) = UnixStream::pair().unwrap();
            reader.set_nonblocking(true).unwrap();
            self.readers.lock().unwrap().push(reader);
            let fd: std::os::fd::OwnedFd = writer.into();
            fd.into()
        }
        #[zbus(property, name = "InhibitDelayMaxUSec")]
        fn inhibit_delay_max_usec(&self) -> u64 {
            self.delay
        }
        #[zbus(property)]
        fn preparing_for_sleep(&self) -> bool {
            false
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
    fn released(readers: &Mutex<Vec<UnixStream>>, index: usize) -> bool {
        let mut readers = readers.lock().unwrap();
        readers
            .get_mut(index)
            .is_some_and(|r| r.read(&mut [0]).is_ok_and(|n| n == 0))
    }
    #[tokio::test]
    async fn private_delay_fd_readiness_timeout_foreign_signals_resume_and_owner_loss() {
        for reject in [false, true] {
            let mut daemon = Command::new("dbus-daemon")
                .args(["--session", "--nofork", "--print-address=1"])
                .stdout(Stdio::piped())
                .spawn()
                .unwrap();
            let mut address = String::new();
            BufReader::new(daemon.stdout.take().unwrap())
                .read_line(&mut address)
                .unwrap();
            let _bus = Bus(daemon);
            let address = address.trim().to_owned();
            let readers = Arc::new(Mutex::new(Vec::new()));
            let server = Builder::address(address.as_str())
                .unwrap()
                .name(NAME)
                .unwrap()
                .serve_at(
                    PATH,
                    Manager {
                        readers: readers.clone(),
                        delay: if reject { 400_000 } else { 2_000_000 },
                    },
                )
                .unwrap()
                .build()
                .await
                .unwrap();
            let foreign = Builder::address(address.as_str())
                .unwrap()
                .build()
                .await
                .unwrap();
            let root = tempfile::tempdir().unwrap();
            let exe = root.path().join("locker");
            std::fs::write(&exe,format!("#!/usr/bin/python3\nimport os,sys,time\nfrom pathlib import Path\nassert sys.argv[1:]==['lock']\nPath(__file__).with_suffix('.pid').write_text(str(os.getpid()))\ntime.sleep({})\nsys.exit(0)\n",if reject { "60" } else { ".25" })).unwrap();
            std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o700)).unwrap();
            let state = Arc::new(Mutex::new("starting"));
            let status = state.clone();
            let resumes = Arc::new(AtomicUsize::new(0));
            let resumed = resumes.clone();
            let task = tokio::spawn(async move {
                monitor(Some(&address), &exe, &status, move || {
                    resumed.fetch_add(1, Ordering::SeqCst);
                })
                .await
            });
            wait(|| *state.lock().unwrap() == "watching").await;
            assert!(!released(&readers, 0));
            foreign
                .emit_signal(None::<&str>, PATH, INTERFACE, "PrepareForSleep", &(true,))
                .await
                .unwrap();
            tokio::time::sleep(Duration::from_millis(50)).await;
            assert_eq!(*state.lock().unwrap(), "watching");
            assert!(!released(&readers, 0));
            server
                .emit_signal(None::<&str>, PATH, INTERFACE, "PrepareForSleep", &(true,))
                .await
                .unwrap();
            wait(|| *state.lock().unwrap() == "locking").await;
            assert!(!released(&readers, 0));
            wait(|| *state.lock().unwrap() == if reject { "lock-failed" } else { "sleep-ready" })
                .await;
            wait(|| released(&readers, 0)).await;
            let pid = std::fs::read_to_string(root.path().join("locker.pid")).unwrap();
            wait(|| !Path::new(&format!("/proc/{pid}")).exists()).await;
            server
                .emit_signal(None::<&str>, PATH, INTERFACE, "PrepareForSleep", &(false,))
                .await
                .unwrap();
            wait(|| resumes.load(Ordering::SeqCst) == 1 && readers.lock().unwrap().len() == 2)
                .await;
            assert!(!released(&readers, 1));
            server
                .emit_signal(None::<&str>, PATH, INTERFACE, "PrepareForSleep", &(true,))
                .await
                .unwrap();
            wait(|| *state.lock().unwrap() == "locking").await;
            server.release_name(NAME).await.unwrap();
            assert!(task.await.unwrap().is_err());
            wait(|| released(&readers, 1)).await;
        }
    }
}
