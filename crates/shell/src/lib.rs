//! Built-in shell features and shared host machinery.
struct RendererLog;

impl log::Log for RendererLog {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.target().starts_with("wgpu") || metadata.target().starts_with("iced_wgpu")
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            eprintln!("{} {}: {}", record.level(), record.target(), record.args());
        }
    }

    fn flush(&self) {}
}

pub fn renderer_diagnostics() {
    if std::env::var_os("CABBY_RENDERER_DIAGNOSTICS").is_some() {
        static LOGGER: RendererLog = RendererLog;
        log::set_logger(&LOGGER).expect("probe logger initialized once");
        log::set_max_level(log::LevelFilter::Debug);
    }
}

pub mod niri;

pub mod ui;

pub mod host;
mod plugins;

mod network;

mod icons;

mod runtime_status;
