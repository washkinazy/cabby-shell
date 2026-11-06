//! Shared startup for repository-owned features.
//!
//! Each existing executable selects one built-in plugin. This registry defines
//! code ownership, not process isolation or a dynamic loading interface.
use crate::ui;
mod control;
mod hint;
mod launcher;
mod live_config;
mod lock;
mod notifications;
mod panel;
mod surfaces;
use cabby_config::{Config, Source};

/// The built-in features currently migrated to the shared host.
#[derive(Clone, Copy)]
pub enum Plugin {
    Desktop,
    Lock,
    Bar,
    Launcher,
    Wallpaper,
    Notifications,
}

/// Validated startup inputs. The source is retained for launcher helper calls.
pub(crate) struct Context {
    pub config: Config,
    pub source: Source,
}

fn load_config(source: &Source, plugin: Plugin) -> cabby_config::Result<Config> {
    match source.load(true) {
        Ok(config) => Ok(config),
        Err(_) if matches!(plugin, Plugin::Lock) => {
            eprintln!("Invalid lock styling configuration; using defaults");
            Ok(Config::default())
        }
        Err(error) => Err(error),
    }
}

/// Validate inputs and initialize shared process state before backend tasks.
fn prepare(plugin: Plugin) -> Context {
    let context = ui::configuration_source()
        .and_then(|source| {
            let config = load_config(&source, plugin)?;
            Ok(Context { config, source })
        })
        .unwrap_or_else(|error| {
            // Preserve the notification frontend's content-free diagnostics.
            if matches!(plugin, Plugin::Notifications) {
                eprintln!("notification configuration unavailable");
            } else {
                eprintln!("{error}");
            }
            std::process::exit(1)
        });
    // Renderer re-exec must precede event loops and backend worker threads.
    ui::renderer(&context.config);
    crate::renderer_diagnostics();
    // The launcher retains clipboard support for its text input.
    if !matches!(plugin, Plugin::Launcher) {
        iced_exwlshell::disable_clipboard();
    }
    context
}

/// Run one feature in the caller's supervised process, on its main thread.
pub fn run(plugin: Plugin) -> iced_exwlshell::Result {
    let context = prepare(plugin);
    match plugin {
        Plugin::Lock => lock::run(context, None),
        Plugin::Desktop => surfaces::run(context, true, true),
        Plugin::Bar => surfaces::run(context, true, false),
        Plugin::Launcher => launcher::run(context),
        Plugin::Wallpaper => surfaces::run(context, false, true),
        Plugin::Notifications => notifications::run(context),
    }
}

/// Exercise the actual audio panel with an explicitly injected fixture helper.
pub fn audio_probe() -> iced_exwlshell::Result {
    let guard = std::env::var("CABBY_TEST_WAYLAND").expect("disposable session guard required");
    assert!(!guard.is_empty());
    assert_eq!(Some(guard), std::env::var("WAYLAND_DISPLAY").ok());
    let helper = std::path::PathBuf::from(
        std::env::var_os("CABBY_TEST_AUDIO_HELPER").expect("private audio helper required"),
    );
    assert!(helper.is_absolute());
    surfaces::run_with_audio(
        prepare(Plugin::Desktop),
        true,
        true,
        Some(cabby_audio::Client::with_executable(helper)),
    )
}
/// Synthetic audio/LED sources on a disposable display; production never reads these overrides.
pub fn osd_probe() -> iced_exwlshell::Result {
    let guard = std::env::var("CABBY_TEST_WAYLAND").expect("private display guard required");
    assert!(!guard.is_empty());
    assert_eq!(Some(guard), std::env::var("WAYLAND_DISPLAY").ok());
    let helper = std::path::PathBuf::from(
        std::env::var_os("CABBY_TEST_AUDIO_HELPER").expect("fixture helper required"),
    );
    let leds = std::path::PathBuf::from(
        std::env::var_os("CABBY_TEST_LEDS").expect("fixture LEDs required"),
    );
    assert!(
        helper.is_absolute()
            && leds.is_absolute()
            && leds.starts_with(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../.artifacts")
                    .canonicalize()
                    .unwrap()
            )
    );
    surfaces::run_with_osd(
        prepare(Plugin::Desktop),
        cabby_audio::Client::with_executable(helper),
        leds,
    )
}

/// Disposable-session entrypoint with an explicitly supplied private BlueZ bus.
pub fn bluetooth_probe() -> iced_exwlshell::Result {
    let guard = std::env::var("CABBY_TEST_WAYLAND").expect("disposable session guard required");
    assert!(!guard.is_empty());
    assert_eq!(Some(guard), std::env::var("WAYLAND_DISPLAY").ok());
    let address = std::env::var("CABBY_TEST_BLUEZ_BUS").expect("private BlueZ bus required");
    assert!(address.starts_with("unix:path="));
    surfaces::run_with_bluetooth(
        prepare(Plugin::Desktop),
        cabby_bluetooth::Endpoint::Private(address),
    )
}

/// Private-session tray rendering and dismissal probe.
pub fn tray_probe() -> iced_exwlshell::Result {
    let guard = std::env::var("CABBY_TEST_WAYLAND").expect("disposable display required");
    assert!(!guard.is_empty());
    assert_eq!(Some(guard), std::env::var("WAYLAND_DISPLAY").ok());
    surfaces::run_with_tray(prepare(Plugin::Desktop))
}

/// Authenticated lock host exercised only on a guarded disposable compositor.
pub fn lock_probe() -> iced_exwlshell::Result {
    let guard = std::env::var("CABBY_TEST_WAYLAND").expect("disposable display required");
    assert!(!guard.is_empty());
    assert_eq!(Some(guard), std::env::var("WAYLAND_DISPLAY").ok());
    let helper = std::path::PathBuf::from(
        std::env::var_os("CABBY_TEST_AUTH_HELPER").expect("private helper required"),
    );
    assert!(helper.is_absolute());
    let mode = std::env::var("CABBY_TEST_LOCK_MODE").expect("probe mode required");
    assert!(["authenticate", "hold", "crash"].contains(&mode.as_str()));
    lock::run(
        prepare(Plugin::Lock),
        Some((cabby_lock::auth::Client::new(helper), mode)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_styling_cannot_prevent_lock_configuration() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        let source = Source::from_env(Some(&path)).unwrap();
        for content in ["invalid = true", "font_size = 0", "renderer = 42"] {
            std::fs::write(&path, content).unwrap();
            assert!(load_config(&source, Plugin::Desktop).is_err());
            assert_eq!(
                load_config(&source, Plugin::Lock).unwrap(),
                Config::default()
            );
        }
        std::fs::remove_file(path).unwrap();
        assert!(load_config(&source, Plugin::Desktop).is_err());
        assert_eq!(
            load_config(&source, Plugin::Lock).unwrap(),
            Config::default()
        );
    }
}
