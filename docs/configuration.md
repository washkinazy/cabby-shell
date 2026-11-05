# Configuration reference

## Location and loading

Cabby reads TOML from `$HOME/.config/cabby-shell/config.toml`. This is currently one file; there are no includes, automatic subfile discovery, or layered merges. `$XDG_CONFIG_HOME` does not override this explicitly chosen default. `HOME` must be absolute when no explicit path is supplied.

`cabby validate --config PATH` and `cabby-platform-spike --config PATH` select a different file. Relative CLI paths resolve against the command's working directory. Cabby does not expand `~` or environment variables inside path strings; a shell may expand an unquoted CLI path before invoking Cabby.

A missing default file at startup uses built-in defaults. A missing explicitly selected file is an error. Empty files and omitted settings use defaults. Invalid startup configuration fails ordinary component startup. The lock UI falls back to built-in styling defaults on a parse/validation/load error; path-resolution or renderer failures still fail startup. Invalid reloads (including a removed file) retain the entire previous valid snapshot. Unknown/duplicate keys, wrong types, unknown renderer names, and invalid ranges are errors. Files are limited to 64 KiB and must be UTF-8.

The coordinator watches config and active wallpaper files with a 300 ms debounce, validates candidates and atomically publishes the active snapshot. Desktop and notification hosts consume snapshots in place: ordinary edits preserve open panels, pairing interactions, tray ownership and notifications. Wallpaper decoding runs off the UI thread and retains the current image on failure. Only `renderer` changes restart active desktop/notification services. Launcher settings apply next opening; active lock configuration never changes. Direct desktop/notification invocation watches its config file; wallpaper-file-only edits are watched by the managed coordinator. Invalid edits retain the previous valid snapshot. See [services.md](services.md).

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

### `bar_left`

- **File/purpose:** `config.toml`; ordered widgets aligned to the left of the bar.
- **Type/default:** array of strings; `["workspaces"]` when omitted. No units.
- **Values:** any subset of `"workspaces"`, `"clock"`, `"audio"`, `"network"`, `"bluetooth"`, `"tray"`, `"apps"`, `"title"`, `"status"`, in the desired order. Unknown names and duplicate widgets anywhere across the three regions are errors. `[]` leaves this region empty.
- **Example:** `bar_left = ["workspaces"]`; optional Apps/title/status placement is demonstrated in [custom-layout.toml](../examples/config/custom-layout.toml).
- **Dependencies/interactions:** the three regions occupy equal widths. Overflow scrolls horizontally within its region; it cannot shift or cover another region. Each array replaces that region's default, while omitted regions still use their defaults. Widgets omitted from all regions disappear; omitted clock/network/tray widgets do not start their background subscriptions. See the widget behavior below.
- **Application:** valid config reload updates regions in place.
- **Migration:** defaults change from the former fixed Apps/device/workspace/title/status row to the user-requested three-region layout. Existing files inherit these defaults without adding keys. Include `apps`, `title` or `status` explicitly to restore those optional contributions; the launcher command/keybinding remains available.

### `bar_middle`

- **File/purpose:** `config.toml`; ordered widgets centered on the output, independent of left/right content widths.
- **Type/default:** array of strings; `["clock"]` when omitted. No units.
- **Values:** the same widget names and uniqueness rules as `bar_left`; `[]` leaves this region empty.
- **Example:** `bar_middle = ["clock"]` in [full.toml](../examples/config/full.toml).
- **Dependencies/interactions:** uses the middle third of the content area; oversized contents scroll within that region. To move the clock elsewhere, explicitly remove it from this list as well as adding it to the destination.
- **Application:** valid config reload updates regions in place.
- **Migration:** omitted values now show local clock/date in the middle.

### `bar_right`

