# Install and operate Cabby

The tested baseline is Fedora with niri managed by `niri.service`, a systemd user manager and a working Vulkan driver. Cabby supplies shell services alongside niri. The [development guide](development.md) lists the full Fedora package set; the [readiness record](readiness.md) identifies tested versions and acceptance limits.

## Install or update

From the Cabby checkout:

```sh
just deps
just install
```

If just is missing, run `bash scripts/install-fedora-deps.sh` first. After subsequent source updates, `just install` rebuilds and updates the local installation. It installs eight binaries in `$HOME/.local/bin` and six user units, reloads systemd and validates configuration. It preserves existing preferences and starts/restarts no services. See [installation ownership](services.md#local-install-ownership) for overrides and migration from dotfiles-owned units.

For a running niri session, use the installed command directly:

```sh
~/.local/bin/cabby start
```

After updating an already running Cabby, `~/.local/bin/cabby restart` activates the new ordinary components. An active lock remains independent; its new binary takes effect on the next lock. Dotfiles session startup can run `$HOME/.local/bin/cabby start` once the niri session environment has been imported by its normal session wrapper. Cabby requires the real active `niri.service`; a standalone niri process needs a deliberate lifecycle adapter. Do not create a dummy service to bypass that check. Competing notification/tray owners should be removed from session startup before switching.

## Preferences and controls

The default user file is `$HOME/.config/cabby-shell/config.toml`. Missing files use built-in defaults; installation does not create or replace the file. Copy settings from the [examples](../examples/config/README.md), using the [complete reference](configuration.md). Valid edits reload automatically; invalid edits preserve the last accepted configuration. `cabby validate` checks the file without applying it; `cabby reload` requests immediate validation/publication.

| Control | Command | Current dotfiles binding |
| --- | --- | --- |
| Launcher | `cabby launcher` | Mod+D |
| Clipboard history | `cabby clipboard` | Mod+Ctrl+V |
| Region screenshot | `cabby screenshot` | Mod+Print |
| Monitor screenshot | `cabby screenshot-monitor` | Mod+Shift+Print |
| Night light | `cabby nightlight-toggle` | Mod+Shift+G |
| Caffeine | `cabby caffeine-toggle` or bar button | Bar button |
| Lock | `cabby lock` | Mod+Escape |
| Lock and suspend | `cabby suspend` | Mod+Shift+Escape |

Bindings are compositor preferences; Cabby does not install or rewrite them. Clipboard is memory-only and bounded. Screenshot pixels remain in memory until explicit Save; Copy uses the coordinator clipboard owner. Default bar regions are workspaces left, clock/date middle and audio/network/Bluetooth/Caffeine/night-light/tray right. Optional clipboard/screenshot buttons can be added through `bar_left`, `bar_middle` or `bar_right`.

Automatic idle locking, display power, external-sleep locking and audio/fullscreen Caffeine sources are opt-in. The personal dotfiles select idle lock after 600 seconds, display-off after 660 seconds and lock-before-sleep. Before applying that configuration and the lock bindings, run `just install-lock-support` from the Cabby checkout to install the Fedora PAM policy, then `chezmoi apply`. The policy task needs sudo and does not acquire a lock. Night light needs configured policy and gamma support. Follow [locking](locking.md), [Caffeine](caffeine.md) and [night light](night-light.md) for these interactions. Real-user PAM and physical lock/display/sleep behavior have separate validation limits.

## Diagnose and recover

```sh
~/.local/bin/cabby status
systemctl --user status cabby-session.service cabby-ui.service cabby-notifications.service
journalctl --user -b -u cabby-session.service -u cabby-ui.service -u cabby-notifications.service
```

If an edit is rejected, correct the source file and validate/reload it. If a component exhausts its bounded restart budget, fix the reported cause, then reset that component and start it again, for example:

```sh
systemctl --user reset-failed cabby-ui.service
systemctl --user start cabby-ui.service
```

`cabby restart-ui` restarts the shared bar/wallpaper/panel host. `cabby restart` restarts ordinary shell components. Both discard unsaved screenshot previews; coordinator restart also discards clipboard history and runtime manual Caffeine state. `cabby stop` stops ordinary components while an active lock stays session-owned. Dependency loss reports unavailable status and reconnects with bounded work; ensure the real PipeWire/PulseAudio, BlueZ or NetworkManager service is healthy before retrying controls. Cabby does not own those shared services.

If a locker fails after acquisition, the compositor remains locked and systemd attempts bounded recovery. Exhausted locker recovery requires authenticating on another VT and ending the affected graphical session. Never treat stopping a lock process as an unlock path. See the [lock recovery contract](locking.md) for details and unverified hardware behavior.

To remove the local install, first unlock normally and stop Cabby, then run `just uninstall`. It refuses active/unknown unit states and modified owned files, preserves preferences/overrides and the administrator-owned PAM policy, and does not modify session bindings/startup. Remove the Cabby session-start command from your compositor preferences when retiring it.
