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
- **Values:** any subset of `"workspaces"`, `"clock"`, `"audio"`, `"network"`, `"bluetooth"`, `"tray"`, `"apps"`, `"title"`, `"status"`, `"idle"`, `"nightlight"`, `"caffeine"`, `"clipboard"`, `"screenshot"`, in the desired order. Unknown names and duplicate widgets anywhere across the three regions are errors. `[]` leaves this region empty.
- **Example:** `bar_left = ["workspaces"]`; optional Apps/title/status placement is demonstrated in [custom-layout.toml](../examples/config/custom-layout.toml).
- **Dependencies/interactions:** the middle uses its content width and equal side regions divide the remaining space. Overflow scrolls horizontally within its region; it cannot shift or cover another region. Each array replaces that region's default, while omitted regions still use their defaults. Widgets omitted from all regions disappear; omitted clock/network/tray widgets do not start their background subscriptions. See the widget behavior below.
- **Application:** valid config reload updates regions in place.
- **Migration:** defaults change from the former fixed Apps/device/workspace/title/status row to the user-requested three-region layout. Existing files inherit these defaults without adding keys. Include `apps`, `title` or `status` explicitly to restore those optional contributions; the launcher command/keybinding remains available.

### `bar_middle`

- **File/purpose:** `config.toml`; ordered widgets centered on the output, independent of left/right content widths.
- **Type/default:** array of strings; `["clock"]` when omitted. No units.
- **Values:** the same widget names and uniqueness rules as `bar_left`; `[]` leaves this region empty.
- **Example:** `bar_middle = ["clock"]` in [full.toml](../examples/config/full.toml).
- **Dependencies/interactions:** uses its content width, centered between equal side regions; oversized contents scroll within its available region. To move the clock elsewhere, explicitly remove it from this list as well as adding it to the destination.
- **Application:** valid config reload updates regions in place.
- **Migration:** omitted values now show local clock/date in the middle.

### `bar_right`

- **File/purpose:** `config.toml`; ordered widgets aligned to the right of the bar.
- **Type/default:** array of strings; `["audio", "network", "bluetooth", "caffeine", "nightlight", "clipboard", "tray"]` when omitted. No units.
- **Values:** the same widget names and uniqueness rules as `bar_left`; `[]` leaves this region empty.
- **Example:** `bar_right = ["audio", "network", "bluetooth", "caffeine", "nightlight", "clipboard", "tray"]` in [full.toml](../examples/config/full.toml).
- **Dependencies/interactions:** uses half of the space remaining beside the middle region; a large tray or long status scrolls within the region. Audio/Bluetooth keep their existing on-demand panels. Network is read-only status; it does not change connections or radio state.
- **Application:** valid config reload updates regions in place.
- **Migration:** the default order is audio, network, Bluetooth, Caffeine, night light, clipboard, system tray. Existing explicit arrays keep their order; add `clipboard` before `tray` to show its history picker. If `clipboard` already appears on the left/middle and `bar_right` was omitted, set an explicit right list without `clipboard` after upgrading; otherwise the new default duplicates it and validation rejects the config. [clipboard.toml](../examples/config/clipboard.toml) demonstrates this arrangement. The middle uses content width, leaving equal side regions without a config change.

#### Widget behavior

Status/action widgets use repository-owned 18-pixel outline SVG icons with no icon-font dependency. Workspace names, date/time and the optional window title retain text. `clipboard` opens the existing in-memory history picker; `screenshot` opens capture selection. Hovering a status/action/tray icon for 450 ms shows a brief opaque hint beside the bar on that output. This separate input-transparent surface takes no keyboard focus; leaving, activating a control, configuration reload or output removal dismisses it. Hints use the configured background/foreground and typography. No tooltip or icon configuration key is introduced; appearance applies after upgrading/restarting the desktop.

`workspaces` follows niri and shows an unavailable label if disconnected. `clock` shows local date and 24-hour time (for example `Thu 05 Nov  09:07`) and updates at minute boundaries; wall-clock jumps are reflected within a minute. Timezone/format configuration is not exposed; restart after changing the process timezone. `audio` and `bluetooth` use outline icons and open their device panels; brief hover hints report their event-driven status. Audio reports default-output volume/mute; Bluetooth reports power/connected count. Omitting Bluetooth stops its observation when its panel is closed. Audio observation stops only when its widget/panel and volume/microphone OSD are all inactive. Enabled OSD sources also keep niri focus events active for output selection, even without workspaces/title/status widgets. Bluetooth registers its agent only while the panel is open; passive observation never starts discovery. `network` reads NetworkManager's connection type/state and connectivity events on the system bus, including offline, connecting, local-only, limited and captive-portal states. Missing NetworkManager shows an offline icon with an unavailable hint; owner/bus loss clears status and reconnects after two seconds. No network scan or connectivity-check command is issued. `tray` hosts [StatusNotifier items and menus](tray.md). `caffeine` shows off/manual/automatic state and sleep-inhibitor failures, with a brief hint listing reasons and sleep-inhibitor failure. Clicking toggles only the manual reason. `idle` uses a lock icon with automatic-lock status in its hint (`Idle lock`, `Idle disabled`, `Locking`, `Idle failed`, or unavailable `Idle ?`) and toggles a coordinator-lifetime manual Caffeine reason; `Keep awake` means inhibited. Changes from CLI controls appear through atomic runtime status updates. No subscription starts when omitted. The default regions do not include it; see [idle.toml](../examples/config/idle.toml). `nightlight` uses a moon icon (slashed when off/paused), with off/scheduled/night/forced/unavailable state in its hint and toggles the same coordinator-owned state as `cabby nightlight-toggle`; its tooltip reports temperature/output failures. Forced mode returns to scheduled-on on toggle, then the next toggle disables it. It is included before the tray by default; explicit old bar lists must add it to show the indicator. `apps` opens the launcher. `title` shows the output's active window title when `show_window_title` is true. `status` shows niri connection/action errors. The middle region uses its content width; equal side regions share the remaining space, keeping the middle centered while leaving room for side indicators. Overflow scrolls horizontally within its region. Shared typography, colors, spacing and the bar's top/bottom placement apply to every region.

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