- **File/purpose:** `config.toml`; ordered widgets aligned to the right of the bar.
- **Type/default:** array of strings; `["audio", "network", "bluetooth", "tray"]` when omitted. No units.
- **Values:** the same widget names and uniqueness rules as `bar_left`; `[]` leaves this region empty.
- **Example:** `bar_right = ["audio", "network", "bluetooth", "tray"]` in [full.toml](../examples/config/full.toml).
- **Dependencies/interactions:** uses the right third; a large tray or long status scrolls within the region. Audio/Bluetooth keep their existing on-demand panels. Network is read-only status; it does not change connections or radio state.
- **Application:** valid config reload updates regions in place.
- **Migration:** the default order is audio, network, Bluetooth, system tray.

#### Widget behavior

`workspaces` follows niri and shows an unavailable label if disconnected. `clock` shows local date and 24-hour time (for example `Thu 05 Nov  09:07`) and updates at minute boundaries; wall-clock jumps are reflected within a minute. Timezone/format configuration is not exposed; restart after changing the process timezone. `audio` and `bluetooth` open their existing device panels. `network` reads NetworkManager's connection type/state and connectivity events on the system bus, including offline, connecting, local-only, limited and captive-portal states. Missing NetworkManager shows `Network ?`; owner/bus loss clears status and reconnects after two seconds. No network scan or connectivity-check command is issued. `tray` hosts [StatusNotifier items and menus](tray.md). `apps` opens the launcher. `title` shows the output's active window title when `show_window_title` is true. `status` shows niri connection/action errors. Shared typography, colors, spacing and the bar's top/bottom placement apply to every region.

### `bar_height`

- **Purpose:** sets the bar's logical height and matching reserved screen area.
- **Type/default:** integer; `40` when omitted.
- **Values:** inclusive range `32` through `128`, in logical pixels. Fractional output scaling determines physical pixel size.
- **Example:** `bar_height = 40` in [full.toml](../examples/config/full.toml).
- **Dependencies/interactions:** applies to the bar on each output, independently of renderer choice. It does not configure monitor resolution or scale.
- **Application:** live update after valid automatic/manual reload.
- **Migration:** first schema; no prior setting to migrate.

### `bar_position`

- **Purpose:** Which output edge holds the bar and its reserved area.
- **Type/default:** string; `"top"` when omitted.
- **Values:** "top" or "bottom".
- **Example:** `bar_position = "top"` in [full.toml](../examples/config/full.toml).
- **Dependencies/interactions:** applies to every bar output and both renderers; widget placement follows `bar_left`, `bar_middle`, and `bar_right`. Overflow scrolls within each region; titles are limited to 100 characters and control characters are removed.
- **Application:** live update after valid automatic/manual reload; lock and launcher use the next opening.
- **Migration:** new optional setting; existing configurations retain defaults.

### `bar_padding`

- **Purpose:** Horizontal padding around bar contents.
- **Type/default:** integer; `8` when omitted.
- **Values:** 0..32 logical pixels.
- **Example:** `bar_padding = 8` in [full.toml](../examples/config/full.toml).
- **Dependencies/interactions:** applies to every bar output and both renderers; widget placement follows `bar_left`, `bar_middle`, and `bar_right`. Overflow scrolls within each region; titles are limited to 100 characters and control characters are removed.
- **Application:** live update after valid automatic/manual reload; lock and launcher use the next opening.
- **Migration:** new optional setting; existing configurations retain defaults.

### `bar_spacing`

- **Purpose:** Gap between workspace buttons and bar sections.
- **Type/default:** integer; `8` when omitted.
- **Values:** 0..32 logical pixels.
- **Example:** `bar_spacing = 8` in [full.toml](../examples/config/full.toml).
- **Dependencies/interactions:** applies to every bar output and both renderers; widget placement follows `bar_left`, `bar_middle`, and `bar_right`. Overflow scrolls within each region; titles are limited to 100 characters and control characters are removed.
- **Application:** live update after valid automatic/manual reload; lock and launcher use the next opening.
- **Migration:** new optional setting; existing configurations retain defaults.

### `font_size`

- **Purpose:** Shared text size. Uses the system sans-serif font.
- **Type/default:** integer; `14` when omitted.
- **Values:** 10..32 logical pixels; at least 8 smaller than bar_height.
- **Example:** `font_size = 14` in [full.toml](../examples/config/full.toml).
- **Dependencies/interactions:** applies to every bar output and both renderers; widget placement follows `bar_left`, `bar_middle`, and `bar_right`. Overflow scrolls within each region; titles are limited to 100 characters and control characters are removed.
- **Application:** live update after valid automatic/manual reload; lock and launcher use the next opening.
- **Migration:** new optional setting; existing configurations retain defaults.

