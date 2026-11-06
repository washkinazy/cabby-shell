# Cabby Shell

**This was an experiment, archived**

Cabby Shell is a personal, simplified alternative to Noctalia and Dank shells. It provides the everyday shell features needed alongside a tiling or scrolling window manager, with a deliberately small scope.

This is a personal project in a public repository; competing with other shells or pursuing broad adoption is not its purpose. [Internal plugins](docs/plugins.md) are the intended design pattern for consistent, maintainable feature development within this repository. Existing features live in built-in plugin modules with shared startup. Bar and wallpaper share one desktop host and service. Launcher and notification plugins use host adapters with distinct lifecycle policies; audio and Bluetooth plugins provide passive status and share an on-demand device-panel slot in the desktop host.

## Design principles

1. **Configuration lives in files.** There is no settings app. Persistent preferences, layout, and styling are configured through documented text files.
2. **Use Rust/Iced styling.** The shell does not use GTK for its interface or theming. Phase 0 validated the pinned Rust/Iced stack documented in PLAN.md.
3. **Provide the essentials around the window manager.** Combine the basic desktop interactions into one coherent shell without taking ownership of unrelated system services.
4. **Use a solid Fluent-inspired appearance.** Opaque surfaces, restrained rounding, neutral controls and clear accent states form the shared [visual direction](docs/appearance.md). Bar status/actions use consistent outline icons with short, opaque hover hints; workspaces and date/time retain text. Existing configuration supplies the palette and layout; glass and backdrop blur are excluded.

## Scope boundaries

- The window manager owns window placement, tiling or scrolling, and workspace behavior. Cabby can expose controls and status for those features.
- Display managers and login tools are out of scope.
- Polkit agents and authorization engines are out of scope. Screen locking for an existing session is in scope; it does not extend to display managers or login tools.
- A graphical settings application is out of scope. Everyday controls such as adjusting volume are distinct from editing persistent shell configuration.

## Required features

The baseline is a niri environment without Noctalia or DMS. Cabby Shell should provide:

- Bar.
- Application launcher.
- Idle management with configurable lock, display-off and automatic-suspend deadlines, including Caffeine inhibition for manual/audio/fullscreen activity.
- Night light through color-temperature/gamma adjustment (Redshift-style functionality).
- Notifications.
- Wallpaper.
- Bluetooth controls.
- Audio device picker and audio management.
- Screen locking.
- A [session menu](docs/session-menu.md) for lock, suspend, log out, reboot and power off.
- Clipboard management.
- A basic screenshot GUI workflow.
- Passive bottom-center [OSD feedback](docs/osd.md) for volume/microphone and lock-key changes, with volume progress meters.

These features define the required scope; whether each is implemented directly or integrated through an existing service remains an implementation decision. Interactive controls belong in the shell, while persistent configuration remains file-driven.

Existing tools such as grim may provide backend functionality behind Cabby's Rust/Iced interface. Implementing every backend in Rust is not a requirement.

Each addition should serve a frequent desktop interaction and fit the boundaries above. The validated baseline uses TOML configuration and Rust/Iced with Vulkan rendering; tiny-skia is an explicit software fallback. Support for compositors beyond niri remains an open decision.

## Process ownership and lifecycle

Cabby may comprise multiple cooperating services. A single process is not a requirement, and process boundaries should follow useful isolation or lifecycle needs rather than assigning every feature its own daemon.

- Starting the shell starts its configured components under explicit supervision.
- Components may manage their own work, but must never become detached, unowned background processes.
- Shell or compositor crashes must not leave orphaned Cabby services. Every component must either remain under a live supervisor with a defined recovery policy or be stopped.
- Stopping the shell must provide coordinated cleanup of its components and their child processes.
- Ending or losing the compositor session must stop the associated Cabby components.
- Shared system services such as BlueZ and PipeWire are dependencies, not Cabby-owned processes.

The Linux implementation will use systemd user services with control-group cleanup and explicit dependencies on the compositor session. The initial coordinator and UI use bounded restart policies; see [service ownership and controls](docs/services.md). The lock service needs a separate lifecycle from the ordinary UI so a UI crash cannot unlock the session or discard the active lock interface.

## Development

User configuration defaults to `$HOME/.config/cabby-shell`, with subfiles/directories as needed. See the [configuration reference](docs/configuration.md) and [example configuration directory](examples/config/README.md). Every feature or setting change must keep the reference and complete example set current in the same branch.

