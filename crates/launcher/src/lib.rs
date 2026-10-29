//! Application discovery, argument planning, and selection; no UI or shell execution.
use freedesktop_desktop_entry::DesktopEntry;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

type Result<T> = std::result::Result<T, String>;
#[derive(Debug, Clone)]
pub struct Application {
    pub id: String,
    pub name: String,
    pub search: String,
    pub argv: Vec<String>,
    pub directory: Option<PathBuf>,
    pub terminal: bool,
}
impl Application {
    pub fn command(&self, terminal: &[String]) -> Result<Vec<String>> {
        if self.terminal && terminal.is_empty() {
            return Err("Set launcher_terminal in config to open terminal applications".into());
        }
        let mut args = if self.terminal {
            terminal.to_vec()
        } else {
            Vec::new()
        };
        args.extend(self.argv.clone());
        Ok(args)
    }
}
#[derive(Debug, Clone, Default)]
pub struct Catalog {
    pub apps: Vec<Application>,
    pub skipped: usize,
}

pub fn data_paths() -> Vec<PathBuf> {
    let home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
                .map(|p| p.join(".local/share"))
        });
    let dirs = std::env::var_os("XDG_DATA_DIRS")
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    home.into_iter()
        .chain(std::env::split_paths(&dirs).filter(|p| p.is_absolute()))
        .map(|p| p.join("applications"))
        .collect()
}
pub fn locales() -> Vec<String> {
    let locale = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .find_map(|key| std::env::var(key).ok().filter(|v| !v.is_empty()))
        .unwrap_or_default();
    let base = locale.split('.').next().unwrap_or("").to_owned();
    let language = base.split(['_', '@']).next().unwrap_or("").to_owned();
    vec![base, language]
}
pub fn discover() -> Catalog {
    let desktops = std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_else(|_| "niri".into())
        .split(':')
        .map(str::to_owned)
        .collect::<Vec<_>>();
    scan(
        &data_paths(),
        &locales(),
        &desktops,
        &std::env::var_os("PATH")
            .map(|v| std::env::split_paths(&v).collect::<Vec<_>>())
            .unwrap_or_default(),
    )
}
fn executable(value: &str, paths: &[PathBuf]) -> bool {
    let valid = |p: &Path| {
        p.metadata()
            .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    };
    if value.contains('/') {
        Path::new(value).is_absolute() && valid(Path::new(value))
    } else {
        paths.iter().any(|p| valid(&p.join(value)))
    }
}
fn walk(
    root: &Path,
    directory: &Path,
    depth: usize,
    found: &mut BTreeMap<String, PathBuf>,
    budget: &mut usize,
) {
    if depth > 16 || *budget == 0 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    let mut entries: Vec<_> = entries
        .filter_map(std::result::Result::ok)
        .take(*budget)
        .collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        if *budget == 0 {
            return;
        }
        *budget -= 1;
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            walk(root, &path, depth + 1, found, budget);
        } else if path.extension().is_some_and(|s| s == "desktop")
            && path.is_file()
            && let Ok(relative) = path.strip_prefix(root)
            && let Some(id) = relative.to_str()
        {
            found.entry(id.replace('/', "-")).or_insert(path);
        }
    }
}
pub fn scan(
    roots: &[PathBuf],
    locales: &[String],
    desktops: &[String],
    paths: &[PathBuf],
) -> Catalog {
    let mut files = BTreeMap::new();
    let mut budget = 20000;
    for root in roots {
        walk(root, root, 0, &mut files, &mut budget);
    }
    let mut catalog = Catalog::default();
    for (id, path) in files {
        let parsed = (|| {
            let mut source = String::new();
            std::fs::File::open(&path)
                .ok()?
                .take(262145)
                .read_to_string(&mut source)
                .ok()?;
            if source.len() > 262144 {
                return None;
            }
            let entry = DesktopEntry::from_str(&path, &source, Some(locales)).ok()?;
            if entry.hidden() || entry.no_display() || entry.type_() != Some("Application") {
                return None;
            }
            if entry
                .only_show_in()
                .is_some_and(|v| !v.iter().any(|d| desktops.iter().any(|s| s == d)))
                || entry
                    .not_show_in()
                    .is_some_and(|v| v.iter().any(|d| desktops.iter().any(|s| s == d)))
            {
                return None;
            }
            if entry.try_exec().is_some_and(|v| !executable(v, paths)) {
                return None;
            }
            let name = entry.name(locales)?.into_owned();
            if name.is_empty() {
                return None;
            }
            let argv = exec_arguments(entry.exec()?, &name, entry.icon(), &path).ok()?;
            let directory = entry.path().filter(|v| !v.is_empty()).map(PathBuf::from);
            if directory.as_ref().is_some_and(|p| !p.is_absolute()) {
                return None;
            }
            let keywords = entry
                .keywords(locales)
                .unwrap_or_default()
                .iter()
                .map(|v| v.as_ref())
                .collect::<Vec<_>>()
                .join(" ");
            let search = format!(
                "{} {} {} {}",
                name,
                entry.generic_name(locales).unwrap_or_default(),
                keywords,
                id
            )
            .to_lowercase();
            Some(Application {
                id,
                name,
                search,
                argv,
                directory,
                terminal: entry.terminal(),
            })
        })();
        match parsed {
            Some(app) => catalog.apps.push(app),
            None => catalog.skipped += 1,
        }
    }
    catalog
        .apps
        .sort_by(|a, b| (a.name.to_lowercase(), &a.id).cmp(&(b.name.to_lowercase(), &b.id)));
    catalog
}

