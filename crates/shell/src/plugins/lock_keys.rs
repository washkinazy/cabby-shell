//! Read-only global LED state: Wayland keyboard modifiers require keyboard focus.
use iced::futures::SinkExt;
use std::{
    io::Read,
    path::PathBuf,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Keys {
    pub caps: Option<bool>,
    pub num: Option<bool>,
    pub reset: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct Settings {
    pub root: PathBuf,
    pub caps: bool,
    pub num: bool,
}
#[derive(Default, PartialEq, Eq)]
struct Paths {
    caps: Vec<PathBuf>,
    num: Vec<PathBuf>,
}
fn discover(settings: &Settings) -> Paths {
    let mut paths = Paths::default();
    if let Ok(entries) = std::fs::read_dir(&settings.root) {
        for entry in entries.take(256).filter_map(Result::ok) {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            let group = if settings.caps && name.ends_with("::capslock") {
                &mut paths.caps
            } else if settings.num && name.ends_with("::numlock") {
                &mut paths.num
            } else {
                continue;
            };
            if group.len() < 32 {
                group.push(entry.path().join("brightness"));
            }
        }
    }
    paths.caps.sort();
    paths.num.sort();
    paths
}
fn group(paths: &[PathBuf]) -> Option<bool> {
    if paths.is_empty() {
        return None;
    }
    let mut on = false;
    for path in paths {
        let mut value = String::new();
        std::fs::File::open(path)
            .ok()?
            .take(16)
            .read_to_string(&mut value)
            .ok()?;
        on |= value.trim().parse::<u32>().ok()? > 0;
    }
    Some(on)
}
fn read(paths: &Paths) -> Keys {
    Keys {
        caps: group(&paths.caps),
        num: group(&paths.num),
        reset: false,
    }
}
pub(super) fn events(settings: &Settings) -> impl iced::futures::Stream<Item = Keys> + use<> {
    let settings = settings.clone();
    iced::stream::channel(
        1,
        move |mut sender: iced::futures::channel::mpsc::Sender<Keys>| async move {
            let mut paths = Paths::default();
            let mut scan = Instant::now() - Duration::from_secs(3);
            let mut previous = None;
            let mut last_value = None;
            loop {
                // Only bounded LED files are read, never /dev/input or keycodes.
                let task_settings = settings.clone();
                let rescan = scan.elapsed() >= Duration::from_secs(2);
                let result = tokio::task::spawn_blocking(move || {
                    let next = if rescan {
                        discover(&task_settings)
                    } else {
                        paths
                    };
                    let value = read(&next);
                    (next, value)
                })
                .await;
                let Ok((next, mut value)) = result else {
                    return;
                };
                // A scan establishes a fresh baseline; membership changes must not
                // masquerade as a user toggling a lock key.
                if rescan {
                    scan = Instant::now();
                    value.reset = previous
                        .as_ref()
                        .is_none_or(|(caps, num)| (caps, num) != (&next.caps, &next.num));
                    previous = Some((next.caps.clone(), next.num.clone()));
                }
                paths = next;
                let delay = if value.caps.is_some() || value.num.is_some() {
                    200
                } else {
                    2000
                };
                let state = (value.caps, value.num);
                if value.reset || last_value != Some(state) {
                    if settings.caps
                        && value.caps.is_none()
                        && last_value.is_none_or(|(caps, _)| caps.is_some())
                    {
                        eprintln!("OSD Caps Lock indicator unavailable");
                    }
                    if settings.num
                        && value.num.is_none()
                        && last_value.is_none_or(|(_, num)| num.is_some())
                    {
                        eprintln!("OSD Num Lock indicator unavailable");
                    }
                    if sender.send(value).await.is_err() {
                        return;
                    }
                    last_value = Some(state);
                }
                tokio::time::sleep(Duration::from_millis(delay)).await;
            }
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn leds_aggregate_actual_state_and_missing_devices_are_unknown() {
        let root = tempfile::tempdir().unwrap();
        let add = |name: &str, value: &str| {
            let p = root.path().join(name);
            std::fs::create_dir_all(&p).unwrap();
            std::fs::write(p.join("brightness"), value).unwrap();
        };
        add("input1::capslock", "0\n");
        add("input2::capslock", "1\n");
        add("input1::numlock", "0\n");
        add("network::lan", "1\n");
        let settings = Settings {
            root: root.path().into(),
            caps: true,
            num: true,
        };
        let paths = discover(&settings);
        assert_eq!(read(&paths).caps, Some(true));
        assert_eq!(read(&paths).num, Some(false));
        std::fs::write(root.path().join("input2::capslock/brightness"), "0\n").unwrap();
        assert_eq!(read(&paths).caps, Some(false));
        std::fs::remove_dir_all(root.path().join("input2::capslock")).unwrap();
        assert_eq!(read(&paths).caps, None);
        assert_eq!(read(&discover(&settings)).caps, Some(false));
        std::fs::write(root.path().join("input1::capslock/brightness"), "invalid").unwrap();
        assert_eq!(read(&discover(&settings)).caps, None);
    }
}
