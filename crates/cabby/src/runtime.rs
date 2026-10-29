use cabby_config::{Config, Result, Source, Store};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::linux::net::SocketAddrExt;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::os::unix::net::{SocketAddr, UnixDatagram, UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub fn directory(explicit: Option<PathBuf>) -> Result<PathBuf> {
    let path = explicit
        .or_else(|| {
            std::env::var_os("XDG_RUNTIME_DIR").map(|root| PathBuf::from(root).join("cabby-shell"))
        })
        .ok_or("XDG_RUNTIME_DIR is required (or supply --runtime-dir)")?;
    if !path.is_absolute() {
        return Err("runtime directory must be absolute".into());
    }
    Ok(path)
}

fn publish(path: &Path, config: &Config) -> Result<()> {
    let temporary = path.join("active.toml.tmp");
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&temporary)
        .map_err(|error| error.to_string())?;
    file.write_all(config.to_toml().as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(|error| error.to_string())?;
    fs::rename(temporary, path.join("active.toml")).map_err(|error| error.to_string())
}

fn notify_ready() -> Result<()> {
    if let Some(path) = std::env::var_os("NOTIFY_SOCKET") {
        use std::os::unix::ffi::OsStrExt;
        let bytes = path.as_bytes();
        let address = if bytes.first() == Some(&b'@') {
            SocketAddr::from_abstract_name(&bytes[1..])
        } else {
            SocketAddr::from_pathname(&path)
        }
        .map_err(|error| error.to_string())?;
        UnixDatagram::unbound()
            .and_then(|socket| socket.send_to_addr(b"READY=1", &address))
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

struct Ownership {
    _lock: File,
    socket: PathBuf,
}
impl Drop for Ownership {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.socket);
    }
}

pub fn serve(
    source: Source,
    path: PathBuf,
    ui_unit: Option<String>,
    wallpaper_unit: Option<String>,
) -> Result<()> {
    fs::create_dir_all(&path).map_err(|error| error.to_string())?;
    if fs::symlink_metadata(&path)
        .map_err(|error| error.to_string())?
        .file_type()
        .is_symlink()
    {
        return Err("runtime directory must not be a symlink".into());
    }
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700))
        .map_err(|error| error.to_string())?;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(path.join("owner.lock"))
        .map_err(|error| error.to_string())?;
    lock.try_lock()
        .map_err(|_| "another Cabby coordinator owns this runtime directory")?;
    let mut store = Store::open(source)?;
    publish(&path, store.active())?;
    let socket = path.join("control.sock");
    match fs::remove_file(&socket) {
        Ok(()) => (),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
        Err(error) => return Err(error.to_string()),
    }
    let listener = UnixListener::bind(&socket).map_err(|error| error.to_string())?;
    let _ownership = Ownership {
        _lock: lock,
        socket: socket.clone(),
    };
    fs::set_permissions(&socket, fs::Permissions::from_mode(0o600))
        .map_err(|error| error.to_string())?;
    notify_ready()?;
    for stream in listener.incoming() {
        let mut stream = stream.map_err(|error| error.to_string())?;
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .map_err(|error| error.to_string())?;
        stream
            .set_write_timeout(Some(Duration::from_secs(2)))
            .map_err(|error| error.to_string())?;
        let mut command = String::new();
        let result = BufReader::new((&stream).take(4097)).read_line(&mut command);
        let response = if result.is_err() || command.len() > 4096 || !command.ends_with('\n') {
            serde_json::json!({"ok": false, "error": "invalid or timed-out request"})
        } else {
            match command.trim() {
                "status" => {
                    serde_json::json!({"ok": true, "source": store.source.path, "config": store.active(), "pid": std::process::id()})
                }
                "reload" => match store.reload_with(|candidate| publish(&path, candidate)) {
                    Ok(changed) => {
                        let restart = if changed {
                            ui_unit.as_ref().map(|unit| {
                                std::process::Command::new("systemctl")
                                    .args(["--user", "--no-block", "try-restart", unit])
                                    .status()
                                    .is_ok_and(|status| status.success())
                            })
                        } else {
                            None
                        };
                        let wallpaper_restart = if changed {
                            wallpaper_unit.as_ref().map(|unit| {
                                std::process::Command::new("systemctl")
                                    .args(["--user", "--no-block", "try-restart", unit])
                                    .status()
                                    .is_ok_and(|s| s.success())
                            })
                        } else {
                            None
                        };
                        serde_json::json!({"ok": true, "changed": changed, "ui_restart_queued": restart, "wallpaper_restart_queued": wallpaper_restart, "config": store.active()})
                    }
                    Err(error) => serde_json::json!({"ok": false, "error": error}),
                },
                _ => serde_json::json!({"ok": false, "error": "unknown request"}),
            }
        };
        // An abandoned caller must not take down the coordinator.
        let _ = writeln!(stream, "{response}");
    }
    Ok(())
}

pub fn request(path: &Path, command: &str) -> Result<serde_json::Value> {
    let mut stream = UnixStream::connect(path.join("control.sock"))
        .map_err(|error| format!("coordinator unavailable: {error}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|error| error.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .map_err(|error| error.to_string())?;
    writeln!(stream, "{command}").map_err(|error| error.to_string())?;
    let mut response = String::new();
    BufReader::new(stream.take(65537))
        .read_line(&mut response)
        .map_err(|error| error.to_string())?;
    if response.len() > 65536 {
        return Err("oversized response".into());
    }
    serde_json::from_str(&response).map_err(|error| error.to_string())
}
