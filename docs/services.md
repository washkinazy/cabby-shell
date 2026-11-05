# Service ownership and control

This page describes the implemented service architecture. [Internal plugin design](plugins.md) describes the shared startup, built-in feature modules, and bar/wallpaper surface host now used by the UI executables. Bar and wallpaper now share the desktop process; notification ownership and on-demand launcher lifetime remain isolated.

## Components

The initial units are in `systemd/user/`. They target a niri session managed as `niri.service` (for example through niri-session). They are not enabled by builds/tests. Running niri without that service requires a deliberate adapter for its lifetime; do not install a dummy niri.service to bypass ownership.

| Unit | Responsibility |
| --- | --- |
| cabby.target | Starts/stops ordinary shell components; bound to niri.service |
| cabby-session.service | Configuration snapshot, private control socket, readiness notification |
| cabby-ui.service | Runs the desktop host with bar and wallpaper plugins |
| cabby-notifications.service | Notification bus owner and passive cards |
| cabby-launcher.service | On-demand application overlay; never auto-restarts after dismissal or failure |

The coordinator uses `Wants=` to start the desktop host and notifications with each coordinator activation, including crash recovery. Each component owns its bounded `Restart=on-failure` policy; continuous `Upholds=` is deliberately avoided because it can retry components after their restart limit. The UI waits for coordinator readiness, binds to it and niri, and belongs to cabby.target. The coordinator, desktop host, and notifications use cgroup cleanup, a five-second stop timeout, one-second restart delay, and at most three starts in thirty seconds. Crash loops eventually remain failed; status and journal output explain the failure. `systemctl --user reset-failed cabby-ui.service` permits retry after resolving the cause.

An active lock must be separate from cabby.target and bound to the compositor session. No production lock unit is shipped yet: Phase 5 supplies authenticated locking. The Phase 0 lock-probe must never be installed as a real locker. Lifecycle tests use an explicit sleep-process stand-in to validate this boundary.

Shared BlueZ/PipeWire services remain external dependencies. User applications need separate session-owned systemd units, not children in the UI cgroup. `cabby launch -- PROGRAM [ARG...]` supplies that path using a transient `app-cabby-*.service` bound to the compositor, independent of cabby.target. Shell stop/restart leaves it running; compositor loss stops its cgroup. This applies to the launched process tree; applications that hand off to an existing shared process retain that process's ownership.

## Runtime state

The coordinator uses `$XDG_RUNTIME_DIR/cabby-shell` (0700), including `control.sock` (0600), an advisory `owner.lock`, and a normalized `active.toml` snapshot. `--runtime-dir /absolute/path` is an explicit development/test override. User preferences remain in `$HOME/.config/cabby-shell/config.toml`; the runtime snapshot is never a second user configuration file.

A duplicate coordinator cannot acquire ownership or replace the existing socket. After a crash, the next owner can remove a stale socket. The systemd RuntimeDirectory removes runtime state on service stop. The coordinator sends READY only after validated config, snapshot, and IPC initialization.

Reload parses and validates the whole source file, atomically publishes a temporary-file rename, then commits the in-memory state. Failed parsing, file deletion, or failed snapshot publication leaves the previous in-memory configuration active. A changed configuration or wallpaper image queues component restarts when `--ui-unit`/`--wallpaper-unit`/`--notifications-unit` are supplied; `ui_restart_queued`, `wallpaper_restart_queued`, and `notifications_restart_queued` report whether systemd accepted the job, not whether rendering subsequently succeeded. An unchanged config and image do not restart these components. The shipped coordinator supplies only `--ui-unit` and `--notifications-unit`; `ui_restart_queued` covers both bar and wallpaper. The legacy development `--wallpaper-unit` option remains available for explicitly configured custom units, and `wallpaper_restart_queued` is null in the shipped layout. A standalone coordinator reports null for each component restart unit that was not supplied. Image validation precedes snapshot publication; failed image decoding also preserves the active configuration.

## Commands available now

```sh
cabby validate
cabby status
cabby reload
cabby start
cabby restart-ui
cabby restart
cabby stop
cabby launcher
cabby launch-app --desktop-id org.example.App.desktop
cabby launch -- foot
systemctl --user status cabby-session.service cabby-ui.service
journalctl --user -u cabby-session.service -u cabby-ui.service
```

`start` and `restart` operate on cabby.target; `restart-ui` restarts cabby-ui.service, including both bar and wallpaper. They require an active niri.service, as does `launch`, so an inactive session is reported instead of intentionally starting a compositor. `stop` works even after session loss. Unit definitions remain the authority for session binding. Development overrides `--target NAME.target`, `--ui-unit NAME.service`, and `--session-unit NAME.service` must match the staged unit relationships; they do not rewrite those relationships. See `cabby --help` for options accepted by each command. Unknown, duplicate, and irrelevant options fail.

`launch` requires `--` before the executable. Arguments are passed literally without shell or environment expansion. It preserves the working directory (or accepts `--working-directory PATH`) and forwards available display/session variables (WAYLAND_DISPLAY, DISPLAY, XAUTHORITY, XDG_RUNTIME_DIR, XDG_CURRENT_DESKTOP, NIRI_SOCKET); other environment comes from the user service manager. It returns JSON with the allocated unit name after successful execution startup, not application readiness. Applications can be inspected/stopped with systemctl using that name.