- **Purpose:** Shared body text size using the system sans-serif font. Fluent panel/launcher/notification headings add five logical pixels and use semibold weight; supporting text keeps the body size with reduced emphasis.
- **Type/default:** integer; `14` when omitted.
- **Values:** 10..32 logical pixels; at least 8 smaller than bar_height.
- **Example:** `font_size = 14` in [full.toml](../examples/config/full.toml).
- **Dependencies/interactions:** applies to every bar output and both renderers; widget placement follows `bar_left`, `bar_middle`, and `bar_right`. Overflow scrolls within each region; titles are limited to 100 characters and control characters are removed.
- **Application:** live update after valid automatic/manual reload; lock and launcher use the next opening.
- **Migration:** new optional setting; existing configurations retain defaults.

### `background`

- **Purpose:** Opaque base surface for the bar, launcher, audio/Bluetooth panels, tray menus, clipboard/screenshot UI and notifications.
- **Type/default:** string; `"#181b22"` when omitted.
- **Values:** Opaque #RRGGBB hexadecimal RGB, case-insensitive digits.
- **Example:** `background = "#181b22"` in [full.toml](../examples/config/full.toml).
- **Appearance:** [Solid Fluent-inspired surfaces](appearance.md) use this exact base color; raised controls, selection fills and borders mix it with `foreground`. Vulkan adds a subtle panel shadow; software omits that shadow to preserve configured color/contrast. No glass or blur. Existing palette values remain valid after this visual update; no key/default migration is required.
- **Dependencies/interactions:** applies to every bar output and both renderers; widget placement follows `bar_left`, `bar_middle`, and `bar_right`. Overflow scrolls within each region; titles are limited to 100 characters and control characters are removed.
- **Application:** live update after valid automatic/manual reload; lock and launcher use the next opening.
- **Migration:** new optional setting; existing configurations retain defaults.

### `foreground`

- **Purpose:** Shared normal text color. Solid Fluent controls derive neutral raised surfaces, borders and lower-emphasis text from this color and `background`; full text remains opaque.
- **Type/default:** string; `"#e5e9f0"` when omitted.
- **Values:** Opaque #RRGGBB hexadecimal RGB, case-insensitive digits.
- **Example:** `foreground = "#e5e9f0"` in [full.toml](../examples/config/full.toml).
- **Dependencies/interactions:** applies to every bar output and both renderers; widget placement follows `bar_left`, `bar_middle`, and `bar_right`. Overflow scrolls within each region; titles are limited to 100 characters and control characters are removed.
- **Application:** live update after valid automatic/manual reload; lock and launcher use the next opening.
- **Migration:** new optional setting; existing configurations retain defaults.

### `accent`

- **Purpose:** Workspace underline, launcher/audio selection markers, sliders, focused search borders and primary-action color. Selected rows use a neutral fill; ordinary actions are neutral. Iced derives contrasting text for filled primary actions. Notification actions use ordinary neutral styling unless explicitly promoted. Existing values remain valid; this visual migration changes emphasis rather than key names/defaults.
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
- **Dependencies/interactions:** overlay height follows this count and `font_size`, with a 24-pixel minimum row-content height for icons. Search, Close and result status remain outside the scrolling result list. The results scroll when constrained; reduce rows/text size for very small outputs.
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

The notification UI uses `config.toml` and the existing `renderer`, `font_size`, `background`, `foreground`, and `accent` settings with their documented types, defaults, ranges, and examples. `bar_position` and `bar_height` also determine notification top clearance and available vertical space. No separate notification settings are supported. All outputs share the theme; card placement and retention limits are fixed policies documented in [notifications.md](notifications.md). Cards retain their slots as others close; hover pauses remaining expiry. Geometry edits move the existing slots without resetting notification lifetimes.

Notification colors, body typography and geometry update in place. Ordinary config or wallpaper changes preserve active/queued notifications and D-Bus ownership. Renderer changes require a restart and discard in-memory notifications. Direct invocation watches its config file. Existing configurations require no key migration; an existing notification owner still causes a reported conflict rather than replacement.

## Desktop plugin host migration

