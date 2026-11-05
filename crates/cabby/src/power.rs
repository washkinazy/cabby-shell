//! Explicit power requests; the independent notify service owns lock readiness.
use cabby_config::Result;
use std::{
    os::unix::process::CommandExt,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

const QUERY_TIMEOUT: Duration = Duration::from_secs(5);
// The shipped lock service has a 15-second startup limit.
const LOCK_TIMEOUT: Duration = Duration::from_secs(20);
const SUSPEND_TIMEOUT: Duration = Duration::from_secs(10);

struct OwnedChild(Child);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn bounded(command: &mut Command, timeout: Duration, operation: &str) -> Result<()> {
    command.stdin(Stdio::null());
    let parent = std::process::id() as libc::pid_t;
    // systemctl has no interactive agent (--no-ask-password). Kill this owned
    // client if its CLI parent dies; killing it does not cancel a systemd job
    // already accepted, and must never stop/unlock the independent locker.
    unsafe {
        command.pre_exec(move || {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::getppid() != parent {
                return Err(std::io::Error::other("request parent exited"));
            }
            Ok(())
        });
    }
    let mut child = OwnedChild(command.spawn().map_err(|e| format!("{operation}: {e}"))?);
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child
            .0
            .try_wait()
            .map_err(|e| format!("{operation}: {e}"))?
        {
            return if status.success() {
                Ok(())
            } else {
                Err(format!("{operation} failed ({status})"))
            };
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "{operation} timed out; an accepted service job may still complete"
            ));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn ctl(args: &[&str], timeout: Duration, operation: &str) -> Result<()> {
    bounded(
        Command::new("systemctl")
            .arg("--no-ask-password")
            .args(args),
        timeout,
        operation,
    )
}

pub fn request(session: &str, locker: &str, suspend: bool) -> Result<()> {
    ctl(
        &["--user", "is-active", "--quiet", session],
        QUERY_TIMEOUT,
        "checking active compositor session",
    )?;
    // Deliberate requests renew the recovery budget. An unloaded unit has no
    // counters, so failure to reset is harmless; failure to start is not.
    let _ = ctl(
        &["--user", "reset-failed", locker],
        QUERY_TIMEOUT,
        "resetting locker recovery budget",
    );
    ctl(
        &["--user", "start", locker],
        LOCK_TIMEOUT,
        "waiting for compositor-confirmed lock",
    )?;
    // A service may exit just after notifying READY. Check each separately:
    // systemctl is-active with multiple units succeeds if *any* is active.
    ctl(
        &["--user", "is-active", "--quiet", session],
        QUERY_TIMEOUT,
        "rechecking compositor session after locking",
    )?;
    ctl(
        &["--user", "is-active", "--quiet", locker],
        QUERY_TIMEOUT,
        "rechecking confirmed locker",
    )?;
    if suspend {
        ctl(
            &["--check-inhibitors=yes", "--no-block", "suspend"],
            SUSPEND_TIMEOUT,
            "requesting suspend (the session remains locked on rejection)",
        )?;
        println!("Suspend requested after lock confirmation.");
    }
    Ok(())
}

/// Niri retains output configuration; this is DPMS, never output removal.
pub fn monitors(off: bool) -> Result<()> {
    bounded(
        Command::new("niri").args([
            "msg",
            "action",
            if off {
                "power-off-monitors"
            } else {
                "power-on-monitors"
            },
        ]),
        Duration::from_secs(5),
        if off {
            "requesting display power off"
        } else {
            "restoring display power"
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::PermissionsExt};

    #[test]
    fn timed_out_helper_is_killed_and_reaped() {
        let temp = tempfile::tempdir().unwrap();
        let script = temp.path().join("helper");
        let pid = temp.path().join("pid");
        fs::write(&script, "#!/usr/bin/python3\nimport os,sys,time\nopen(sys.argv[1], 'w').write(str(os.getpid()))\ntime.sleep(60)\n").unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
        let result = bounded(
            Command::new(script).arg(&pid),
            Duration::from_millis(500),
            "fixture",
        );
        assert!(result.unwrap_err().contains("timed out"));
        let pid = fs::read_to_string(pid).unwrap();
        assert!(!std::path::Path::new(&format!("/proc/{pid}")).exists());
    }
}
