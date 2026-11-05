//! Bounded icon lookup shared by in-repository UI plugins. Call from a blocking worker.
use iced::widget::{image, svg};
use std::os::unix::fs::OpenOptionsExt;
use std::{
    collections::VecDeque,
    io::Read,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
#[derive(Clone, Debug)]
pub(crate) enum Icon {
    Raster(image::Handle),
    Vector(svg::Handle),
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Key(pub String, pub String);
type Entries = VecDeque<(Key, Option<Icon>)>;
#[derive(Clone, Default)]
pub(crate) struct Cache(Arc<Mutex<Entries>>);
pub(crate) async fn load(cache: Cache, keys: Vec<Key>) -> Vec<(Key, Option<Icon>)> {
    let fallback = keys.clone();
    tokio::task::spawn_blocking(move || {
        keys.into_iter()
            .take(64)
            .map(|key| {
                let mut entries = cache.0.lock().unwrap();
                if let Some(index) = entries.iter().position(|(k, _)| k == &key) {
                    let entry = entries.remove(index).unwrap();
                    entries.push_back(entry.clone());
                    return entry;
                }
                drop(entries);
                let icon = themed_icon(&key.0, &key.1);
                let mut entries = cache.0.lock().unwrap();
                if entries.len() >= 64 {
                    entries.pop_front();
                }
                entries.push_back((key.clone(), icon.clone()));
                (key, icon)
            })
            .collect()
    })
    .await
    .unwrap_or_else(|_| fallback.into_iter().map(|key| (key, None)).collect())
}
fn themed_icon(name: &str, extra: &str) -> Option<Icon> {
    if name.is_empty() {
        return None;
    }
    let mut candidates = Vec::new();
    if Path::new(name).is_absolute() {
        candidates.push(PathBuf::from(name));
    } else if !name.contains('/') && name != "." && name != ".." {
        let mut roots = Vec::new();
        if Path::new(extra).is_absolute() {
            roots.push(PathBuf::from(extra));
        }
        if let Some(home) = std::env::var_os("HOME") {
            roots.push(PathBuf::from(&home).join(".icons"));
            roots.push(
                std::env::var_os("XDG_DATA_HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from(home).join(".local/share"))
                    .join("icons"),
            );
        }
        for root in std::env::split_paths(
            &std::env::var_os("XDG_DATA_DIRS")
                .unwrap_or_else(|| "/usr/local/share:/usr/share".into()),
        ) {
            roots.push(root.join("icons"));
            candidates.push(root.join("pixmaps").join(format!("{name}.png")));
            candidates.push(root.join("pixmaps").join(format!("{name}.svg")));
        }
        for root in roots {
            for suffix in ["", "hicolor", "Adwaita"] {
                let base = root.join(suffix);
                for size in [
                    "", "24x24", "22x22", "32x32", "16x16", "48x48", "scalable", "symbolic",
                ] {
                    for category in ["", "status", "apps", "devices", "actions"] {
                        for extension in ["png", "svg"] {
                            candidates.push(
                                base.join(size)
                                    .join(category)
                                    .join(format!("{name}.{extension}")),
                            );
                        }
                    }
                }
            }
        }
    }
    for path in candidates.into_iter().take(4096) {
        let Ok(file) = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(&path)
        else {
            continue;
        };
        if !file
            .metadata()
            .is_ok_and(|m| m.is_file() && m.len() <= 1024 * 1024)
        {
            continue;
        }
        let mut bytes = Vec::new();
        if file.take(1024 * 1024 + 1).read_to_end(&mut bytes).is_err() || bytes.len() > 1024 * 1024
        {
            continue;
        }
        if path.extension().is_some_and(|e| e == "svg") {
            return Some(Icon::Vector(svg::Handle::from_memory(bytes)));
        }
        let Ok(mut reader) =
            image_codec::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format()
        else {
            continue;
        };
        let mut limits = image_codec::Limits::default();
        limits.max_image_width = Some(512);
        limits.max_image_height = Some(512);
        limits.max_alloc = Some(4 * 1024 * 1024);
        reader.limits(limits);
        if let Ok(decoded) = reader.decode() {
            let rgba = decoded.into_rgba8();
            return Some(Icon::Raster(image::Handle::from_rgba(
                rgba.width(),
                rgba.height(),
                rgba.into_raw(),
            )));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn cached_icons_and_invalid_assets_are_bounded() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("icon.png");
        image_codec::RgbaImage::from_pixel(4, 4, image_codec::Rgba([1, 2, 3, 255]))
            .save(&path)
            .unwrap();
        let key = Key(path.to_string_lossy().into_owned(), String::new());
        let cache = Cache::default();
        assert!(matches!(
            load(cache.clone(), vec![key.clone()]).await[0].1,
            Some(Icon::Raster(_))
        ));
        std::fs::remove_file(path).unwrap();
        assert!(load(cache.clone(), vec![key]).await[0].1.is_some());
        for i in 0..80 {
            let _ = load(
                cache.clone(),
                vec![Key(format!("/missing/{i}"), String::new())],
            )
            .await;
        }
        assert_eq!(cache.0.lock().unwrap().len(), 64);
        let path = directory.path().join("large.png");
        image_codec::RgbaImage::new(513, 1).save(&path).unwrap();
        assert!(
            load(
                cache,
                vec![Key(path.to_string_lossy().into_owned(), String::new())]
            )
            .await[0]
                .1
                .is_none()
        );
    }
}
