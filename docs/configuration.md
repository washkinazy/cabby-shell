# Configuration reference

## Location and loading

Cabby reads TOML from `$HOME/.config/cabby-shell/config.toml`. This is currently one file; there are no includes, automatic subfile discovery, or layered merges. `$XDG_CONFIG_HOME` does not override this explicitly chosen default. `HOME` must be absolute when no explicit path is supplied.

`cabby validate --config PATH` and `cabby-platform-spike --config PATH` select a different file. Relative CLI paths resolve against the command's working directory. Cabby does not expand `~` or environment variables inside path strings; a shell may expand an unquoted CLI path before invoking Cabby.

A missing default file at startup uses built-in defaults. A missing explicitly selected file is an error. Empty files and omitted settings use defaults. Invalid startup configuration fails startup. Invalid reloads (including a removed file) retain the entire previous valid snapshot. Unknown/duplicate keys, wrong types, unknown renderer names, and invalid ranges are errors. Files are limited to 64 KiB and must be UTF-8.

The coordinator implements `cabby reload`: invalid input retains the previous active snapshot; changed valid input is published atomically and queues a managed UI restart. Bar settings require that restart; launcher settings take effect on its next opening. Direct UI invocation reads settings on startup only. See [services.md](services.md) for runtime ownership and restart results.

## Validation and installation

```sh
cargo run -p cabby -- validate --config examples/config/full.toml
cargo run -p cabby -- validate --config examples/config/minimal.toml
cargo run -p cabby -- validate --config examples/config/software.toml
```

Create `$HOME/.config/cabby-shell` and copy one example to `config.toml` only if you do not already have a configuration. Installation and upgrades never overwrite user configuration. Runtime snapshots, logs, caches, clipboard history, and sockets are separate from user preferences.

## Settings in `config.toml`

### `renderer`

- **Purpose:** selects the renderer for Cabby's UI surfaces.
- **Type/default:** string; `"vulkan"` when omitted.
- **Values:** `"vulkan"` or `"software"`, case-sensitive; no units.
- **Example:** `renderer = "vulkan"` in [full.toml](../examples/config/full.toml); the software alternative is [software.toml](../examples/config/software.toml).
- **Dependencies/interactions:** Vulkan requires a compatible Vulkan driver/adapter. Software selects Iced tiny-skia and is not proof of hardware acceleration. The UI process explicitly selects its backend, overriding inherited `ICED_BACKEND`/`WGPU_BACKEND` values without changing the user's session environment. System services and screenshot helpers do not use this renderer.
- **Application:** UI restart required. No silent fallback when Vulkan is selected.
- **Migration:** first schema; no prior setting to migrate.

### `bar_height`

- **Purpose:** sets the bar's logical height and matching reserved screen area.
- **Type/default:** integer; `40` when omitted.
- **Values:** inclusive range `32` through `128`, in logical pixels. Fractional output scaling determines physical pixel size.
- **Example:** `bar_height = 40` in [full.toml](../examples/config/full.toml).
- **Dependencies/interactions:** applies to the bar on each output, independently of renderer choice. It does not configure monitor resolution or scale.
- **Application:** UI restart required.
- **Migration:** first schema; no prior setting to migrate.

### `bar_position`

- **Purpose:** Which output edge holds the bar and its reserved area.
- **Type/default:** string; `"top"` when omitted.
- **Values:** "top" or "bottom".
- **Example:** `bar_position = "top"` in [full.toml](../examples/config/full.toml).
- **Dependencies/interactions:** applies to every bar output and both renderers; layout stays workspace buttons followed by active-window title and connection/action status. Long workspace lists scroll horizontally; titles are limited to 100 characters and control characters are removed.
- **Application:** UI restart required; managed `cabby reload` queues it.
- **Migration:** new optional setting; existing configurations retain defaults.

### `bar_padding`

- **Purpose:** Horizontal padding around bar contents.
- **Type/default:** integer; `8` when omitted.
- **Values:** 0..32 logical pixels.
- **Example:** `bar_padding = 8` in [full.toml](../examples/config/full.toml).
- **Dependencies/interactions:** applies to every bar output and both renderers; layout stays workspace buttons followed by active-window title and connection/action status. Long workspace lists scroll horizontally; titles are limited to 100 characters and control characters are removed.
- **Application:** UI restart required; managed `cabby reload` queues it.
- **Migration:** new optional setting; existing configurations retain defaults.

### `bar_spacing`

- **Purpose:** Gap between workspace buttons and bar sections.
- **Type/default:** integer; `8` when omitted.
- **Values:** 0..32 logical pixels.
- **Example:** `bar_spacing = 8` in [full.toml](../examples/config/full.toml).
- **Dependencies/interactions:** applies to every bar output and both renderers; layout stays workspace buttons followed by active-window title and connection/action status. Long workspace lists scroll horizontally; titles are limited to 100 characters and control characters are removed.
- **Application:** UI restart required; managed `cabby reload` queues it.
- **Migration:** new optional setting; existing configurations retain defaults.

### `font_size`

- **Purpose:** Shared text size. Uses the system sans-serif font.
- **Type/default:** integer; `14` when omitted.
- **Values:** 10..32 logical pixels; at least 8 smaller than bar_height.
- **Example:** `font_size = 14` in [full.toml](../examples/config/full.toml).
- **Dependencies/interactions:** applies to every bar output and both renderers; layout stays workspace buttons followed by active-window title and connection/action status. Long workspace lists scroll horizontally; titles are limited to 100 characters and control characters are removed.
- **Application:** UI restart required; managed `cabby reload` queues it.
- **Migration:** new optional setting; existing configurations retain defaults.

