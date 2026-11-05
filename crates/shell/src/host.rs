//! Shared startup for repository-owned features.
//!
//! Each existing executable selects one built-in plugin. This registry defines
//! code ownership, not process isolation or a dynamic loading interface.
use crate::{plugins, ui};
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

/// Run one feature in the caller's supervised process, on its main thread.
pub fn run(plugin: Plugin) -> iced_exwlshell::Result {
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
    match plugin {
        Plugin::Bar => plugins::bar::run(context),
        Plugin::Launcher => plugins::launcher::run(context),
        Plugin::Wallpaper => plugins::wallpaper::run(context),
        Plugin::Notifications => plugins::notifications::run(context),
    }
}