/// Tokenize the already-decoded Exec string. No shell, glob, variable, or command expansion.
/// Field codes that introduce multiple arguments must occupy a complete token.
pub fn exec_arguments(
    exec: &str,
    name: &str,
    icon: Option<&str>,
    path: &Path,
) -> Result<Vec<String>> {
    let mut tokens = Vec::new();
    let mut token = String::new();
    let mut quoted = false;
    let mut started = false;
    let mut chars = exec.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                quoted = !quoted;
                started = true;
            }
            '\\' => {
                let next = chars.next().ok_or("dangling escape")?;
                if quoted && !matches!(next, '"' | '`' | '$' | '\\') {
                    token.push('\\');
                }
                token.push(next);
                started = true;
            }
            c if c.is_ascii_whitespace() && !quoted => {
                if started {
                    tokens.push(std::mem::take(&mut token));
                    started = false;
                }
            }
            c => {
                if c == '\0' {
                    return Err("NUL in Exec".into());
                }
                token.push(c);
                started = true;
            }
        }
    }
    if quoted {
        return Err("unclosed Exec quote".into());
    }
    if started {
        tokens.push(token);
    }
    let mut args = Vec::new();
    for token in tokens {
        match token.as_str() {
            "%f" | "%F" | "%u" | "%U" | "%d" | "%D" | "%n" | "%N" | "%v" | "%m" => (),
            "%i" => {
                if let Some(icon) = icon {
                    args.extend(["--icon".into(), icon.into()]);
                }
            }
            "%c" => args.push(name.into()),
            "%k" => args.push(path.to_string_lossy().into_owned()),
            _ => {
                let mut value = String::new();
                let mut chars = token.chars();
                while let Some(c) = chars.next() {
                    if c == '%' {
                        if chars.next() != Some('%') {
                            return Err("unsupported or embedded Exec field code".into());
                        }
                        value.push('%');
                    } else {
                        value.push(c);
                    }
                }
                args.push(value);
            }
        }
    }
    if args
        .first()
        .is_none_or(|v| v.is_empty() || v.contains('=') || v.starts_with('-'))
    {
        return Err("missing or invalid executable".into());
    }
    Ok(args)
}

