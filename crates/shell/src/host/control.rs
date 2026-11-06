//! Small desktop activation endpoint. No clipboard payloads cross this socket.
use iced::{Subscription, futures::SinkExt};
use std::path::{Path, PathBuf};
#[derive(Clone, Copy, Debug)]
pub(super) enum Command {
    Clipboard,
    Screenshot(bool),
    ScreenshotCancel,
}
pub(super) fn directory(config: &Path) -> Option<PathBuf> {
    let managed = config.file_name().is_some_and(|s| s == "active.toml")
        && config
            .parent()
            .is_some_and(|p| p.join("owner.lock").is_file());
    let root = if managed {
        config.parent().map(Path::to_path_buf)
    } else {
        std::env::var_os("XDG_RUNTIME_DIR").map(|p| PathBuf::from(p).join("cabby-shell"))
    };
    root.filter(|p| p.is_absolute())
}
pub(super) fn subscription(root: PathBuf) -> Subscription<Command> {
    Subscription::run_with(root, |root| events(root))
}
fn events(root: &Path) -> impl iced::futures::Stream<Item = Command> + use<> {
    let root = root.to_path_buf();
    iced::stream::channel(
        1,
        move |mut output: iced::futures::channel::mpsc::Sender<Command>| async move {
            use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
            use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
            let setup = (|| -> std::io::Result<_> {
                std::fs::create_dir_all(&root)?;
                if std::fs::symlink_metadata(&root)?.file_type().is_symlink() {
                    return Err(std::io::Error::other("invalid runtime directory"));
                }
                std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))?;
                let lock = std::fs::OpenOptions::new()
                    .create(true)
                    .truncate(false)
                    .write(true)
                    .mode(0o600)
                    .custom_flags(libc::O_NOFOLLOW)
                    .open(root.join("desktop.lock"))?;
                lock.try_lock().map_err(std::io::Error::other)?;
                let path = root.join("desktop.sock");
                let _ = std::fs::remove_file(&path);
                let listener = tokio::net::UnixListener::bind(&path)?;
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
                Ok((lock, listener, path))
            })();
            let Ok((_lock, listener, path)) = setup else {
                eprintln!("Desktop activation unavailable");
                return;
            };
            struct Cleanup(PathBuf);
            impl Drop for Cleanup {
                fn drop(&mut self) {
                    let _ = std::fs::remove_file(&self.0);
                }
            }
            let _cleanup = Cleanup(path);
            crate::plugins::screenshot::idle(&root);
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    return;
                };
                let command = tokio::time::timeout(std::time::Duration::from_secs(1), async {
                    let mut line = String::new();
                    use tokio::io::AsyncReadExt;
                    tokio::io::BufReader::new((&mut stream).take(257))
                        .read_line(&mut line)
                        .await?;
                    Ok::<_, std::io::Error>(line)
                })
                .await;
                let Ok(Ok(line)) = command else {
                    continue;
                };
                let command = match line.as_str() {
                    "clipboard\n" => Command::Clipboard,
                    "screenshot\n" => Command::Screenshot(false),
                    "screenshot-monitor\n" => Command::Screenshot(true),
                    "screenshot-cancel\n" => Command::ScreenshotCancel,
                    _ => {
                        let _ = stream.write_all(b"error\n").await;
                        continue;
                    }
                };
                if output.send(command).await.is_err() {
                    return;
                }
                let _ = tokio::time::timeout(
                    std::time::Duration::from_secs(1),
                    stream.write_all(b"ok\n"),
                )
                .await;
            }
        },
    )
}
