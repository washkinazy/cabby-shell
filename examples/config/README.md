# Example configurations

Copy one example to `$HOME/.config/cabby-shell/config.toml` after creating the directory, without overwriting an existing configuration. Each is a complete standalone input, not a file to merge with the others.

All examples use the [solid Fluent-inspired appearance](../../docs/appearance.md): opaque surfaces, neutral controls and accent selection markers. Existing palette/layout keys remain the complete customization interface. Vulkan adds a subtle panel shadow; software uses the same opaque surfaces without that shadow. No theme-selection key or glass effect is supported.

[osd.toml](osd.toml) demonstrates bottom-center volume/microphone progress feedback and both lock-key indicators; [osd-disabled.toml](osd-disabled.toml) disables the feature. Omitted OSD settings use their defaults, including enabled volume, microphone and Caps Lock feedback. See [OSD behavior](../../docs/osd.md).

| File | Purpose |
| --- | --- |
| [wallpaper.toml](wallpaper.toml) | Relative PNG path and fit mode; copy `assets/` too |
| [wallpaper-center.toml](wallpaper-center.toml) | Centered image alternative |
| [wallpaper-stretch.toml](wallpaper-stretch.toml) | Stretched image alternative |
| [minimal.toml](minimal.toml) | All defaults, with every setting omitted |
| [custom-layout.toml](custom-layout.toml) | Optional Apps, active-window title, and diagnostics; clock omitted |
| [full.toml](full.toml) | Every supported setting, annotated and set to its default |
| [software.toml](software.toml) | Software renderer, taller bottom bar, hidden window titles, and optional foot terminal integration |

See the [configuration reference](../../docs/configuration.md) for types, defaults, ranges, dependencies, reload behavior, and validation commands. Vulkan needs a working driver; software mode does not count as GPU validation. Tests parse all these files and enforce setting coverage. Update examples and the reference alongside every feature/setting change.

The managed desktop hosts bar and wallpaper together. `cabby restart-ui` restarts both; ordinary accepted reloads update the desktop and notifications in place; renderer changes restart them. No configuration keys change. See the service migration linked from the configuration reference when updating manually installed units.

The lock UI uses shared `renderer`, `font_size`, `background`, `foreground` and `accent` settings only when a new lock starts. Reload never restarts an active locker; invalid styling uses built-in defaults. No lock-specific settings or automatic idle activation are present. See [locking](../../docs/locking.md).

[idle.toml](idle.toml) enables a ten-minute idle lock after installing lock support; other examples leave automatic locking disabled.

Night light examples cover [custom times](nightlight-custom.toml), [manual coordinates](nightlight-location.toml), [automatic IP lookup](nightlight-auto.toml), [address geocoding](nightlight-address.toml), and [forced mode](nightlight-forced.toml). Network lookup is opt-in and no example executes it merely by validation. The default right bar now includes the night-light toggle before the system tray.

All examples with the `tray` widget use the same [tray interactions](../../docs/tray.md): right click opens the exported menu; left click activates, with a menu fallback if activation fails or times out. This compatibility behavior needs no additional setting or configuration migration.

[caffeine.toml](caffeine.toml) enables automatic audio/fullscreen reasons with default effects; [caffeine-effects.toml](caffeine-effects.toml) demonstrates independent idle/sleep and manual/fullscreen night-light switches. All settings live in `[caffeine]` and reload live. Explicit bar lists must add `caffeine` for its indicator.

[caffeine-background.toml](caffeine-background.toml) explicitly includes background/hidden fullscreen windows; the default requires keyboard focus.

[clipboard.toml](clipboard.toml) demonstrates the optional picker contribution and larger memory limits; [clipboard-disabled.toml](clipboard-disabled.toml) opts out. History is memory-only, follows the coordinator lifetime and accepts UTF-8 text/PNG. The default bar stays unchanged; `cabby clipboard` also opens the picker.

[screenshot.toml](screenshot.toml) demonstrates an optional bar button, relative capture destination and cursor inclusion. Capture/preview/copy are memory-only; the destination is created only on explicit Save. [Screenshot workflow](../../docs/screenshots.md) documents commands, cancellation and filesystem support.
