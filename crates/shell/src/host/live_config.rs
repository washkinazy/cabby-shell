//! Event-driven snapshots. Decode off the UI thread, serialize loads, retain the
//! last valid scene on errors. Managed wallpaper edits arrive via active.toml.
use cabby_config::{Config, Source, watch::Watch};
use iced::{Subscription, futures::SinkExt};
use std::{path::PathBuf, sync::Arc};
#[derive(Clone)]
pub struct Update {
    pub config: Config,
    pub asset: Option<Arc<cabby_config::wallpaper::Asset>>,
}
impl std::fmt::Debug for Update {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ConfigUpdate")
    }
}
pub fn subscription(path: PathBuf, wallpaper: bool) -> Subscription<Update> {
    Subscription::run_with((path, wallpaper), updates)
}
fn updates(input: &(PathBuf, bool)) -> impl iced::futures::Stream<Item = Update> + use<> {
    let (path, wallpaper) = input.clone();
    iced::stream::channel(
        1,
        move |mut output: iced::futures::channel::mpsc::Sender<Update>| async move {
            loop {
                let Ok(watch) = Watch::new([path.clone()]) else {
                    eprintln!("UI configuration watch unavailable");
                    return;
                };
                // SAFETY: Watch owns its descriptor; only drain is called while registered.
                let Ok(mut watch) = (unsafe { tokio::io::unix::AsyncFd::register(watch) }) else {
                    return;
                };
                let source = Source {
                    path: path.clone(),
                    explicit: true,
                };
                if let Ok(Ok(update)) =
                    tokio::task::spawn_blocking(move || -> cabby_config::Result<Update> {
                        let config = source.load(false)?;
                        let asset = if wallpaper {
                            cabby_config::wallpaper::load(&config)?.map(Arc::new)
                        } else {
                            None
                        };
                        Ok(Update { config, asset })
                    })
                    .await
                    && output.send(update).await.is_err()
                {
                    return;
                }
                loop {
                    let Ok(mut ready) = watch.readable_mut().await else {
                        return;
                    };
                    ready.clear_ready();
                    drop(ready);
                    match watch.get_mut().drain() {
                        Ok(true) => break,
                        Ok(false) => (),
                        Err(_) => return,
                    }
                }
                tokio::time::sleep(cabby_config::watch::DEBOUNCE).await;
                if watch.get_mut().drain().is_err() {
                    return;
                }
            }
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::futures::StreamExt;
    #[tokio::test]
    async fn atomic_invalid_and_recreated_files_preserve_last_good_update() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("config");
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("config.toml");
        std::fs::write(&path, "bar_height=40").unwrap();
        let mut stream = Box::pin(updates(&(path.clone(), true)));
        assert_eq!(stream.next().await.unwrap().config.bar_height, 40);
        std::fs::write(&path, "bar_height=0").unwrap();
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(650), stream.next())
                .await
                .is_err()
        );
        std::fs::remove_dir_all(&directory).unwrap();
        std::fs::create_dir(&directory).unwrap();
        std::fs::write(directory.join("temp"), "bar_height=48").unwrap();
        std::fs::rename(directory.join("temp"), &path).unwrap();
        let update = tokio::time::timeout(std::time::Duration::from_secs(3), stream.next())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(update.config.bar_height, 48);
        std::fs::write(&path, "bar_height=56").unwrap();
        assert_eq!(
            tokio::time::timeout(std::time::Duration::from_secs(3), stream.next())
                .await
                .unwrap()
                .unwrap()
                .config
                .bar_height,
            56
        );
    }
}