#[derive(Debug, Clone, Default)]
pub struct Picker {
    pub catalog: Catalog,
    pub query: String,
    pub matches: Vec<usize>,
    pub selected: usize,
    pub pending: bool,
    pub error: Option<String>,
}
impl Picker {
    pub fn loaded(catalog: Catalog) -> Self {
        let mut picker = Self {
            catalog,
            ..Default::default()
        };
        picker.filter(String::new());
        picker
    }
    pub fn filter(&mut self, query: String) {
        if self.pending {
            return;
        }
        self.query = query.chars().take(256).collect();
        self.selected = 0;
        self.error = None;
        let query = self.query.trim().to_lowercase();
        let tokens: BTreeSet<_> = query.split_whitespace().collect();
        let mut found: Vec<_> = self
            .catalog
            .apps
            .iter()
            .enumerate()
            .filter(|(_, a)| tokens.iter().all(|t| a.search.contains(t)))
            .map(|(i, a)| {
                let name = a.name.to_lowercase();
                let rank = if name == query {
                    0
                } else if name.starts_with(&query) {
                    1
                } else if name.contains(&query) {
                    2
                } else {
                    3
                };
                (rank, i)
            })
            .collect();
        found.sort();
        self.matches = found.into_iter().map(|(_, i)| i).collect();
    }
    pub fn step(&mut self, backwards: bool) {
        if !self.pending && !self.matches.is_empty() {
            self.selected = if backwards {
                (self.selected + self.matches.len() - 1) % self.matches.len()
            } else {
                (self.selected + 1) % self.matches.len()
            };
        }
    }
    pub fn begin(&mut self) -> Option<Application> {
        if self.pending {
            return None;
        }
        let app = self
            .catalog
            .apps
            .get(*self.matches.get(self.selected)?)?
            .clone();
        self.pending = true;
        self.error = None;
        Some(app)
    }
    pub fn failed(&mut self, error: String) {
        self.pending = false;
        self.error = Some(error);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn write(root: &Path, id: &str, body: &str) {
        let path = root.join(id);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, format!("[Desktop Entry]\nType=Application\n{body}\n")).unwrap();
    }
    fn scan_test(roots: &[PathBuf]) -> Catalog {
        scan(
            roots,
            &["fr".into()],
            &["niri".into()],
            &["/usr/bin".into()],
        )
    }
    #[test]
    fn user_overrides_hidden_and_nested_ids_mask_system_entries() {
        let temp = tempfile::tempdir().unwrap();
        let user = temp.path().join("user");
        let system = temp.path().join("system");
        write(&system, "hidden.desktop", "Name=System Hidden\nExec=true");
        write(&user, "hidden.desktop", "Hidden=true");
        write(&system, "nested-app.desktop", "Name=System App\nExec=false");
        write(&user, "nested/app.desktop", "Name=User App\nExec=true");
        let catalog = scan_test(&[user, system]);
        assert_eq!(catalog.apps.len(), 1);
        assert_eq!(catalog.apps[0].id, "nested-app.desktop");
        assert_eq!(catalog.apps[0].name, "User App");
    }
    #[test]
    fn discovery_filters_and_localizes_and_accepts_file_symlinks() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("applications");
        write(
            &root,
            "app.desktop",
            "Name=English\nName[fr]=Français\nGenericName=Editor\nKeywords=write;notes;\nExec=/usr/bin/true\nOnlyShowIn=niri;",
        );
        write(
            &root,
            "hidden.desktop",
            "Name=Hidden\nExec=true\nNoDisplay=true",
        );
        write(
            &root,
            "other.desktop",
            "Name=Other\nExec=true\nOnlyShowIn=GNOME;",
        );
        write(
            &root,
            "excluded.desktop",
            "Name=Excluded\nExec=true\nNotShowIn=niri;",
        );
        write(
            &root,
            "missing.desktop",
            "Name=Missing\nExec=true\nTryExec=cabby-test-nonexistent",
        );
        write(
            &root,
            "dbus.desktop",
            "Name=D-Bus only\nDBusActivatable=true",
        );
        write(&root, "bad.desktop", "Name=Bad\nExec=true %z");
        std::os::unix::fs::symlink(root.join("app.desktop"), root.join("link.desktop")).unwrap();
        std::os::unix::fs::symlink(&root, root.join("cycle")).unwrap();
        let catalog = scan_test(&[root]);
        assert_eq!(catalog.apps.len(), 2);
        assert_eq!(catalog.apps[0].name, "Français");
        assert!(catalog.apps[0].search.contains("notes"));
    }
    #[test]
    fn exec_codes_quotes_and_shell_metacharacters_are_literal_arguments() {
        let path = Path::new("/tmp/example.desktop");
        assert_eq!(
            exec_arguments(
                r#"/usr/bin/app "two words" "" "$HOME" "$(touch no)" %% %f %F %u %U %i %c %k"#,
                "App Name",
                Some("app-icon"),
                path
            )
            .unwrap(),
            vec![
                "/usr/bin/app",
                "two words",
                "",
                "$HOME",
                "$(touch no)",
                "%",
                "--icon",
                "app-icon",
                "App Name",
                "/tmp/example.desktop"
            ]
        );
        for source in [
            "",
            "\"unclosed",
            "prog %z",
            "prog prefix%F",
            "A=B",
            "--help",
            "prog trailing\\",
        ] {
            assert!(
                exec_arguments(source, "App", None, path).is_err(),
                "{source}"
            );
        }
    }
    #[test]
    fn terminal_prefix_and_working_directory_are_preserved() {
        let temp = tempfile::tempdir().unwrap();
        write(
            temp.path(),
            "term.desktop",
            "Name=Terminal App\nExec=editor \"two words\"\nTerminal=true\nPath=/tmp",
        );
        let catalog = scan_test(&[temp.path().to_owned()]);
        let app = &catalog.apps[0];
        assert!(app.command(&[]).is_err());
        assert_eq!(
            app.command(&["foot".into(), "-e".into()]).unwrap(),
            vec!["foot", "-e", "editor", "two words"]
        );
        assert_eq!(app.directory, Some(PathBuf::from("/tmp")));
    }
    #[test]
    fn ranking_navigation_and_pending_launch_policy() {
        let temp = tempfile::tempdir().unwrap();
        write(
            temp.path(),
            "alpha.desktop",
            "Name=Alpha Notes\nExec=true\nKeywords=editor;",
        );
        write(temp.path(), "notes.desktop", "Name=Notes\nExec=true");
        write(
            temp.path(),
            "unicode.desktop",
            "Name=Éditeur\nExec=true\nKeywords=editor;",
        );
        let mut picker = Picker::loaded(scan_test(&[temp.path().to_owned()]));
        picker.filter("notes".into());
        assert_eq!(picker.catalog.apps[picker.matches[0]].name, "Notes");
        picker.step(true);
        assert_eq!(picker.selected, 1);
        picker.step(false);
        assert_eq!(picker.selected, 0);
        assert!(picker.begin().is_some());
        assert!(picker.begin().is_none());
        picker.filter("different".into());
        picker.step(true);
        assert_eq!(picker.query, "notes");
        assert_eq!(picker.selected, 0);
        picker.failed("launch failed".into());
        assert!(!picker.pending);
        picker.filter("éDIt".into());
        assert_eq!(picker.matches.len(), 1);
        picker.filter("alpha editor".into());
        assert_eq!(picker.matches.len(), 1);
        picker.filter("no match".into());
        picker.step(true);
        assert!(picker.begin().is_none());
        assert_eq!(picker.selected, 0);
    }
    #[test]
    fn decoding_exec_backslashes_and_malformed_override() {
        let temp = tempfile::tempdir().unwrap();
        let user = temp.path().join("user");
        let system = temp.path().join("system");
        write(
            &user,
            "escape.desktop",
            r#"Name=Escaped
Exec=app "a\\\\b" "\\"quoted\\"""#,
        );
        write(&system, "bad.desktop", "Name=Fallback\nExec=true");
        write(&user, "bad.desktop", "Name=Broken\nExec=\"unterminated");
        let catalog = scan_test(&[user, system]);
        assert_eq!(catalog.apps.len(), 1);
        assert_eq!(catalog.apps[0].argv, vec!["app", r"a\b", "\"quoted\""]);
    }
}
