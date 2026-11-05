//! Shared startup for repository-owned features.
//!
//! Each existing executable selects one built-in plugin. This registry defines
//! code ownership, not process isolation or a dynamic loading interface.
use crate::ui;
mod launcher;
mod notifications;
mod surfaces;
use cabby_config::{Config, Source};

/// The built-in features currently migrated to the shared host.
#[derive(Clone, Copy)]
pub enum Plugin {
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

/// Validate inputs and initialize shared process state before backend tasks.
fn prepare(plugin: Plugin) -> Context {
    let context = ui::configuration_source()
        .and_then(|source| {
            let config = source.load(true)?;
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
        Plugin::Bar => surfaces::run(context, true, false),
        Plugin::Launcher => launcher::run(context),
        Plugin::Wallpaper => surfaces::run(context, false, true),
        Plugin::Notifications => notifications::run(context),
    }
}

/// Exercise shared bar/background hosting only in a guarded disposable session.
/// This is test infrastructure, not a production CLI or service mode.
pub fn composition_probe() -> iced_exwlshell::Result {
    let guard = std::env::var("CABBY_TEST_WAYLAND").expect("disposable session guard required");
    assert!(!guard.is_empty());
    assert_eq!(Some(guard), std::env::var("WAYLAND_DISPLAY").ok());
    surfaces::run(prepare(Plugin::Bar), true, true)
}
