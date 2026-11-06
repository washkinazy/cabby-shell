pub mod changes;
pub mod wallpaper;
pub mod watch;
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::{Path, PathBuf};

pub type Result<T> = std::result::Result<T, String>;
const MAX_CONFIG_BYTES: u64 = 64 * 1024;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Renderer {
    #[default]
    Vulkan,
    Software,
}

impl Renderer {
    pub fn environment(self) -> [(&'static str, &'static str); 2] {
        match self {
            Self::Vulkan => [("ICED_BACKEND", "wgpu"), ("WGPU_BACKEND", "vulkan")],
            Self::Software => [("ICED_BACKEND", "tiny-skia"), ("WGPU_BACKEND", "vulkan")],
        }
    }
}

/// Built-in bar contributions, in the order given by each region.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BarWidget {
    Workspaces,
    Clock,
    Audio,
    Network,
    Bluetooth,
    Tray,
    Apps,
    Title,
    Status,
    Idle,
    Nightlight,
    Caffeine,
    Clipboard,
    Screenshot,
}
pub const BAR_WIDGETS: &[BarWidget] = &[
    BarWidget::Workspaces,
    BarWidget::Clock,
    BarWidget::Audio,
    BarWidget::Network,
    BarWidget::Bluetooth,
    BarWidget::Tray,
    BarWidget::Apps,
    BarWidget::Title,
    BarWidget::Status,
    BarWidget::Idle,
    BarWidget::Nightlight,
    BarWidget::Caffeine,
    BarWidget::Clipboard,
    BarWidget::Screenshot,
];

// Field names for coverage are generated from the same declaration as the
// parser type. New fields cannot bypass documentation/example coverage.
macro_rules! configuration {
    ($($field:ident: $ty:ty = $default:expr),+ $(,)?) => {
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
        #[serde(default, deny_unknown_fields)]
        pub struct Config { $(pub $field: $ty),+ }
        impl Default for Config {
            fn default() -> Self { Self { $($field: $default),+ } }
        }
        pub const SETTING_KEYS: &[&str] = &[$(stringify!($field)),+];
    };
}

macro_rules! section {
    ($name:ident, $keys:ident, {$($field:ident: $ty:ty = $default:expr),+ $(,)?}) => {
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
        #[serde(default, deny_unknown_fields)]
        pub struct $name { $(pub $field: $ty),+ }
        impl Default for $name { fn default() -> Self { Self { $($field: $default),+ } } }
        pub const $keys: &[&str] = &[$(stringify!($field)),+];
    };
}
section!(Nightlight, NIGHTLIGHT_KEYS, {
    enabled: bool = false,
    force: bool = false,
    temperature_day: u16 = 6500,
    temperature_night: u16 = 4000,
});
section!(Location, LOCATION_KEYS, {
    auto_locate: bool = false,
    address: String = String::new(),
    custom_schedule: bool = false,
    sunset: String = String::new(),
    sunrise: String = String::new(),
    latitude: Option<f64> = None,
    longitude: Option<f64> = None,
});

section!(Screenshot, SCREENSHOT_KEYS, {
    directory: String = String::new(),
    include_cursor: bool = false,
});

section!(Clipboard, CLIPBOARD_KEYS, {
    enabled: bool = true,
    max_items: u16 = 50,
    max_item_bytes: u32 = 4_194_304,
    memory_bytes: u32 = 33_554_432,
});

section!(Caffeine, CAFFEINE_KEYS, {
    audio: bool = false,
    fullscreen: bool = false,
    fullscreen_requires_focus: bool = true,
    inhibit_idle: bool = true,
    inhibit_sleep: bool = true,
    manual_pause_nightlight: bool = false,
    fullscreen_pause_nightlight: bool = true,
});