### `background`

- **Purpose:** Shared bar and launcher background color.
- **Type/default:** string; `"#181b22"` when omitted.
- **Values:** Opaque #RRGGBB hexadecimal RGB, case-insensitive digits.
- **Example:** `background = "#181b22"` in [full.toml](../examples/config/full.toml).
- **Dependencies/interactions:** applies to every bar output and both renderers; layout stays workspace buttons followed by active-window title and connection/action status. Long workspace lists scroll horizontally; titles are limited to 100 characters and control characters are removed.
- **Application:** UI restart required; managed `cabby reload` queues it.
- **Migration:** new optional setting; existing configurations retain defaults.

### `foreground`

- **Purpose:** Shared bar and launcher normal text color.
- **Type/default:** string; `"#e5e9f0"` when omitted.
- **Values:** Opaque #RRGGBB hexadecimal RGB, case-insensitive digits.
- **Example:** `foreground = "#e5e9f0"` in [full.toml](../examples/config/full.toml).
- **Dependencies/interactions:** applies to every bar output and both renderers; layout stays workspace buttons followed by active-window title and connection/action status. Long workspace lists scroll horizontally; titles are limited to 100 characters and control characters are removed.
- **Application:** UI restart required; managed `cabby reload` queues it.
- **Migration:** new optional setting; existing configurations retain defaults.

### `accent`

- **Purpose:** Active workspace and selected launcher-result color. Iced derives contrasting button text.
- **Type/default:** string; `"#88c0d0"` when omitted.
- **Values:** Opaque #RRGGBB hexadecimal RGB, case-insensitive digits.
- **Example:** `accent = "#88c0d0"` in [full.toml](../examples/config/full.toml).
- **Dependencies/interactions:** applies to every bar output and both renderers; layout stays workspace buttons followed by active-window title and connection/action status. Long workspace lists scroll horizontally; titles are limited to 100 characters and control characters are removed.
- **Application:** UI restart required; managed `cabby reload` queues it.
- **Migration:** new optional setting; existing configurations retain defaults.

### `show_window_title`

- **Purpose:** Show the active window title for each output; hide it with false. Falls back to app ID when the title is absent.
- **Type/default:** boolean; `true` when omitted.
- **Values:** true or false; no units.
- **Example:** `show_window_title = true` in [full.toml](../examples/config/full.toml).
- **Dependencies/interactions:** applies to every bar output and both renderers; layout stays workspace buttons followed by active-window title and connection/action status. Long workspace lists scroll horizontally; titles are limited to 100 characters and control characters are removed.
- **Application:** UI restart required; managed `cabby reload` queues it.
- **Migration:** new optional setting; existing configurations retain defaults.

### `launcher_width`

- **Purpose:** width of the centered launcher overlay on the active output.
- **Type/default:** integer; `640` when omitted.
- **Values:** `320..1200`, logical pixels; the compositor may constrain small outputs.
- **Example:** `launcher_width = 640` in [full.toml](../examples/config/full.toml).
- **Dependencies/interactions:** uses the shared `renderer`, colors, and `font_size`; no monitor selection or resolution change.
- **Application:** applied at the next launcher opening. Managed `cabby reload` publishes it; an already-open launcher retains its layout until closed.
- **Migration:** new optional setting; omission uses defaults.

### `launcher_rows`

- **Purpose:** number of application results per visible page. Up/Down crosses page boundaries; results are not discarded.
- **Type/default:** integer; `8` when omitted.
- **Values:** inclusive `4..12` rows; no pixel units.
- **Example:** `launcher_rows = 8` in [full.toml](../examples/config/full.toml).
- **Dependencies/interactions:** overlay height follows this count and `font_size`. The panel scrolls when constrained; reduce rows/text size for very small outputs.
- **Application:** applied at next opening after managed reload; direct invocation reads on startup.
- **Migration:** new optional setting.

### `launcher_terminal`

- **Purpose:** literal executable and argument prefix for `Terminal=true` desktop applications. The application's arguments are appended as separate arguments.
- **Type/default:** array of strings; `[]` when omitted. Empty means terminal apps remain searchable but launching one reports a configuration error.
- **Values:** 0..32 nonempty, NUL-free arguments; no shell expansion or command string parsing.
- **Example:** `launcher_terminal = ["foot", "-e"]` in [software.toml](../examples/config/software.toml). For another terminal, supply its equivalent execute-command flag.
- **Dependencies/interactions:** the selected terminal must be installed separately. No terminal package is mandatory for Cabby development. Ordinary GUI apps ignore this prefix. Desktop `Path` becomes the working directory. Shared terminal-server processes keep their own ownership.
- **Application:** selection uses the launcher's startup snapshot for preliminary validation; the launch helper revalidates against the current config/snapshot. Close/reopen after managed reload for consistent UI behavior.
- **Migration:** new optional setting.

## Keeping this reference complete

The configuration type and its setting-key metadata come from the same Rust declaration. Tests require every key in this reference and the full example, parse/validate every `.toml` example with the production parser, check enum alternatives, and compare the full example with actual defaults. Additions, removals, renames, new values, default changes, and changed behavior must update code, reference, examples, and relevant tests in the same branch.
