//! Bounded, opt-in location lookup using Noctalia's public location API.
//! Coordinates stay in memory; response/address contents are never logged.
use std::{
    io::Read,
    os::{fd::AsRawFd, unix::process::CommandExt},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
pub struct Fetch {
    child: Child,
    started: Instant,
    bytes: Vec<u8>,
}
impl Fetch {
    pub fn start(location: &cabby_config::Location) -> std::io::Result<Self> {
        Self::with_executable(location, std::path::Path::new("curl"))
    }
    fn with_executable(
        location: &cabby_config::Location,
        exe: &std::path::Path,
    ) -> std::io::Result<Self> {
        let url = if location.auto_locate {
            "https://api.noctalia.dev/geolocate".to_owned()
        } else {
            let city: String = location
                .address
                .bytes()
                .map(|b| {
                    if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                        (b as char).to_string()
                    } else {
                        format!("%{b:02X}")
                    }
                })
                .collect();
            format!("https://api.noctalia.dev/geocode?city={city}")
        };
        let parent = std::process::id() as libc::pid_t;
        let mut command = Command::new(exe);
        command
            .args([
                "--disable",
                "--fail",
                "--silent",
                "--proto",
                "=https",
                "--max-time",
                "8",
                "--max-filesize",
                "16384",
                &url,
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        unsafe {
            command.pre_exec(move || {
                if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                if libc::getppid() != parent {
                    return Err(std::io::Error::other("location owner exited"));
                }
                Ok(())
            });
        }
        let child = command.spawn()?;
        let fetch = Self {
            child,
            started: Instant::now(),
            bytes: Vec::new(),
        };
        let fd = fetch.child.stdout.as_ref().unwrap().as_raw_fd();
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(fetch)
    }
    fn drain(&mut self) -> Result<(), ()> {
        let mut buf = [0; 4096];
        loop {
            match self.child.stdout.as_mut().unwrap().read(&mut buf) {
                Ok(0) => return Ok(()),
                Ok(n) => {
                    self.bytes.extend_from_slice(&buf[..n]);
                    if self.bytes.len() > 16384 {
                        return Err(());
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => return Ok(()),
                Err(_) => return Err(()),
            }
        }
    }
    pub fn next(&mut self) -> Option<Result<(f64, f64), ()>> {
        if self.drain().is_err() {
            return Some(Err(()));
        }
        match self.child.try_wait() {
            Ok(Some(status)) => Some(if status.success() && self.drain().is_ok() {
                decode(&self.bytes)
            } else {
                Err(())
            }),
            Ok(None) if self.started.elapsed() < Duration::from_secs(9) => None,
            _ => Some(Err(())),
        }
    }
}
impl Drop for Fetch {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
fn decode(bytes: &[u8]) -> Result<(f64, f64), ()> {
    let v: serde_json::Value = serde_json::from_slice(bytes).map_err(|_| ())?;
    let lat = v["lat"].as_f64().ok_or(())?;
    let lon = v["lng"].as_f64().ok_or(())?;
    if !lat.is_finite() || !lon.is_finite() || lat.abs() > 90. || lon.abs() > 180. {
        return Err(());
    }
    Ok((lat, lon))
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    #[test]
    fn optional_network_helper_is_bounded_redacted_and_owned() {
        let root = tempfile::tempdir().unwrap();
        let exe = root.path().join("curl");
        std::fs::write(&exe,"#!/usr/bin/python3\nimport json,sys\nassert '--proto' in sys.argv and '=https' in sys.argv\nassert sys.argv[-1]=='https://api.noctalia.dev/geocode?city=Test%20City%26x%3D1'\nprint(json.dumps({'lat':52.52,'lng':13.405}))\n").unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o700)).unwrap();
        let location = cabby_config::Location {
            address: "Test City&x=1".into(),
            ..Default::default()
        };
        let mut fetch = Fetch::with_executable(&location, &exe).unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if let Some(result) = fetch.next() {
                assert_eq!(result, Ok((52.52, 13.405)));
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(decode(br#"{"lat":91,"lng":0}"#).is_err());
        assert!(decode(br#"{"lat":"bad","lng":0}"#).is_err());
    }
}