configuration! {
    screenshot: Screenshot = Screenshot::default(),
    clipboard: Clipboard = Clipboard::default(),
    caffeine: Caffeine = Caffeine::default(),
    nightlight: Nightlight = Nightlight::default(),
    location: Location = Location::default(),
    lock_before_sleep: bool = false,
    idle_lock_seconds: u32 = 0,
    idle_display_off_seconds: u32 = 0,
    renderer: Renderer = Renderer::Vulkan,
    bar_height: u16 = 40,
    bar_left: Vec<BarWidget> = vec![BarWidget::Workspaces],
    bar_middle: Vec<BarWidget> = vec![BarWidget::Clock],
    bar_right: Vec<BarWidget> = vec![BarWidget::Audio, BarWidget::Network, BarWidget::Bluetooth, BarWidget::Caffeine, BarWidget::Nightlight, BarWidget::Tray],
    bar_position: BarPosition = BarPosition::Top,
    bar_padding: u16 = 8,
    bar_spacing: u16 = 8,
    font_size: u16 = 14,
    background: String = "#181b22".into(),
    foreground: String = "#e5e9f0".into(),
    accent: String = "#88c0d0".into(),
    show_window_title: bool = true,
    launcher_width: u16 = 640,
    launcher_rows: u16 = 8,
    launcher_terminal: Vec<String> = Vec::new(),
    wallpaper_image: String = String::new(),
    wallpaper_color: String = "#181b22".into(),
    wallpaper_mode: WallpaperMode = WallpaperMode::Fill,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BarPosition {
    #[default]
    Top,
    Bottom,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WallpaperMode {
    #[default]
    Fill,
    Fit,
    Stretch,
    Center,
}

/// Validated RGB color, independent of the UI toolkit.
pub fn rgb(value: &str) -> Option<[u8; 3]> {
    if value.len() != 7
        || !value.starts_with('#')
        || !value[1..].bytes().all(|b| b.is_ascii_hexdigit())
    {
        return None;
    }
    Some([
        u8::from_str_radix(&value[1..3], 16).ok()?,
        u8::from_str_radix(&value[3..5], 16).ok()?,
        u8::from_str_radix(&value[5..7], 16).ok()?,
    ])
}

pub fn clock_minutes(value: &str) -> Option<u16> {
    let b = value.as_bytes();
    if b.len() != 5 || b[2] != b':' || ![b[0], b[1], b[3], b[4]].iter().all(u8::is_ascii_digit) {
        return None;
    }
    let h = (b[0] - b'0') as u16 * 10 + (b[1] - b'0') as u16;
    let m = (b[3] - b'0') as u16 * 10 + (b[4] - b'0') as u16;
    (h < 24 && m < 60).then_some(h * 60 + m)
}
impl Config {
    pub fn parse(source: &str) -> Result<Self> {
        if source.len() > MAX_CONFIG_BYTES as usize {
            return Err("configuration exceeds 64 KiB".into());
        }
        let config: Self = toml::from_str(source).map_err(|error| error.to_string())?;
        if !(1000..=10000).contains(&config.nightlight.temperature_day)
            || !(1000..=10000).contains(&config.nightlight.temperature_night)
            || config.nightlight.temperature_day
                < config.nightlight.temperature_night.saturating_add(100)
        {
            return Err("nightlight temperatures must be 1000..10000 Kelvin and day must exceed night by at least 100".into());
        }
        if config.location.address.len() > 1024
            || config.location.address.chars().any(char::is_control)
        {
            return Err(
                "location.address must be at most 1024 UTF-8 bytes without control characters"
                    .into(),
            );
        }
        for (key, value) in [
            ("sunset", &config.location.sunset),
            ("sunrise", &config.location.sunrise),
        ] {
            if !value.is_empty() && clock_minutes(value).is_none() {
                return Err(format!("location.{key} must be empty or HH:MM"));
            }
        }
        if config.location.custom_schedule {
            let sunset = clock_minutes(&config.location.sunset)
                .ok_or("location.sunset must be HH:MM when custom_schedule is true")?;
            let sunrise = clock_minutes(&config.location.sunrise)
                .ok_or("location.sunrise must be HH:MM when custom_schedule is true")?;
            if sunrise == sunset {
                return Err("custom sunrise and sunset must differ".into());
            }
        }
        for (key, value, bound) in [
            ("latitude", config.location.latitude, 90.0),
            ("longitude", config.location.longitude, 180.0),
        ] {
            if value.is_some_and(|v| !v.is_finite() || v.abs() > bound) {
                return Err(format!(
                    "location.{key} must be finite and within -{bound}..{bound} degrees"
                ));
            }
        }
        if config.location.latitude.is_some() != config.location.longitude.is_some() {
            return Err("location requires both latitude and longitude".into());
        }
        if config.screenshot.directory.contains('\0') {
            return Err("screenshot.directory must not contain NUL".into());
        }
        if !(1..=200).contains(&config.clipboard.max_items)
            || !(65_536..=16_777_216).contains(&config.clipboard.max_item_bytes)
            || config.clipboard.memory_bytes < config.clipboard.max_item_bytes
            || config.clipboard.memory_bytes > 134_217_728
        {
            return Err("clipboard requires max_items=1..200, max_item_bytes=65536..16777216, memory_bytes>=max_item_bytes and <=134217728".into());
        }
        if config.idle_lock_seconds > 86400 {
            return Err("idle_lock_seconds must be 0..86400 seconds".into());
        }
        if config.idle_display_off_seconds > 86400
            || (config.idle_display_off_seconds > 0
                && (config.idle_lock_seconds == 0
                    || config.idle_display_off_seconds <= config.idle_lock_seconds))
        {
            return Err("idle_display_off_seconds must be 0 (disabled), or greater than idle_lock_seconds and at most 86400 seconds; locking must be enabled".into());
        }
        let mut widgets = std::collections::HashSet::new();
        for widget in config
            .bar_left
            .iter()
            .chain(&config.bar_middle)
            .chain(&config.bar_right)
        {
            if !widgets.insert(*widget) {
                return Err(
                    "bar widgets may appear only once across bar_left, bar_middle and bar_right"
                        .into(),
                );
            }
        }
        if !(32..=128).contains(&config.bar_height) {
            return Err("bar_height must be between 32 and 128 logical pixels".into());
        }
        if config.bar_padding > 32 || config.bar_spacing > 32 {
            return Err(
                "bar_padding and bar_spacing must be between 0 and 32 logical pixels".into(),
            );
        }
        if !(10..=32).contains(&config.font_size) || config.font_size + 8 > config.bar_height {
            return Err(
                "font_size must be 10..32 and at least 8 pixels smaller than bar_height".into(),
            );
        }
        for (key, value) in [
            ("background", &config.background),
            ("foreground", &config.foreground),
            ("accent", &config.accent),
            ("wallpaper_color", &config.wallpaper_color),
        ] {
            if rgb(value).is_none() {
                return Err(format!("{key} must be #RRGGBB"));
            }
        }
        if !(320..=1200).contains(&config.launcher_width)
            || !(4..=12).contains(&config.launcher_rows)
        {
            return Err("launcher_width must be 320..1200 pixels and launcher_rows 4..12".into());
        }
        if config.launcher_terminal.len() > 32
            || config
                .launcher_terminal
                .iter()
                .any(|s| s.is_empty() || s.contains('\0'))
        {
            return Err(
                "launcher_terminal must contain at most 32 nonempty, NUL-free arguments".into(),
            );
        }
        if config.wallpaper_image.contains('\0') {
            return Err("wallpaper_image cannot contain NUL".into());
        }
        Ok(config)
    }

    pub fn has_widget(&self, widget: BarWidget) -> bool {
        self.bar_left.contains(&widget)
            || self.bar_middle.contains(&widget)
            || self.bar_right.contains(&widget)
    }
    pub fn to_toml(&self) -> String {
        toml::to_string_pretty(self).expect("configuration contains only serializable fields")
    }
}

#[derive(Debug, Clone)]
pub struct Source {
    pub path: PathBuf,
    pub explicit: bool,
}

impl Source {
    pub fn resolve(explicit: Option<&Path>, home: Option<&Path>, cwd: &Path) -> Result<Self> {
        let path = if let Some(path) = explicit {
            if path.as_os_str().is_empty() {
                return Err("--config requires a nonempty path".into());
            }
            if path.is_absolute() {
                path.to_owned()
            } else {
                cwd.join(path)
            }
        } else {
            let home = home
                .filter(|path| path.is_absolute())
                .ok_or("HOME must be an absolute path when --config is omitted")?;
            home.join(".config/cabby-shell/config.toml")
        };
        Ok(Self {
            path,
            explicit: explicit.is_some(),
        })
    }

    pub fn from_env(explicit: Option<&Path>) -> Result<Self> {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let cwd = std::env::current_dir().map_err(|error| error.to_string())?;
        Self::resolve(explicit, home.as_deref(), &cwd)
    }

    pub fn load(&self, initial: bool) -> Result<Config> {
        let file = match std::fs::File::open(&self.path) {
            Ok(file) => file,
            Err(error)
                if initial && !self.explicit && error.kind() == std::io::ErrorKind::NotFound =>
            {
                return Ok(Config::default());
            }
            Err(error) => return Err(format!("{}: {error}", self.path.display())),
        };
        let mut text = String::new();
        file.take(MAX_CONFIG_BYTES + 1)
            .read_to_string(&mut text)
            .map_err(|error| format!("{}: {error}", self.path.display()))?;
        let mut config =
            Config::parse(&text).map_err(|error| format!("{}: {error}", self.path.display()))?;
        if !config.screenshot.directory.is_empty() {
            let path = Path::new(&config.screenshot.directory);
            if path.is_relative() {
                config.screenshot.directory = self
                    .path
                    .parent()
                    .ok_or("config has no parent directory")?
                    .join(path)
                    .to_str()
                    .ok_or("screenshot directory must be UTF-8")?
                    .to_owned();
            }
        }
        if !config.wallpaper_image.is_empty() {
            let path = Path::new(&config.wallpaper_image);
            if path.is_relative() {
                config.wallpaper_image = self
                    .path
                    .parent()
                    .ok_or("config has no parent directory")?
                    .join(path)
                    .to_str()
                    .ok_or("wallpaper path must be UTF-8")?
                    .to_owned();
            }
        }
        Ok(config)
    }
}

pub struct Store {
    pub source: Source,
    active: Config,
    asset_revision: Option<u64>,
}

impl Store {
    pub fn open(source: Source) -> Result<Self> {
        let active = source.load(true)?;
        let asset_revision = wallpaper::revision(&active)?;
        Ok(Self {
            source,
            active,
            asset_revision,
        })
    }

    pub fn active(&self) -> &Config {
        &self.active
    }

    /// Parse and validate the entire candidate before replacing the snapshot.
    pub fn reload(&mut self) -> Result<bool> {
        self.reload_with(|_| Ok(()))
    }

    pub fn reload_with(&mut self, publish: impl FnOnce(&Config) -> Result<()>) -> Result<bool> {
        let candidate = self.source.load(false)?;
        let asset_revision = wallpaper::revision(&candidate)?;
        let changed = candidate != self.active || asset_revision != self.asset_revision;
        if changed {
            publish(&candidate)?;
        }
        self.active = candidate;
        self.asset_revision = asset_revision;
        Ok(changed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bar_regions_default_order_custom_order_and_duplicates() {
        use BarWidget::*;
        let defaults = Config::parse("").unwrap();
        assert_eq!(defaults.bar_left, vec![Workspaces]);
        assert_eq!(defaults.bar_middle, vec![Clock]);
        assert_eq!(
            defaults.bar_right,
            vec![Audio, Network, Bluetooth, Caffeine, Nightlight, Tray]
        );
        let custom = Config::parse(
            "bar_left=['tray','audio']\nbar_middle=[]\nbar_right=['clock','workspaces']",
        )
        .unwrap();
        assert_eq!(custom.bar_left, vec![Tray, Audio]);
        assert!(!custom.has_widget(Network));
        assert_eq!(Config::parse(&custom.to_toml()).unwrap(), custom);
        for invalid in [
            "bar_left=['unknown']",
            "bar_left=['clock']",
            "bar_left=['apps','apps']",
            "bar_right='audio'",
        ] {
            assert!(Config::parse(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn invalid_candidates_never_replace_active_config() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        std::fs::write(&path, "bar_height = 64").unwrap();
        let mut store = Store::open(Source {
            path: path.clone(),
            explicit: true,
        })
        .unwrap();
        for candidate in [
            "bar_height = 0",
            "bar_height = 500",
            "renderer = 'unknown'",
            "typo = true",
            "wallpaper_color = '#xx1122'",
            "wallpaper_mode = 'tile'",
            "launcher_width = 0",
            "launcher_rows = 13",
            "launcher_terminal = ['']",
            "background = '#xyz123'",
            "accent = 'é00000'",
            "bar_position = 'left'",
            "bar_padding = 33",
            "bar_spacing = 33",
            "font_size = 33",
            "bar_height = 32\nfont_size = 32",
            "[",
            "bar_height = 40\nbar_height = 60",
        ] {
            std::fs::write(&path, candidate).unwrap();
            assert!(store.reload().is_err(), "{candidate}");
            assert_eq!(store.active().bar_height, 64);
        }
        std::fs::remove_file(&path).unwrap();
        assert!(store.reload().is_err());
        assert_eq!(store.active().bar_height, 64);
        std::fs::write(&path, "renderer = 'software'\nbar_height = 48").unwrap();
        assert!(
            store
                .reload_with(|_| Err("disk unavailable".into()))
                .is_err()
        );
        assert_eq!(store.active().bar_height, 64);
        assert!(store.reload().unwrap());
        assert_eq!(store.active().renderer, Renderer::Software);
        assert_eq!(store.active().bar_height, 48);
        assert!(!store.reload().unwrap());
    }

    #[test]
    fn paths_and_missing_files_have_explicit_semantics() {
        let temp = tempfile::tempdir().unwrap();
        let source = Source::resolve(None, Some(temp.path()), temp.path()).unwrap();
        assert_eq!(
            source.path,
            temp.path().join(".config/cabby-shell/config.toml")
        );
        assert_eq!(source.load(true).unwrap(), Config::default());
        assert!(source.load(false).is_err());
        let explicit = Source::resolve(Some(Path::new("missing.toml")), None, temp.path()).unwrap();
        assert_eq!(explicit.path, temp.path().join("missing.toml"));
        assert!(explicit.load(true).is_err());
        assert!(Source::resolve(None, None, temp.path()).is_err());
    }

    #[test]
    fn screenshot_paths_resolve_from_config_and_reject_nul() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("config.toml");
        std::fs::write(&path, "[screenshot]\ndirectory='captures'").unwrap();
        let config = Source {
            path,
            explicit: true,
        }
        .load(true)
        .unwrap();
        assert_eq!(
            Path::new(&config.screenshot.directory),
            root.path().join("captures")
        );
        assert!(Config::parse("[screenshot]\ndirectory=\"bad\\u0000path\"").is_err());
    }

    #[test]
    fn clipboard_limits_are_validated_even_when_disabled() {
        for settings in [
            "max_items=0",
            "max_items=201",
            "max_item_bytes=65535",
            "max_item_bytes=16777217",
            "memory_bytes=134217729",
            "max_item_bytes=1048576\nmemory_bytes=65536",
        ] {
            assert!(Config::parse(&format!("[clipboard]\nenabled=false\n{settings}")).is_err());
        }
        assert!(
            Config::parse(
                "[clipboard]\nmax_items=200\nmax_item_bytes=16777216\nmemory_bytes=16777216"
            )
            .is_ok()
        );
    }

    #[test]
    fn every_example_and_setting_has_reference_coverage() {
        assert!(Config::parse("idle_lock_seconds=86400").is_ok());
        assert!(Config::parse("idle_lock_seconds=10\nidle_display_off_seconds=11").is_ok());
        for config in [
            "idle_display_off_seconds=10",
            "idle_lock_seconds=10\nidle_display_off_seconds=10",
            "idle_lock_seconds=10\nidle_display_off_seconds=86401",
        ] {
            assert!(Config::parse(config).is_err());
        }
        assert!(Config::parse("idle_lock_seconds=86401").is_err());
        assert!(Config::parse("idle_lock_seconds=-1").is_err());
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let reference = std::fs::read_to_string(root.join("docs/configuration.md")).unwrap();
        let full = std::fs::read_to_string(root.join("examples/config/full.toml")).unwrap();
        let table: toml::Table = toml::from_str(&full).unwrap();
        assert_eq!(table.len(), SETTING_KEYS.len());
        for key in SETTING_KEYS {
            assert!(table.contains_key(*key), "missing full example: {key}");
            assert!(
                reference.contains(&format!("### `{key}`")),
                "missing reference: {key}"
            );
        }
        for (section, keys) in [
            ("nightlight", NIGHTLIGHT_KEYS),
            ("location", LOCATION_KEYS),
            ("caffeine", CAFFEINE_KEYS),
            ("clipboard", CLIPBOARD_KEYS),
            ("screenshot", SCREENSHOT_KEYS),
        ] {
            let values = table[section].as_table().unwrap();
            assert!(values.len() <= keys.len());
            for key in keys {
                assert!(
                    values.contains_key(*key)
                        || std::fs::read_dir(root.join("examples/config"))
                            .unwrap()
                            .filter_map(|e| e.ok())
                            .filter(|e| e.path().extension().is_some_and(|x| x == "toml"))
                            .any(|e| std::fs::read_to_string(e.path())
                                .ok()
                                .and_then(|s| toml::from_str::<toml::Table>(&s).ok())
                                .is_some_and(|t| t
                                    .get(section)
                                    .and_then(toml::Value::as_table)
                                    .is_some_and(|v| v.contains_key(*key)))),
                    "missing full nested example: {section}.{key}"
                );
                assert!(
                    reference.contains(&format!("### `{section}.{key}`")),
                    "missing nested reference: {section}.{key}"
                );
            }
        }
        let mut covered_widgets = std::collections::HashSet::new();
        let mut variants = Vec::new();
        let mut modes = Vec::new();
        for entry in std::fs::read_dir(root.join("examples/config")).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|ext| ext == "toml") {
                let config = Source {
                    path: path.clone(),
                    explicit: true,
                }
                .load(true)
                .unwrap();
                wallpaper::revision(&config).unwrap();
                covered_widgets.extend(
                    config
                        .bar_left
                        .iter()
                        .chain(&config.bar_middle)
                        .chain(&config.bar_right)
                        .copied(),
                );
                modes.push(config.wallpaper_mode);
                variants.push(
                    Config::parse(&std::fs::read_to_string(path).unwrap())
                        .unwrap()
                        .renderer,
                );
            }
        }
        for mode in [
            WallpaperMode::Fill,
            WallpaperMode::Fit,
            WallpaperMode::Stretch,
            WallpaperMode::Center,
        ] {
            assert!(modes.contains(&mode));
        }
        assert!(
            BAR_WIDGETS.iter().all(|w| covered_widgets.contains(w)),
            "every widget must appear in a runnable example"
        );
        assert!(variants.contains(&Renderer::Vulkan));
        assert!(variants.contains(&Renderer::Software));
        assert_eq!(
            Config::parse(&full).unwrap(),
            Config::default(),
            "full example must reflect defaults"
        );
    }
}