### `background`

- **Purpose:** Shared bar, launcher, audio/Bluetooth panels, and notification background color.
- **Type/default:** string; `"#181b22"` when omitted.
- **Values:** Opaque #RRGGBB hexadecimal RGB, case-insensitive digits.
- **Example:** `background = "#181b22"` in [full.toml](../examples/config/full.toml).
- **Dependencies/interactions:** applies to every bar output and both renderers; widget placement follows `bar_left`, `bar_middle`, and `bar_right`. Overflow scrolls within each region; titles are limited to 100 characters and control characters are removed.
- **Application:** live update after valid automatic/manual reload; lock and launcher use the next opening.
- **Migration:** new optional setting; existing configurations retain defaults.

### `foreground`

- **Purpose:** Shared bar, launcher, audio/Bluetooth panels, and notification normal text color.
- **Type/default:** string; `"#e5e9f0"` when omitted.
- **Values:** Opaque #RRGGBB hexadecimal RGB, case-insensitive digits.
- **Example:** `foreground = "#e5e9f0"` in [full.toml](../examples/config/full.toml).
- **Dependencies/interactions:** applies to every bar output and both renderers; widget placement follows `bar_left`, `bar_middle`, and `bar_right`. Overflow scrolls within each region; titles are limited to 100 characters and control characters are removed.
- **Application:** live update after valid automatic/manual reload; lock and launcher use the next opening.
- **Migration:** new optional setting; existing configurations retain defaults.

### `accent`

- **Purpose:** Active workspace and selected launcher-result and notification button color. Iced derives contrasting button text.
- **Type/default:** string; `"#88c0d0"` when omitted.
- **Values:** Opaque #RRGGBB hexadecimal RGB, case-insensitive digits.
- **Example:** `accent = "#88c0d0"` in [full.toml](../examples/config/full.toml).
- **Dependencies/interactions:** applies to every bar output and both renderers; widget placement follows `bar_left`, `bar_middle`, and `bar_right`. Overflow scrolls within each region; titles are limited to 100 characters and control characters are removed.
- **Application:** live update after valid automatic/manual reload; lock and launcher use the next opening.
- **Migration:** new optional setting; existing configurations retain defaults.

### `show_window_title`

- **Purpose:** Show the active window title when the `"title"` widget is included in a bar region; hide it with false. Falls back to app ID when the title is absent.
- **Type/default:** boolean; `true` when omitted.
- **Values:** true or false; no units.
- **Example:** `show_window_title = true` in [full.toml](../examples/config/full.toml).
- **Dependencies/interactions:** applies to every bar output and both renderers; widget placement follows `bar_left`, `bar_middle`, and `bar_right`. Overflow scrolls within each region; titles are limited to 100 characters and control characters are removed.
- **Application:** live update after valid automatic/manual reload; lock and launcher use the next opening.
- **Migration:** the boolean default remains true, but title display now also requires `"title"` in one region. It is omitted from the new default layout.

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

### `wallpaper_image`

- **Purpose:** PNG/JPEG image shared by all output backgrounds. Empty means solid color.
- **Type/default:** string; `""` when omitted; NUL is rejected.
- **Values:** a UTF-8 file path, absolute or relative to the directory containing the selected config file. No tilde/environment expansion. The managed snapshot normalizes paths to absolute paths.
- **Example:** `wallpaper_image = "assets/sample.png"` in [wallpaper.toml](../examples/config/wallpaper.toml); copy its `assets/` folder too.
- **Dependencies/interactions:** static PNG/JPEG only; regular files at most 32 MiB, dimensions at most 8192×8192, decoder allocation limit 256 MiB. Transparent pixels reveal wallpaper_color. Image metadata orientation is not applied. No external wallpaper helper or GTK dependency.
- **Application:** startup/reload validates and decodes images before publishing configuration. Invalid/missing images retain the current config on reload. Image content changes at the same path are detected by reload even when TOML is unchanged; successful changes replace the wallpaper in place. The managed coordinator watches both this config and the active image path. Concurrent decode failures retain the displayed image; replace image files atomically.
- **Migration:** new optional setting; omission produces a solid background. No automatic import of another wallpaper tool's settings.