Bar and wallpaper share `cabby-ui.service`. `cabby restart-ui` explicitly restarts both, while ordinary automatic/manual reload applies changes in place. The old separate wallpaper unit remains removed; see [service migration](services.md#desktop-host-migration). Standalone desktop/bar/wallpaper hosts watch their selected config file.

## Audio panel interactions

The `"audio"` widget opens the audio panel. Shared colors and body typography update in place; headings use fixed sizes and button labels use the startup text size. Panel geometry follows bar/output settings; renderer changes require restart. Ordinary reload preserves the panel and operations. Runtime device controls are described in [audio.md](audio.md). No audio-specific settings are added.


## Bluetooth panel interactions

Bluetooth uses the same shared appearance and panel geometry as audio. Ordinary reload preserves pairing and discovery; renderer changes or explicit desktop restart close the panel and cancel its interaction. No Bluetooth-specific settings are added. See [bluetooth.md](bluetooth.md).

Renderer changes reset start counters before restarting active components. Other changes do not restart processes. Stopped components stay stopped; automatic crash recovery remains bounded.

The [system tray](tray.md) shares desktop appearance. Left click activates the application, falling back to its exported menu on failure or a one-second activation timeout; right click opens the exported menu directly. Menu-only items open their menu on either click. Ordinary reload preserves open menus and item ownership; renderer changes require a desktop restart. No tray-specific settings are added. Existing configurations gain the activation fallback after upgrading/restarting the desktop; no configuration migration is needed.
Device panels and tray menus open beside the clicked widget on its output, below a top bar or above a bottom bar, clamped to the available space. Changing `bar_position` or `bar_height` moves an open panel in place. Escape, outside click, focus loss, close and removal of its output dismiss the panel.

The [session menu](session-menu.md) shares the palette/typography and desktop panel slot, with centered placement and no additional setting. `cabby session-menu` toggles it; ordinary reload preserves selection/confirmation and updates layout in place. Renderer changes require a desktop restart. Lock-induced focus loss preserves an already pending suspend request; ordinary menu dismissal never unlocks a locker.

### `idle_lock_seconds`

- **File/purpose:** `config.toml`; automatic lock after compositor-reported inactivity.
- **Type/default:** integer seconds; `0` when omitted disables automatic locking.
- **Values:** `0..86400` inclusive; positive values register inhibitor-aware idle notifications on all seats.
- **Example:** `idle_lock_seconds = 600` in [idle.toml](../examples/config/idle.toml).
- **Dependencies/interactions:** the coordinator owns observation; ext-idle-notify and a seat are required. All seats must be idle before locking. Compositor application inhibitors are respected. Requires the independent Cabby locker and PAM setup. A failed attempt waits for new activity before retrying; no repeated lock loop. Disconnection clears idle state and retries the connection after two seconds. Missing protocol/seat reports unsupported, without a fallback timer. Manual `cabby idle-inhibit-on`, `idle-inhibit-off`, or `idle-inhibit-toggle` affects automatic idle lock/display-off/suspend deadlines; it lasts until coordinator restart. `cabby idle-status` reports status. The optional `idle` bar widget exposes the same toggle and status. Explicit lock/suspend commands remain available.
- **Application:** valid reload recreates notifications and starts a full new countdown. Inhibition cancels observation and pending owned clients; releasing inhibition starts a fresh countdown. A lock job already accepted by systemd may still complete; no cleanup unlock occurs. Coordinator stop/disconnect cancels owned helper clients, while the independent active locker keeps its lifetime.
- **Migration:** omitted setting preserves previous disabled behavior. No user config or bindings are changed on install.

### `idle_display_off_seconds`

- **File/purpose:** `config.toml`; inactivity timeout for niri DPMS display power-off after confirmed locking.
- **Type/default:** integer seconds; omitted or `0` disables automatic display power changes.
- **Values:** `0..86400`; a positive value requires positive `idle_lock_seconds` and must be strictly greater than it.
- **Example:** `idle_lock_seconds = 600` with `idle_display_off_seconds = 660` in [idle.toml](../examples/config/idle.toml).
- **Dependencies/interactions:** niri IPC/CLI and the independent locker are required. All seats must be idle; the compositor's application inhibitors and Cabby's manual inhibitor apply. The display action independently verifies confirmed lock, including after a failed earlier idle lock. It never removes outputs or changes output configuration. Activity restores display power without unlocking. A failed action is not repeatedly retried in the same idle cycle. Explicit `cabby display-off` also requires confirmed locking; `cabby display-on` requests restoration without authentication. Niri's power actions are documented in its [IPC reference](https://niri-wm.github.io/niri/niri_ipc/enum.Action.html#variant.PowerOffMonitors).
- **Application:** valid reload resets idle timers and restores previously requested power-off before reconfiguration. Inhibition and graceful observer exit also restore; abrupt coordinator failure relies on compositor input handling or explicit `display-on`, with no cleanup unlock. Accepted IPC actions and subsequent activity are not an atomic transaction; hardware wake behavior remains unverified.
- **Migration:** no automatic display policy is introduced unless explicitly configured. No change to user files or bindings during install.

### `idle_suspend_seconds`

- **File/purpose:** `config.toml`; automatically request system suspend after this many seconds of inactivity, measured from the last activity rather than from display-off.
- **Type/default:** integer seconds; omitted or `0` disables automatic suspend.
- **Values:** `0..86400`. A positive value requires positive `idle_lock_seconds` and must exceed it and any enabled `idle_display_off_seconds`. Display-off may remain disabled.
- **Example:** `idle_lock_seconds = 900`, `idle_display_off_seconds = 1200`, `idle_suspend_seconds = 2700` locks after 15 minutes, powers displays off after 20, and requests suspend after 45. See [idle.toml](../examples/config/idle.toml) for another complete sequence.
- **Dependencies/interactions:** requires ext-idle-notify, the independent locker/PAM support, and systemd/logind suspend support. All seats must be idle and compositor application inhibitors apply. Caffeine reasons with `caffeine.block_idle = true` pause every idle deadline; `caffeine.block_sleep = true` also holds a sleep inhibitor, which the suspend request respects. The existing `cabby suspend` path confirms the lock again and rechecks the session/locker before requesting sleep. Lock failure prevents suspend; rejected suspend leaves the lock intact. An attempt runs once per idle cycle, with no retry loop. `lock_before_sleep` separately protects sleep initiated elsewhere; automatic suspend does not require that switch because its own request always confirms locking. Logind resume observation resets the timers even when external sleep locking/night light are disabled.
- **Application:** valid reload resets all idle deadlines. Activity, inhibition, reconfiguration and coordinator shutdown cancel pending owned requests; activity/restoration never unlocks. Releasing inhibition or resuming starts a fresh idle cycle. Already accepted systemd lock/suspend jobs cannot be withdrawn by cancelling the CLI, so input and accepted power requests are not atomic. `cabby idle-status` reports all three timeouts plus `suspending`, `suspend-requested` or `suspend-failed`; request acceptance does not prove physical sleep.
- **Migration:** opt-in; omitted values preserve the previous no-auto-suspend behavior. Install the supporting build before applying a configuration containing this new key. Install/upgrade never overwrites preferences. Real hardware sleep/resume remains a live-use validation item.

### `lock_before_sleep`

- **File/purpose:** `config.toml`; attempt confirmed locking when logind announces an externally initiated system sleep.
- **Type/default:** boolean; `false` when omitted, so no sleep inhibitor is acquired.
- **Values:** `true` or `false`; no units.
- **Example:** `lock_before_sleep = true` in [idle.toml](../examples/config/idle.toml).
- **Dependencies/interactions:** requires installed Cabby lock support and the system-bus org.freedesktop.login1 service. Uses a caller-owned `sleep`/`delay` inhibitor FD, released on lock readiness, failure/deadline, cancellation or owner loss. The client budget is InhibitDelayMaxUSec minus 100 milliseconds, capped at 30 seconds. A delay cannot veto sleep; logind's deadline remains authoritative and failure may still be followed by sleep. `cabby suspend` retains its separate confirmation-before-request guarantee. No host suspend is initiated by observing signals. Idle inhibitors do not suppress sleep locking. Missing service, rejected FD acquisition, bus loss or owner change report unavailable and reconnect after two seconds. The FD is close-on-exec and never inherited by lock/PAM children.
- **Application:** live reload cancels/replaces observation and its owned clients/FDs. Resume re-acquires a fresh inhibitor and resets idle/display/suspend timers, preserving the manual inhibitor and active lock. `cabby status` (and idle-status replies) report sleep watcher status. Coordinator shutdown never requests unlock; an accepted lock job remains independently owned.
- **Migration:** opt-in; installation never enables it or changes bindings. Late startup during an already pending sleep is best effort. Real sleep, resume and real-user authentication remain unverified.

### `nightlight`

- **File/purpose:** `[nightlight]` table in `config.toml`; native per-output color temperature.
- **Type/default:** table; all child defaults below apply when omitted. Unknown child keys are errors.
- **Example:** [full.toml](../examples/config/full.toml); scheduled, forced, coordinate, automatic and address alternatives are in the example directory.
- **Dependencies/interactions:** requires compositor wlr-gamma-control support and usable output gamma tables. The normal niri session advertises the protocol; nested niri may not. Ownership conflicts/output failures are reported, without fighting another gamma client. The location table supplies the schedule. The default right bar includes `nightlight` before `tray`; add it explicitly to pre-existing custom regions.
- **Application:** valid reload applies immediately; ordinary reload never restarts an active lock. Settings/control commands are shared with the indicator/keybinding. Runtime overrides last until coordinator restart or the matching persistent enable/force setting changes.
- **Migration:** new table; old configurations inherit disabled night light. The indicator appears by default unless regions are explicitly configured. No automatic configuration writes.

### `nightlight.enabled`

- **File/purpose/type/default:** `[nightlight]`, boolean `false`; enables scheduled day/night temperatures.
- **Values/example:** `true` or `false`; `enabled = true` in [nightlight-custom.toml](../examples/config/nightlight-custom.toml). No units.
- **Interactions:** requires a location/custom schedule; missing schedule releases gamma and reports `no-schedule`. Force works independently. `cabby nightlight-enable`, `nightlight-disable`, and `nightlight-toggle` override this in runtime. Toggle from forced mode returns to scheduled-on, then the next toggle disables. Explicit disable also clears force.
- **Application:** immediate on valid reload or control request; disabling releases gamma ownership and restores native output.

### `nightlight.force`

- **File/purpose/type/default:** `[nightlight]`, boolean `false`; holds night temperature independently of schedule and enabled state.
- **Values/example:** `force = true` in [nightlight-forced.toml](../examples/config/nightlight-forced.toml); no units.
- **Interactions:** no location is required; `cabby nightlight-force-toggle` toggles runtime force. The indicator shows forced state, or unavailable if gamma cannot apply.
- **Application:** immediate on reload/control; clearing force returns to the configured scheduled/disabled state.

### `nightlight.temperature_day`

- **File/purpose/type/default:** `[nightlight]`, integer `6500`; day white-point temperature in Kelvin.
- **Values/example:** `1000..10000`; must exceed night by at least `100`. Example `temperature_day = 6500` in [full.toml](../examples/config/full.toml).
- **Interactions:** scheduled transitions interpolate between these temperatures; values are approximate color-temperature multipliers, not colorimeter calibration. Enabled scheduled daytime retains gamma ownership, matching Noctalia; disabling restores native tables.
- **Application:** valid reload applies the current target immediately; no restart.

### `nightlight.temperature_night`

- **File/purpose/type/default:** `[nightlight]`, integer `4000`; night/forced white-point temperature in Kelvin.
- **Values/example:** `1000..10000`, at least `100` below day. Example `temperature_night = 4500` in [nightlight-auto.toml](../examples/config/nightlight-auto.toml).
- **Interactions:** one-hour fades are centered on sunrise/sunset and anchored to wall clock. Startup/enable/force/reload snap to the current target; only the schedule fades. Uploads are coalesced when unchanged and paced during transitions.
- **Application:** immediate on valid reload; schedule is rechecked at most one minute apart, more often during large transitions. No restart.

### `location`

- **File/purpose/type/default:** `[location]` table in `config.toml`; shared location/custom sunrise-sunset model for night light. Omitted fields use defaults below; unknown children fail validation.
- **Example:** [nightlight-location.toml](../examples/config/nightlight-location.toml), [nightlight-custom.toml](../examples/config/nightlight-custom.toml).
- **Interactions:** explicit custom times override coordinates. Otherwise automatic lookup takes precedence over address lookup, followed by manual coordinates when available. No resolved location is invented or silently replaced by custom times. Lookup failure keeps the previous valid coordinates and retries in five minutes. Approximate astronomical times are recomputed with date/timezone; polar day/night is supported.
- **Application:** immediate valid reload; coordinates/lookup results remain in memory, without disk cache or content logging. Resume reevaluates the schedule; see night-light documentation for sleep observation.

### `location.auto_locate`

- **File/purpose/type/default:** `[location]`, boolean `false`; opt-in IP-derived approximate location.
- **Values/example:** `auto_locate = true` in [nightlight-auto.toml](../examples/config/nightlight-auto.toml); no units.
- **Interactions:** uses `https://api.noctalia.dev/geolocate`, as Noctalia does. The provider sees the connection IP. Takes precedence over `address`; refreshes every six hours while needed. No lookup runs for disabled night light or explicit custom schedules. Uses bounded supervised curl with HTTPS-only requests, eight-second network limit and 16 KiB response limit; failures do not log response contents.
- **Application:** valid reload cancels obsolete requests and resolves the new source; no restart.

### `location.address`

- **File/purpose/type/default:** `[location]`, UTF-8 string `""`; optional city/address geocoding.
- **Values/example:** at most 1024 bytes without control characters; `address = "Berlin, Germany"` in [nightlight-address.toml](../examples/config/nightlight-address.toml). No units.
- **Interactions:** when auto lookup is off, nonempty text is URL-encoded and sent to `https://api.noctalia.dev/geocode?city=...`; no shell evaluation. Empty text selects manual coordinates. Successful address results remain in memory across toggles/reconnects until the source changes or resume requests refresh; failure retries after five minutes. No address/response content is logged.
- **Application:** immediate valid reload cancels old requests; no restart.

### `location.custom_schedule`

- **File/purpose/type/default:** `[location]`, boolean `false`; use explicit sunrise/sunset instead of astronomical times.
- **Values/example:** `custom_schedule = true` in [nightlight-custom.toml](../examples/config/nightlight-custom.toml); no units.
- **Interactions:** requires valid, distinct `sunset` and `sunrise`. Overrides coordinates and skips network lookup for night light. Invalid custom schedules reject the whole reload, retaining previous valid config.
- **Application:** immediate valid reload; current clock-anchored target applies, without restarting the fade.

### `location.sunset`

- **File/purpose/type/default:** `[location]`, string `""`; local night boundary (fade midpoint).
- **Values/example:** empty or exact 24-hour `HH:MM`, `00:00..23:59`; `sunset = "20:30"` in [nightlight-custom.toml](../examples/config/nightlight-custom.toml). Units are local hours/minutes.
- **Interactions:** used only when custom_schedule is true, which requires nonempty/distinct boundaries. Not a fallback for failed geolocation. Fade starts 30 minutes before and finishes 30 minutes after.
- **Application:** immediate valid reload; no restart.

### `location.sunrise`

- **File/purpose/type/default:** `[location]`, string `""`; local day boundary (fade midpoint).
- **Values/example:** empty or exact `HH:MM`, `00:00..23:59`; `sunrise = "07:30"` in [nightlight-custom.toml](../examples/config/nightlight-custom.toml). Local hours/minutes.
- **Interactions:** same custom-schedule rules as sunset; overnight and daytime-crossing schedules are supported. Process timezone applies; restart after changing the process timezone.
- **Application:** immediate valid reload; no restart.

### `location.latitude`

- **File/purpose/type/default:** `[location]`, optional floating-point number; omitted by default. Manual latitude in degrees.
- **Values/example:** finite `-90..90`; `latitude = 52.52` in [nightlight-location.toml](../examples/config/nightlight-location.toml).
- **Interactions:** both coordinates are required together. Custom times take precedence; resolved network coordinates take precedence when available. No manual coordinates are sent to the lookup provider.
- **Application:** immediate valid reload recomputes the current astronomical schedule; no restart.

### `location.longitude`

- **File/purpose/type/default:** `[location]`, optional floating-point number; omitted by default. Manual longitude in degrees.
- **Values/example:** finite `-180..180`; `longitude = 13.405` in [nightlight-location.toml](../examples/config/nightlight-location.toml).
- **Interactions:** requires latitude; no schedule is assumed when neither coordinates nor custom times resolve. Examples use public city coordinates.
- **Application:** immediate valid reload; no restart.

### `caffeine`

- **File/type/purpose:** `[caffeine]` table in `config.toml`; combines independent manual, audio and fullscreen inhibition reasons. Omission uses child defaults below; unknown keys fail validation.
- **Example:** [caffeine.toml](../examples/config/caffeine.toml). The `caffeine` bar widget is included before night light by default; explicit existing bar lists must add it.
- **Interactions:** with default effects, each active reason prevents Cabby's automatic idle lock/display-off/suspend and holds a logind sleep/block inhibitor; the child switches below can disable either effect. Manual off removes only the manual reason. Qualifying fullscreen pauses night light on affected outputs by default; manual pause is separately opt-in and audio never pauses it. Explicit locking remains available; explicit suspend honors block inhibitors and can be rejected until reasons clear. Sleep/block failure is reported independently of idle/night policy; there is no authorization prompt or service management.
- **Application:** live reload; enabling/disabling sources owns/cancels their subscriptions and starts fresh idle countdowns when the last reason clears. No preferences are rewritten. `caffeine-on|off|toggle|status` use coordinator-lifetime state; the old idle-inhibit commands are aliases for the same manual reason and now also apply configured sleep/night-light effects.
- **Migration:** automatic sources default off; dotfiles opt into both to replace the external audio helper. Manual control is always available. No extra daemon or production unit.

### `caffeine.audio`

- **File/type/default:** `[caffeine]`, boolean `false`; inhibit for any running output or non-monitor recording input.
- **Values/example:** `audio = true` in [caffeine.toml](../examples/config/caffeine.toml); no units.
- **Interactions:** event-driven pactl subscription/snapshots work with PipeWire's PulseAudio compatibility service. RUNNING state counts even when muted; monitor sources are excluded. Paused/suspended/unknown devices do not hold a reason. Disconnect/error clears this reason and reconnects after two seconds, preserving all other reasons. No audio content/device names are logged; no sound state is changed. Observation is independent of the bar/device panel.
- **Application:** valid reload starts/cancels owned helpers. Disabling clears only this reason; helper cleanup stays in the coordinator cgroup. No restart.

### `caffeine.fullscreen`

- **File/type/default:** `[caffeine]`, boolean `false`; react to actual fullscreen windows, requiring focus by default.
- **Values/example:** `fullscreen = true` in [caffeine.toml](../examples/config/caffeine.toml); no units.
- **Interactions:** uses foreign-toplevel fullscreen/minimized state and output-enter/leave events, committed at protocol done. Maximized/minimized and off-output windows do not qualify. Focus is required by default, so niri fullscreen windows on hidden workspaces cannot keep inhibition active. Opting out of focus gating also includes background/hidden fullscreen windows. Qualifying windows contribute output names, so reasons/outputs compose. Output names map across the independent gamma connection. If names are unavailable, night-light pause conservatively covers all outputs until mapping resolves. Missing protocol/disconnect is reported and clears this source; no geometry guessing, title logging or polling. Bounds: 512 windows, 32 observed outputs; connection recovery after two seconds.
- **Application:** valid reload starts/cancels observation. Ending fullscreen, minimizing/closing a window, losing its output or losing focus when required removes its contribution without disturbing manual/audio reasons. When fullscreen pause is enabled, night light restores the current schedule/force target. No restart.

### `caffeine.fullscreen_requires_focus`

- **File/type/default:** `[caffeine]` in `config.toml`, boolean `true`; fullscreen windows must have compositor-reported keyboard focus.
- **Values/example:** `fullscreen_requires_focus = false` in [background example](../examples/config/caffeine-background.toml); no units.
- **Interactions:** requires `caffeine.fullscreen = true`. Niri reports output membership even on hidden workspaces; focus gating prevents these windows from holding inhibition. False includes background fullscreen windows on other outputs and hidden workspaces. It still excludes minimized/off-output windows; this setting does not infer visibility from geometry or titles.
- **Application:** live reload recomputes the existing observation without reconnecting. No restart.

### `caffeine.inhibit_idle`

- **File/type/default:** `[caffeine]` in `config.toml`, boolean `true`; active reasons prevent automatic idle lock/display-off.
- **Values/example:** `inhibit_idle = false` keeps automatic idle actions enabled during Caffeine. No units.
- **Interactions:** affects all reasons; explicit lock is always available. Disabling or releasing inhibition starts fresh idle countdowns. Existing compositor application inhibitors remain independent.
- **Application:** live reload, no restart. [Full example](../examples/config/full.toml) uses true; [effects example](../examples/config/caffeine-effects.toml) uses false.

### `caffeine.inhibit_sleep`

- **File/type/default:** `[caffeine]` in `config.toml`, boolean `true`; active reasons acquire a logind `sleep/block` FD.
- **Values/example:** `inhibit_sleep = false` permits sleep while retaining other configured effects. No units.
- **Interactions:** independent of lock-before-sleep's delay lease. Block acquisition requires logind authorization; failures are reported, never claimed as protection. Explicit suspend also respects this inhibitor; clear reasons to suspend, or disable this setting.
- **Application:** live reload acquires/releases the lease; no restart. [Full example](../examples/config/full.toml) uses true; [effects example](../examples/config/caffeine-effects.toml) uses false.

### `caffeine.manual_pause_nightlight`

- **File/type/default:** `[caffeine]` in `config.toml`, boolean `false`; manual caffeine temporarily pauses night light on all outputs.
- **Values/example:** `manual_pause_nightlight = true` in [effects example](../examples/config/caffeine-effects.toml); no units.
- **Interactions:** independent of fullscreen-specific pause. Audio never pauses night light. Pauses release gamma ownership and never edit enabled/force/temperature preferences; changes made while paused apply when inhibition ends.
- **Application:** live reload recomputes effective policy immediately. No restart or persistent manual-state write.

### `caffeine.fullscreen_pause_nightlight`

- **File/type/default:** `[caffeine]` in `config.toml`, boolean `true`; fullscreen automation temporarily pauses night light on affected outputs.
- **Values/example:** `fullscreen_pause_nightlight = false` in [effects example](../examples/config/caffeine-effects.toml); no units.
- **Interactions:** requires `caffeine.fullscreen = true`; missing output names conservatively pause all outputs. Independent of manual pause and idle/sleep effects. Ending fullscreen restores the current schedule or forced target.
- **Application:** live reload, no restart. Disable this to retain night light while fullscreen windows still inhibit idle/sleep.

### `clipboard`

- **File/type/purpose:** `[clipboard]` table in `config.toml`; regular clipboard monitoring/ownership and bounded memory-only history. Omission uses child defaults; unknown keys are errors.
- **Example:** [clipboard.toml](../examples/config/clipboard.toml), or [disabled alternative](../examples/config/clipboard-disabled.toml).
- **Dependencies/interactions:** native `ext_data_control_manager_v1` and one Wayland seat (niri baseline). Missing protocol/disconnect reports unavailable; reconnect waits two seconds. UTF-8 text and PNG only; primary selection is untouched. `x-kde-passwordManagerHint` offers are skipped before reading. This hint cannot identify every secret. No clipboard payload is logged or written to history files.
- **Application:** live reload. Coordinator restart loses history; desktop restart preserves it. Invalid config retains the previous history policy.
- **Migration:** newly enabled by default. Disable explicitly to opt out. Optional `clipboard` bar widget opens the picker; default regions stay unchanged. `cabby clipboard` opens it through the existing desktop host; no additional process/service.

### `clipboard.enabled`

- **File/type/default:** `[clipboard]` in `config.toml`, boolean `true`; monitor and retain supported regular clipboard selections.
- **Values/example:** `enabled = false` in [disabled alternative](../examples/config/clipboard-disabled.toml); no units.
- **Interactions:** disabling clears history and releases any Cabby-owned selection, cancels transfers and closes the Wayland connection. External selections are left alone. Re-enabling observes the current selection without restoring stale history. Deleting an actively owned entry or clearing history also releases its selection.
- **Application:** live reload, no restart. Picker remains accessible and displays disabled state.

### `clipboard.max_items`

- **File/type/default:** `[clipboard]` in `config.toml`, integer `50`; retained history count, range `1..200`, no units.
- **Example:** `max_items = 100` in [clipboard.toml](../examples/config/clipboard.toml).
- **Interactions:** newest first, duplicate bytes/MIME move the existing entry to the front without adding another. Oldest entries evict when either count or byte budget is reached. Reselecting Cabby's own source is ignored by monitoring. Search is case-insensitive over full text; the picker shows at most 50 matches with 160-character previews.
- **Application:** live reload trims oldest entries immediately. No restart or persistence.

### `clipboard.max_item_bytes`

- **File/type/default:** `[clipboard]` in `config.toml`, integer `4194304` (4 MiB); encoded bytes per text/PNG item, range `65536..16777216`, units bytes.
- **Example:** `max_item_bytes = 8388608` in [clipboard.toml](../examples/config/clipboard.toml).
- **Interactions:** must be at most `clipboard.memory_bytes`. Larger/empty/malformed/unsupported offers are ignored without replacing their source. Text must be valid UTF-8 without NUL; images must decode as PNG within 8192×8192, 16,777,216 pixels and a 64 MiB decoder allocation budget. Per-transfer deadline three seconds; one incoming and at most eight outgoing transfers.
- **Application:** live reload removes oversized retained entries and bounds in-flight reception. An existing owned selection or outgoing transfer can retain its already accepted bytes until cancellation/deadline; restart is unnecessary.

### `clipboard.memory_bytes`

- **File/type/default:** `[clipboard]` in `config.toml`, integer `33554432` (32 MiB); retained encoded-payload budget, at least `clipboard.max_item_bytes` and at most `134217728`, units bytes.
- **Example:** `memory_bytes = 67108864` in [clipboard.toml](../examples/config/clipboard.toml).
- **Interactions:** oldest entries evict first. This bounds retained payloads, not total process RSS: metadata, one incoming item, current selection, bounded pending sends and image decode allocations are additional. No disk cache/history is created.
- **Application:** live reload trims history immediately; coordinator restart clears all memory. No UI restart.

### `screenshot`

- **File/type/purpose:** `[screenshot]` table in `config.toml`; screenshot save preferences and pointer inclusion. Omission uses child defaults; unknown keys fail validation.
- **Example:** [screenshot.toml](../examples/config/screenshot.toml); all defaults appear in [full.toml](../examples/config/full.toml).
- **Dependencies/interactions:** grim and the compositor's screencopy protocol provide pixels; Cabby owns the frozen-image region selector and preview. `cabby screenshot` selects a region; `cabby screenshot-monitor` previews the whole focused monitor. Optional `screenshot` bar widget uses its clicked output; default regions stay unchanged. Copy requires enabled native clipboard support and its configured size limits. No extra service/binary.
- **Application:** directory changes apply to the next Save, including an open preview. Cursor changes apply to the next capture. Shared appearance changes apply live; renderer changes restart ordinary UI. Output changes/removal cancel the frozen workflow; ordinary config reload retains it.
- **Migration:** new opt-in interaction; no screenshot or directory is created merely by starting/reloading Cabby. Capture pixels stay in memory until explicit Save. Existing niri screenshot-window can coexist.

### `screenshot.directory`

- **File/type/default:** `[screenshot]` in `config.toml`, string `""`; the destination directory for explicit PNG saves. Empty/omitted resolves to `$HOME/Pictures/Screenshots` when saving; HOME is required for that default. No units.
- **Values/example:** absolute paths or paths relative to the configuration file; `directory = "captures"` in [screenshot.toml](../examples/config/screenshot.toml). No shell evaluation or tilde expansion; NUL is invalid. Directory existence is checked only on Save.
- **Interactions:** the preview edits a filename within this directory, not an arbitrary path; it must end in `.png` (case-insensitive), contain 1..255 bytes, and contain no `/` or NUL. Generated names use local date/time plus nanoseconds. Save creates parent directories as needed and publishes a new 0600 file without overwriting existing files. Anonymous same-directory temporary files require Linux filesystem `O_TMPFILE`/link support; unsupported/denied/full destinations return an error and keep the preview. No named partial capture remains after cancellation/crash.
- **Application:** live reload, next Save; no restart. Changing HOME affects a new process; use an explicit directory for a stable path. Install/upgrade never overwrites this preference or creates its destination.

### `screenshot.include_cursor`

- **File/type/default:** `[screenshot]` in `config.toml`, boolean `false`; include the compositor cursor in the captured source image.
- **Values/example:** `include_cursor = true` in [screenshot.toml](../examples/config/screenshot.toml); no units.
- **Interactions:** passes grim's explicit `-c` option. The source is captured before selection controls are rendered; changing this cannot alter an already frozen image. Encoded capture maximum 32 MiB, dimensions at most 8192 per axis / 16,777,216 pixels and decoder allocation at most 64 MiB. Capture helper timeout five seconds; cancellation kills/reaps it.
- **Application:** live reload affects the next capture. No restart or recapture of an open preview.

## OSD

### `osd`

- **File/type/default:** optional TOML table in `config.toml`, with field defaults below. Omission enables bottom-center feedback for volume, microphone and Caps Lock; Num Lock is off.
- **Purpose:** passive, transient [on-screen feedback](osd.md) using Cabby's solid Fluent appearance. Volume/microphone show a filled progress bar and percentage; lock keys show On/Off text. No ordinary notification or input focus is involved.
- **Example:** complete defaults in [full.toml](../examples/config/full.toml), a longer/higher variant in [osd.toml](../examples/config/osd.toml), and disabled alternative in [osd-disabled.toml](../examples/config/osd-disabled.toml).
- **Interactions:** the existing desktop process owns the surface. Niri focus events select an output even when workspace/title widgets are omitted; fallback is the first available output. Shared colors/font/renderer apply. Startup, reconnect and new-device baselines are silent. The latest event replaces the previous one. OSD is hidden by the compositor's active session lock, independently of lock authentication.
- **Application/migration:** all fields apply live after a valid reload; invalid edits retain the previous valid configuration. New optional table: existing configs remain valid and gain default feedback after the updated UI starts. No compositor bindings, extra services or user config overwrite are required.

### `osd.enabled`

- **Purpose/type/default:** boolean, `true`; master switch for OSD observation/display. Valid values are `true`/`false`, no units.
- **Example:** `enabled = false` under `[osd]` in [osd-disabled.toml](../examples/config/osd-disabled.toml).
- **Interactions:** false immediately dismisses feedback and drops LED observation; audio observation continues only if an audio bar widget/panel needs it. Other shell controls keep their existing behavior.
- **Application:** live; newly started sources establish silent baselines.

### `osd.volume`

- **Purpose/type/default:** boolean, `true`; feedback for default-output volume and mute changes. Valid values `true`/`false`, no units.
- **Example:** `volume = true` under `[osd]` in [full.toml](../examples/config/full.toml).
- **Interactions:** requires `osd.enabled`; observes the existing audio worker, including external key/tool changes. Non-default devices and playback-only events are ignored. Values above 100% retain their numeric value with a full meter. Muting displays Muted with an empty meter. No audio is changed by OSD itself.
- **Application:** live; existing audio observation is retained across category toggles so the first new change is shown; newly started/reconnected workers establish silent baselines.

### `osd.microphone`

- **Purpose/type/default:** boolean, `true`; feedback for default-input level and mute changes. Valid values `true`/`false`, no units.
- **Example:** `microphone = true` under `[osd]` in [full.toml](../examples/config/full.toml).
- **Interactions:** requires `osd.enabled`; uses the same audio subscription as output feedback. It displays a microphone icon, level/mute status and a filled meter without accessing recording contents. If one snapshot changes both input and output, microphone feedback wins.
- **Application:** live; existing observation is retained, while newly started workers establish silent baselines.

### `osd.caps_lock`

- **Purpose/type/default:** boolean, `true`; On/Off feedback for Caps Lock indicator changes. Valid values `true`/`false`, no units.
- **Example:** `caps_lock = true` under `[osd]` in [full.toml](../examples/config/full.toml).
- **Interactions:** requires `osd.enabled` and readable Linux capslock LED brightness files. A bounded worker reads cached files every 200 ms; unavailable state backs off to two seconds. Membership is rescanned every two seconds. Multiple keyboards aggregate On; seats are not distinguished. Missing/invalid state and hotplug establish silent baselines. No keycodes, input-device access or additional permissions are requested.
- **Application:** live; disabling stops observation when Num Lock is also off.

### `osd.num_lock`

- **Purpose/type/default:** boolean, `false`; optional Num Lock On/Off feedback. Valid values `true`/`false`, no units.
- **Example:** `num_lock = true` under `[osd]` in [osd.toml](../examples/config/osd.toml).
- **Interactions:** requires `osd.enabled`; same read-only Linux LED bounds, availability and multi-keyboard behavior as Caps Lock. When one observation changes both lock keys, enabled Num Lock feedback wins.
- **Application:** live; initial state after enabling is silent.

### `osd.timeout_ms`

- **Purpose/type/default:** integer milliseconds, `1500`; how long feedback remains visible after its latest event. Valid range `500..5000`.
- **Example:** `timeout_ms = 2000` under `[osd]` in [osd.toml](../examples/config/osd.toml).
- **Interactions:** repeated changes replace the content and start a fresh timeout on the same surface. One cancellable timer runs only while visible; stale expiry events cannot close refreshed feedback.
- **Application:** live; changing this field gives visible feedback a fresh full countdown.

### `osd.bottom_margin`

- **Purpose/type/default:** integer logical pixels, `96`; distance from the output bottom to the centered OSD's bottom edge. Valid range `0..512`.
- **Example:** `bottom_margin = 120` under `[osd]` in [osd.toml](../examples/config/osd.toml).
- **Interactions:** horizontal center is fixed. A bottom bar sets a minimum of `bar_height + 12`; small outputs clamp size/margins to stay on screen. Card width starts at 320 pixels and grows with larger shared text; height starts at 100 pixels. Output size/scale changes recompute placement, removal dismisses it.
- **Application:** live, including a currently visible OSD, without resetting its timer.
