# Application launcher

Open the launcher with the bar's **Apps** button or `cabby launcher`. The command requires an active Cabby target and niri service, and starts the on-demand `cabby-launcher.service`. Repeating it while the launcher is open preserves that instance. Nothing is installed or enabled by building; packaged installation remains future work.

For an installed setup, a niri binding can use:

```kdl
binds {
    Mod+D { spawn "cabby" "launcher"; }
}
```

Type to search. Up/Down selects a result and crosses pages, Enter launches it, and Escape or Close dismisses the overlay. Clicking a result launches it. The overlay opens on the compositor's active output, temporarily requests exclusive keyboard input, and releases it when closed. It does not reserve desktop space. The bar keeps its noninteractive keyboard policy.

Names, generic names, keywords, and desktop IDs are matched case-insensitively. All space-separated query terms must match. Exact names rank first, then name prefixes, name substrings, and other matches; ties use name/desktop-ID order. This is substring search, not fuzzy character matching. No usage ranking, persistent history, or query logging is implemented. Discovery refreshes on every opening. Search is limited to 256 characters.

## Application discovery and execution

Cabby searches `applications/` beneath `$XDG_DATA_HOME` (default `$HOME/.local/share`), then `$XDG_DATA_DIRS` (default `/usr/local/share:/usr/share`) in priority order. These are application metadata paths, separate from the deliberately fixed Cabby config root. Relative XDG data paths are ignored. Higher-priority desktop IDs override lower-priority IDs, including hidden or malformed overrides; nested paths use hyphens in their desktop IDs. Directory symlinks below roots are not followed; file symlinks such as Flatpak exports are supported. Export directories must be present in XDG_DATA_DIRS.

Only `Type=Application` entries with a name and supported `Exec` are included. `Hidden`, `NoDisplay`, `OnlyShowIn`, `NotShowIn`, and `TryExec` are respected. Visibility uses XDG_CURRENT_DESKTOP (defaults to niri when absent). Localized names use LC_ALL, LC_MESSAGES, then LANG, with a language fallback. Gettext catalogs are not loaded. Scanning is bounded to 20,000 directory entries, 16 nested levels, and 256 KiB per desktop file; unreadable or malformed entries are skipped.

Desktop-entry decoding uses the Rust `freedesktop-desktop-entry` crate. Cabby's argument planner implements the [Desktop Entry Exec rules](https://specifications.freedesktop.org/desktop-entry/latest/exec-variables.html) needed for application launches: quoted arguments, literal percent, `%c` name, `%k` desktop-file location, and `%i` as `--icon` plus icon. No files/URLs are supplied, so `%f`, `%F`, `%u`, and `%U` are removed. Deprecated codes are removed; unknown or embedded field codes are rejected. Quoted field codes are accepted as complete tokens, though the specification leaves that case undefined. Shell variables, globbing, and command substitution are never expanded by Cabby. A desktop entry can explicitly choose a shell executable, just like any other program.

The desktop `Path` becomes the application's working directory. `Terminal=true` requires `launcher_terminal`, a literal command prefix such as `["foot", "-e"]`. Its default is empty: terminal applications remain visible but selection reports how to configure them. The terminal is an optional user dependency, not a required build package. See the [configuration reference](configuration.md) for width, result rows, terminal prefix, and shared theme settings.

`cabby launch-app --desktop-id ID [--config PATH] [--session-unit UNIT]` is the CLI equivalent of selecting a result. It rediscovers the entry and validates current configuration before launch. IDs include `.desktop`; they are IDs, not arbitrary paths. `DBusActivatable` entries use their Exec fallback; entries without Exec are omitted. D-Bus activation, desktop actions, file/URL launching, custom GPU selection, startup-notification tokens, icon rendering, and shell-command search are outside this increment.

The selected command goes through `cabby launch`'s implementation and runs in a separate compositor-bound `app-cabby-*.service`. It survives ordinary launcher/shell shutdown. Apps that delegate to an existing process retain that process's ownership. Only one selection may be in flight; failures keep the overlay open with an error. Success means execution started, not that the application finished opening its window. Escape during launch closes the overlay but does not promise to cancel an already accepted systemd job. After a timeout, check whether the app opened before retrying.

## Development and evidence

`cabby launcher-ui --config PATH` runs the UI in the foreground for a supervised disposable-session test. Normal usage is `cabby launcher` through systemd. Development unit overrides `--launcher-unit`, `--target`, and `--session-unit` select existing staged units; they do not rewrite dependencies.

Rust fixtures cover discovery precedence, visibility/localization, symlink handling, argument boundaries/escaping, terminal prefixes, search ranking, selection wrapping, duplicate submission, and captured Escape. The lifecycle harness verifies the launcher is on demand, repeated opening is idempotent, shutdown closes it, and a real desktop-entry launch with a working directory survives a shell restart and stops on compositor loss. Graphical tests map the launcher in nested niri, observe keyboard focus and buffer attachment, capture its layout, and verify selected hardware Vulkan separately from software rendering. The GUI test does not activate installed applications. Actual typing/click delivery, focus restoration, and physical multi-output placement remain unverified; manual acceptance is not required.
