# Cabby Shell

**Archived.**

A Wayland shell for niri, built with Rust and Iced. Configuration uses TOML files; systemd user services manage the components.

## Features

- Bar with workspaces, clock, status icons and a system tray.
- Application launcher, notifications and wallpaper.
- Audio and Bluetooth controls.
- Screen locking, idle display-off and automatic suspend.
- Caffeine inhibition and scheduled night light.
- Clipboard history and region/monitor screenshots.
- Session menu for lock, suspend, log out, reboot and power off.
- Volume, microphone and lock-key OSD feedback.

## Installation

From the checkout on Fedora, with niri running under `niri.service`:

```sh
bash scripts/install-fedora-deps.sh
just install
~/.local/bin/cabby start
```

The installer places binaries in `~/.local/bin` and installs the systemd user units. It preserves existing configuration.

Before using locking or suspend, install the Fedora PAM policy:

```sh
just install-lock-support
```

To update a running installation:

```sh
just install
~/.local/bin/cabby restart
```

## Documentation

- [Configuration reference](docs/configuration.md) and [examples](examples/config/README.md). The default file is `~/.config/cabby-shell/config.toml`; valid edits reload automatically.
- [Services and installation ownership](docs/services.md).
- [Locking and idle behavior](docs/locking.md).
- [Build requirements and development](docs/development.md).
- [Architecture](docs/plugins.md).