### `wallpaper_color`

- **Purpose:** solid background and letterbox/transparent-image fill.
- **Type/default:** string; `"#181b22"` when omitted.
- **Values:** opaque `#RRGGBB` RGB with case-insensitive hexadecimal digits, no units.
- **Example:** `wallpaper_color = "#101820"` in [wallpaper.toml](../examples/config/wallpaper.toml).
- **Dependencies/interactions:** independent of UI theme background; applies on every output with either renderer.
- **Application:** live update on config reload.
- **Migration:** new optional setting.

### `wallpaper_mode`

- **Purpose:** place the same image independently within each output's logical bounds.
- **Type/default:** string; `"fill"` when omitted.
- **Values:** `"fill"` preserves aspect ratio and crops; `"fit"` preserves the complete image with color borders; `"stretch"` changes aspect ratio to fill; `"center"` centers at native image dimensions interpreted as logical pixels and clips overflow.
- **Example:** `wallpaper_mode = "fit"`; [center](../examples/config/wallpaper-center.toml) and [stretch](../examples/config/wallpaper-stretch.toml) alternatives are provided.
- **Dependencies/interactions:** ignored for an empty image. Output scaling determines physical pixel dimensions. Tiling, slideshows, and per-output image overrides are not implemented.
- **Application:** live update on config reload; no changes to compositor output configuration.
- **Migration:** new optional setting.

## Keeping this reference complete

The configuration type and its setting-key metadata come from the same Rust declaration. Tests require every key in this reference and the full example, parse/validate every `.toml` example with the production parser, check enum alternatives, and compare the full example with actual defaults. Additions, removals, renames, new values, default changes, and changed behavior must update code, reference, examples, and relevant tests in the same branch.

## Notification component interactions

The notification UI uses `config.toml` and the existing `renderer`, `font_size`, `background`, `foreground`, and `accent` settings with their documented types, defaults, ranges, and examples. `bar_position` and `bar_height` also determine notification top clearance and available vertical space. No separate notification settings are supported. All outputs share the theme; card placement and retention limits are fixed policies documented in [notifications.md](notifications.md).

Notification colors, body typography and geometry update in place. Ordinary config or wallpaper changes preserve active/queued notifications and D-Bus ownership. Renderer changes require a restart and discard in-memory notifications. Direct invocation watches its config file. Existing configurations require no key migration; an existing notification owner still causes a reported conflict rather than replacement.

## Desktop plugin host migration

Bar and wallpaper share `cabby-ui.service`. `cabby restart-ui` explicitly restarts both, while ordinary automatic/manual reload applies changes in place. The old separate wallpaper unit remains removed; see [service migration](services.md#desktop-host-migration). Standalone desktop/bar/wallpaper hosts watch their selected config file.

## Audio panel interactions

The `"audio"` widget opens the audio panel. Shared colors and body typography update in place; headings use fixed sizes and button labels use the startup text size. Panel geometry follows bar/output settings; renderer changes require restart. Ordinary reload preserves the panel and operations. Runtime device controls are described in [audio.md](audio.md). No audio-specific settings are added.


## Bluetooth panel interactions

Bluetooth uses the same shared appearance and panel geometry as audio. Ordinary reload preserves pairing and discovery; renderer changes or explicit desktop restart close the panel and cancel its interaction. No Bluetooth-specific settings are added. See [bluetooth.md](bluetooth.md).

Renderer changes reset start counters before restarting active components. Other changes do not restart processes. Stopped components stay stopped; automatic crash recovery remains bounded.

The [system tray](tray.md) shares desktop appearance. Ordinary reload preserves open menus and item ownership; renderer changes require a desktop restart. No tray-specific settings are added.