See [PLAN.md](PLAN.md) for implementation phases, acceptance criteria, and testing strategy, and [AGENTS.md](AGENTS.md) for repository instructions and the branch/commit/merge workflow. Initial development uses validated feature branches merged into main without a required pull request.

Phases 0 through 3 are complete under the documented automated/synthetic acceptance scope: platform validation, configuration, service ownership, and control CLI. Phase 2 now includes an event-driven niri workspace/window bar with configurable styling. An on-demand [application launcher](docs/launcher.md) is also implemented. Static [wallpaper](docs/wallpaper.md) completes the Phase 2 surfaces. Phase 3 includes [notifications](docs/notifications.md): bounded in-memory policy, D-Bus service, passive Iced cards, and supervised ownership. An on-demand [audio panel](docs/audio.md) provides device defaults, volume, and mute within the desktop host. The [Bluetooth panel](docs/bluetooth.md) adds bounded discovery, pairing interaction, device connections, and confirmed removal. Phase 3 is complete under the automated/synthetic acceptance scope; real-device operations and physical multi-output behavior remain unverified. [Idle/lock](docs/locking.md) and night light are implemented under the documented automated/synthetic acceptance scope, with PAM authentication, an independent Iced lock screen, nested crash-recovery tests, and explicit lock-before-suspend through `cabby suspend`; automatic idle locking and display power are opt-in with `idle_lock_seconds` and `idle_display_off_seconds` and external sleep observation is opt-in with `lock_before_sleep`; [clipboard history](docs/clipboard.md) now runs in the coordinator. [Night light](docs/night-light.md) follows Noctalia-style scheduled/forced per-output gamma with a shared toggle command and bar indicator.

Use the [setup and recovery guide](docs/setup.md) for installation and daily operation, and the [readiness record](docs/readiness.md) for measured performance and remaining acceptance limits. Build prerequisites, the Fedora dependency installer, and isolated test commands are documented in [docs/development.md](docs/development.md).

## Reference tools

These are examples of tools previously used to assemble the environment. They describe the roles Cabby should cover, rather than prescribing dependencies or toolkits; the no-GTK principle still applies to Cabby's interface.

| Role | Previously used examples |
| --- | --- |
| Bar | Waybar |
| Idle inhibition | sway idle inhibitor |
| Notifications | Mako |
| Screen locking | gtklock |
| Wallpaper | Waypaper with swaybg |
| Launcher | fuzzle / Walker |
| Audio management | pavucontrol-qt |
| Bluetooth | Blueman |
| Night light | Redshift |

## Clipboard and screenshot workflow

Clipboard history is bounded and memory-only, supports UTF-8 text and PNG, and provides search, reselect, delete and clear through the desktop picker. Run `cabby clipboard`; the dotfiles bind Mod+Ctrl+V. See [clipboard policy and configuration](docs/clipboard.md).

Screenshots need a small but complete graphical workflow. Flameshot is more elaborate than desired, while sparse capture tools do not provide enough workflow. The goal is a useful middle ground with a focused interface.

[Screenshots](docs/screenshots.md) now support region or monitor capture, frozen-image selection, preview, copy, atomic PNG save and cancel in the desktop host. Use `cabby screenshot` or `cabby screenshot-monitor`; dotfiles bind Mod+Print and Mod+Shift+Print. Annotation, OCR, uploads and recording are outside the baseline.

Run `just deps` once to install Fedora dependencies, including Rust/Cargo and native development headers. After applying the local dotfiles setup, run `just install` to build/install Cabby and its user units, reload systemd, and validate configuration. Cabby owns the service files; dotfiles contain preferences, keybindings and session startup. `just uninstall` removes the tracked local install after stopping Cabby and unlocking normally. Use `just build` to build only.

The coordinator automatically reloads valid config and active wallpaper-file changes; invalid edits preserve the previous configuration. Ordinary reloads preserve panels and notifications; only renderer changes restart these processes. `cabby reload` remains available for immediate validation.

A [system tray](docs/tray.md) now hosts StatusNotifier items and application menus in the desktop process.

The default bar places workspaces left, local clock/date in the middle, and audio, network, Bluetooth, Caffeine, night light, clipboard and system tray on the right. Status/actions use compact icons with brief opaque hover hints. Scrolling over the workspace names switches to the previous/next workspace on that output, without wrapping. Configure their order and placement with `bar_left`, `bar_middle` and `bar_right`; Apps, window title and diagnostic status are optional widgets.

[Caffeine](docs/caffeine.md) combines manual, audio and fullscreen inhibition in the coordinator. Configurable effects cover idle, sleep and temporary night-light pause; the bar toggles the manual reason.
