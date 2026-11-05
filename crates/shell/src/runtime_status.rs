use iced::futures::SinkExt;
use std::{
    io::Read,
    path::{Path, PathBuf},
};
pub(crate) fn events<T: serde::de::DeserializeOwned + Default + Send + 'static>(
    name: &'static str,
) -> impl iced::futures::Stream<Item = T> {
    let path = std::env::var_os("XDG_RUNTIME_DIR")
        .map(|p| PathBuf::from(p).join("cabby-shell").join(name));
    updates(path)
}
fn load<T: serde::de::DeserializeOwned + Default>(path: &Path) -> T {
    use std::os::unix::fs::OpenOptionsExt;
    let Ok(file) = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW)
        .open(path)
    else {
        return T::default();
    };
    if !file.metadata().is_ok_and(|m| m.is_file()) {
        return T::default();
    }
    let mut bytes = Vec::new();
    if file.take(1025).read_to_end(&mut bytes).is_err() || bytes.len() > 1024 {
        return T::default();
    }
    serde_json::from_slice(&bytes).unwrap_or_default()
}
pub(crate) fn updates<T: serde::de::DeserializeOwned + Default + Send + 'static>(
    path: Option<PathBuf>,
) -> impl iced::futures::Stream<Item = T> {
    iced::stream::channel(
        1,
        move |mut output: iced::futures::channel::mpsc::Sender<T>| async move {
            let Some(path) = path else {
                let _ = output.send(T::default()).await;
                std::future::pending::<()>().await;
                return;
            };
            loop {
                let Ok(watch) = cabby_config::watch::Watch::new([path.clone()]) else {
                    return;
                };
                let Ok(mut watch) = (unsafe { tokio::io::unix::AsyncFd::register(watch) }) else {
                    return;
                };
                let input = path.clone();
                let state = tokio::task::spawn_blocking(move || load(&input))
                    .await
                    .unwrap_or_default();
                if output.send(state).await.is_err() {
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
            }
        },
    )
}