`status` and `reload` return one JSON object and exit unsuccessfully for failed requests. A failed UI restart enqueue also yields failure, while reporting that the configuration was accepted; inspect service status/journal for later startup failures. `status` reports the coordinator's config and PID, not health of every component. IPC client operations have a five-second timeout. The local IPC accepts newline-delimited `status` or `reload`, returns a JSON line, and bounds request size/read time. This is a private same-user control interface, not a network API. Do not log config source contents or future secrets.

For isolated development, `cabby serve --config /absolute/file.toml --runtime-dir /absolute/private/directory` runs the coordinator in the foreground. `--ui-unit NAME` queues restarts for an existing managed UI; it is not a command to execute arbitrary shell text. The production `desktop` wrapper locates `cabby-desktop` alongside `cabby`. The standalone `ui` wrapper still locates `cabby-platform-spike`; `launcher-ui` locates `cabby-launcher`. See [launcher.md](launcher.md) for discovery, keyboard controls, and launch semantics.

The packaged unit paths currently expect `/usr/bin/cabby`, `/usr/bin/cabby-desktop`, `/usr/bin/cabby-platform-spike`, `/usr/bin/cabby-launcher`, `/usr/bin/cabby-wallpaper`, and `/usr/bin/cabby-notifications`. No installer or package is provided at this phase, and nothing has been installed into the user's session. Use the test harness for current validation; packaging arrives later.

## Automated lifecycle test

```sh
cargo build --workspace --locked
python3 scripts/test_session_lifecycle.py
```

Requires the Fedora development package set, an accessible systemd user manager, and `setsid` from util-linux. This script rewrites the shipped units to unique `cabby-test-*` names, links them under the user's runtime-only unit directory, and uses stand-ins for compositor, the combined desktop, notifications, launcher, and lock. The real CLI/coordinator/IPC run unchanged and launch actual sleep/Python application processes into separate transient units. It does not start, stop, or modify the real niri.service or any installed Cabby units. A finally block stops test units, removes only its own runtime symlinks, resets failures, and reloads the manager. Results/journal excerpts remain under ignored `.artifacts/lifecycle-*`.

Coverage includes unit verification, repeated starts, coordinator ownership, invalid/deleted reload, valid reload and UI restart, independent UI crash, coordinator crash/recovery, restart limits, cgroup descendant cleanup, lock/app lifetime boundaries, compositor loss, CLI shell/UI restart, inactive-session refusal, literal launch arguments, on-demand launcher ownership, desktop-entry launch working directories, and wallpaper-only reload through the shared desktop and invalid-asset rollback. Actual UI graphics still use the separate nested-niri harness; stand-ins do not establish lock security or graphical application behavior.

See [wallpaper.md](wallpaper.md) for asset validation, relative paths, and image reload behavior.

Notification supervision uses coordinator `Wants=` activation dependencies and the component's own bounded crash recovery. `RestartPreventExitStatus=73` leaves an ownership conflict stopped until explicit retry; continuous upholding is not used. The component is session/coordinator-bound and part of the ordinary target. Changed reloads queue its restart through `--notifications-unit`, losing active/queued notices; invalid or unchanged reloads preserve them. `cabby status` remains coordinator status, not proof of notification ownership or component health. See [notifications.md](notifications.md).

## Desktop-host migration

`cabby-ui.service` now executes `cabby desktop`, composing bar and wallpaper in one process. A desktop crash, `restart-ui`, or accepted changed reload rebuilds both plugins together. Notification ownership remains independent of desktop restarts, and the launcher remains on-demand with no automatic reopening. This reduces the ordinary resident UI processes by one without adding a plugin daemon. The coordinator still owns validated runtime snapshots and control IPC; retaining it separates configuration readiness from rendering failures. Notifications retain their separate service so a renderer crash in the desktop does not lose active notices or release the notification bus name. The launcher retains its separate process to preserve its on-demand lifetime and keyboard/clipboard policy.

The old `cabby-wallpaper.service` is no longer shipped. Nothing has been installed automatically. If units from earlier revisions were installed manually, stop the old Cabby target, disable/remove the old wallpaper unit from the installation, replace the remaining shipped units, and reload the user systemd manager before starting the new target. Do not keep a legacy wallpaper service running alongside `cabby desktop`. No TOML migration is needed. Standalone `cabby ui` and `cabby wallpaper` remain development entry points and should not be started alongside the managed desktop on the same outputs.

The audio panel is owned by the desktop host in `cabby-ui.service`. Its snapshot, command, and subscription helpers run only while the panel is open and remain in that service's cleanup cgroup. Closing the panel cancels its work and reaps helpers; restarting the desktop also closes the panel. PipeWire/PulseAudio are external dependencies and are never stopped by Cabby. No additional audio service is installed.


Bluetooth uses the same on-demand desktop panel ownership as Audio, with one panel open at a time. Its private system-bus connection owns its BlueZ agent and discovery lease; panel dismissal cancels that connection, and desktop process loss closes it at the OS boundary. BlueZ releases caller-owned work. No additional production service is installed; Cabby never manages BlueZ's service lifecycle or assumes ownership of another client's discovery. See [bluetooth.md](bluetooth.md).
