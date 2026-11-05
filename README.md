# Cabby Shell

Cabby Shell is a personal, simplified alternative to Noctalia and Dank shells. It provides the everyday shell features needed alongside a tiling or scrolling window manager, with a deliberately small scope.

This is a personal project in a public repository; competing with other shells or pursuing broad adoption is not its purpose. [Internal plugins](docs/plugins.md) are the intended design pattern for consistent, maintainable feature development within this repository. Existing features live in built-in plugin modules with shared startup. Bar and wallpaper share one desktop host and service. Launcher and notification plugins use host adapters with distinct lifecycle policies; audio and Bluetooth plugins share an on-demand device-panel slot in the desktop host.

## Design principles

1. **Configuration lives in files.** There is no settings app. Persistent preferences, layout, and styling are configured through documented text files.
2. **Use Rust/Iced styling.** The shell does not use GTK for its interface or theming. Phase 0 validated the pinned Rust/Iced stack documented in PLAN.md.
3. **Provide the essentials around the window manager.** Combine the basic desktop interactions into one coherent shell without taking ownership of unrelated system services.

## Scope boundaries

- The window manager owns window placement, tiling or scrolling, and workspace behavior. Cabby can expose controls and status for those features.
- Display managers and login tools are out of scope.
- Polkit agents and authorization engines are out of scope. Screen locking for an existing session is in scope; it does not extend to display managers or login tools.
- A graphical settings application is out of scope. Everyday controls such as adjusting volume are distinct from editing persistent shell configuration.

## Required features

The baseline is a niri environment without Noctalia or DMS. Cabby Shell should provide:

- Bar.
- Application launcher.
- Idle management, including idle inhibitors.
- Night light through color-temperature/gamma adjustment (Redshift-style functionality).
- Notifications.
- Wallpaper.
- Bluetooth controls.
- Audio device picker and audio management.
- Screen locking.
- Clipboard management.
- A basic screenshot GUI workflow.

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

Phases 0 through 3 are complete under the documented automated/synthetic acceptance scope: platform validation, configuration, service ownership, and control CLI. Phase 2 now includes an event-driven niri workspace/window bar with configurable styling. An on-demand [application launcher](docs/launcher.md) is also implemented. Static [wallpaper](docs/wallpaper.md) completes the Phase 2 surfaces. Phase 3 includes [notifications](docs/notifications.md): bounded in-memory policy, D-Bus service, passive Iced cards, and supervised ownership. An on-demand [audio panel](docs/audio.md) provides device defaults, volume, and mute within the desktop host. The [Bluetooth panel](docs/bluetooth.md) adds bounded discovery, pairing interaction, device connections, and confirmed removal. Phase 3 is complete under the automated/synthetic acceptance scope; real-device operations and physical multi-output behavior remain unverified. Idle/lock is the next user-prioritized work; clipboard is deferred.

Build prerequisites, the Fedora dependency installer, and isolated Phase 0 test commands are documented in [docs/development.md](docs/development.md).

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

Clipboard management is part of the baseline shell. History behavior, supported content types, and persistence remain to be defined.

Screenshots need a small but complete graphical workflow. Flameshot is more elaborate than desired, while sparse capture tools do not provide enough workflow. The goal is a useful middle ground with a focused interface.

A proposed starting workflow is to choose a capture target, capture it, preview the result, and copy or save it. Capture modes and any editing or annotation features remain to be decided.

After applying the local dotfiles setup, run `just install` to build/install Cabby, reload user units, and validate configuration. Use `just build` to build only.
