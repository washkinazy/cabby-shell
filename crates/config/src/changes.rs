//! Typed configuration effects, independent of process or UI implementation.
use crate::Config;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Changes {
    pub renderer: bool,
    pub bar_geometry: bool,
    pub appearance: bool,
    pub wallpaper: bool,
}
impl Changes {
    pub fn between(old: &Config, new: &Config) -> Self {
        Self {
            renderer: old.renderer != new.renderer,
            bar_geometry: old.bar_position != new.bar_position || old.bar_height != new.bar_height,
            appearance: old.font_size != new.font_size
                || old.background != new.background
                || old.foreground != new.foreground
                || old.accent != new.accent,
            wallpaper: old.wallpaper_image != new.wallpaper_image
                || old.wallpaper_color != new.wallpaper_color
                || old.wallpaper_mode != new.wallpaper_mode,
        }
    }
}
