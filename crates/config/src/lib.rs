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

// Field names for coverage are generated from the same declaration as the
// parser type. New fields cannot bypass documentation/example coverage.
macro_rules! configuration {
    ($($field:ident: $ty:ty = $default:expr),+ $(,)?) => {
        #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(default, deny_unknown_fields)]
        pub struct Config { $(pub $field: $ty),+ }
        impl Default for Config {
            fn default() -> Self { Self { $($field: $default),+ } }
        }
        pub const SETTING_KEYS: &[&str] = &[$(stringify!($field)),+];
    };
}

configuration! {
    renderer: Renderer = Renderer::Vulkan,
    bar_height: u16 = 40,
    bar_position: BarPosition = BarPosition::Top,
    bar_padding: u16 = 8,
    bar_spacing: u16 = 8,
    font_size: u16 = 14,
    background: String = "#181b22".into(),
    foreground: String = "#e5e9f0".into(),
    accent: String = "#88c0d0".into(),
    show_window_title: bool = true,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BarPosition {
    #[default]
    Top,
    Bottom,
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

impl Config {
    pub fn parse(source: &str) -> Result<Self> {
        if source.len() > MAX_CONFIG_BYTES as usize {
            return Err("configuration exceeds 64 KiB".into());
        }
        let config: Self = toml::from_str(source).map_err(|error| error.to_string())?;
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
        ] {
            if rgb(value).is_none() {
                return Err(format!("{key} must be #RRGGBB"));
            }
        }
        Ok(config)
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
        Config::parse(&text).map_err(|error| format!("{}: {error}", self.path.display()))
    }
}

pub struct Store {
    pub source: Source,
    active: Config,
}

impl Store {
    pub fn open(source: Source) -> Result<Self> {
        let active = source.load(true)?;
        Ok(Self { source, active })
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
        let changed = candidate != self.active;
        if changed {
            publish(&candidate)?;
        }
        self.active = candidate;
        Ok(changed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn every_example_and_setting_has_reference_coverage() {
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
        let mut variants = Vec::new();
        for entry in std::fs::read_dir(root.join("examples/config")).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|ext| ext == "toml") {
                variants.push(
                    Config::parse(&std::fs::read_to_string(path).unwrap())
                        .unwrap()
                        .renderer,
                );
            }
        }
        assert!(variants.contains(&Renderer::Vulkan));
        assert!(variants.contains(&Renderer::Software));
        assert_eq!(
            Config::parse(&full).unwrap(),
            Config::default(),
            "full example must reflect defaults"
        );
    }
}
