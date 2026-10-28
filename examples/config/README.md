# Example configurations

Copy one example to `$HOME/.config/cabby-shell/config.toml` after creating the directory, without overwriting an existing configuration. Each is a complete standalone input, not a file to merge with the others.

| File | Purpose |
| --- | --- |
| [minimal.toml](minimal.toml) | All defaults, with every setting omitted |
| [full.toml](full.toml) | Every supported setting, annotated and set to its default |
| [software.toml](software.toml) | Explicit software renderer alternative with a taller bar |

See the [configuration reference](../../docs/configuration.md) for types, defaults, ranges, dependencies, reload behavior, and validation commands. Vulkan needs a working driver; software mode does not count as GPU validation. Tests parse all these files and enforce setting coverage. Update examples and the reference alongside every feature/setting change.
