# Example configurations

Copy one example to `$HOME/.config/cabby-shell/config.toml` after creating the directory, without overwriting an existing configuration. Each is a complete standalone input, not a file to merge with the others.

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
