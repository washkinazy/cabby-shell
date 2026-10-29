# Service ownership and control

## Components

The initial units are in `systemd/user/`. They target a niri session managed as `niri.service` (for example through niri-session). They are not enabled by builds/tests. Running niri without that service requires a deliberate adapter for its lifetime; do not install a dummy niri.service to bypass ownership.

| Unit | Responsibility |
| --- | --- |
| cabby.target | Starts/stops ordinary shell components; bound to niri.service |
| cabby-session.service | Configuration snapshot, private control socket, readiness notification |
| cabby-ui.service | Runs the workspace/window bar |
| cabby-launcher.service | On-demand application overlay; never auto-restarts after dismissal or failure |

The coordinator uses `Upholds=` to recover the UI after coordinator recovery. The UI waits for coordinator readiness, binds to it and niri, and belongs to cabby.target. Both services use cgroup cleanup, a five-second stop timeout, one-second restart delay, and at most three starts in thirty seconds. Crash loops eventually remain failed; status and journal output explain the failure. `systemctl --user reset-failed cabby-ui.service` permits retry after resolving the cause.

An active lock must be separate from cabby.target and bound to the compositor session. No production lock unit is shipped yet: Phase 5 supplies authenticated locking. The Phase 0 lock-probe must never be installed as a real locker. Lifecycle tests use an explicit sleep-process stand-in to validate this boundary.

Shared BlueZ/PipeWire services remain external dependencies. User applications need separate session-owned systemd units, not children in the UI cgroup. `cabby launch -- PROGRAM [ARG...]` supplies that path using a transient `app-cabby-*.service` bound to the compositor, independent of cabby.target. Shell stop/restart leaves it running; compositor loss stops its cgroup. This applies to the launched process tree; applications that hand off to an existing shared process retain that process's ownership.

## Runtime state

The coordinator uses `$XDG_RUNTIME_DIR/cabby-shell` (0700), including `control.sock` (0600), an advisory `owner.lock`, and a normalized `active.toml` snapshot. `--runtime-dir /absolute/path` is an explicit development/test override. User preferences remain in `$HOME/.config/cabby-shell/config.toml`; the runtime snapshot is never a second user configuration file.

A duplicate coordinator cannot acquire ownership or replace the existing socket. After a crash, the next owner can remove a stale socket. The systemd RuntimeDirectory removes runtime state on service stop. The coordinator sends READY only after validated config, snapshot, and IPC initialization.

Reload parses and validates the whole source file, atomically publishes a temporary-file rename, then commits the in-memory state. Failed parsing, file deletion, or failed snapshot publication leaves the previous in-memory configuration active. A changed configuration queues a UI restart when `--ui-unit` is supplied; `ui_restart_queued` reports whether systemd accepted the job, not whether rendering subsequently succeeded. An unchanged reload does not restart the UI. A standalone coordinator has no UI restart unit and reports null for that field.

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

`start` and `restart` operate on cabby.target; `restart-ui` affects only cabby-ui.service. They require an active niri.service, as does `launch`, so an inactive session is reported instead of intentionally starting a compositor. `stop` works even after session loss. Unit definitions remain the authority for session binding. Development overrides `--target NAME.target`, `--ui-unit NAME.service`, and `--session-unit NAME.service` must match the staged unit relationships; they do not rewrite those relationships. See `cabby --help` for options accepted by each command. Unknown, duplicate, and irrelevant options fail.

`launch` requires `--` before the executable. Arguments are passed literally without shell or environment expansion. It preserves the working directory (or accepts `--working-directory PATH`) and forwards available display/session variables (WAYLAND_DISPLAY, DISPLAY, XAUTHORITY, XDG_RUNTIME_DIR, XDG_CURRENT_DESKTOP, NIRI_SOCKET); other environment comes from the user service manager. It returns JSON with the allocated unit name after successful execution startup, not application readiness. Applications can be inspected/stopped with systemctl using that name.

`status` and `reload` return one JSON object and exit unsuccessfully for failed requests. A failed UI restart enqueue also yields failure, while reporting that the configuration was accepted; inspect service status/journal for later startup failures. `status` reports the coordinator's config and PID, not health of every component. IPC client operations have a five-second timeout. The local IPC accepts newline-delimited `status` or `reload`, returns a JSON line, and bounds request size/read time. This is a private same-user control interface, not a network API. Do not log config source contents or future secrets.

For isolated development, `cabby serve --config /absolute/file.toml --runtime-dir /absolute/private/directory` runs the coordinator in the foreground. `--ui-unit NAME` queues restarts for an existing managed UI; it is not a command to execute arbitrary shell text. The default UI wrapper locates `cabby-platform-spike` alongside `cabby`; `launcher-ui` locates `cabby-launcher`. See [launcher.md](launcher.md) for discovery, keyboard controls, and launch semantics.

The packaged unit paths currently expect `/usr/bin/cabby` and `/usr/bin/cabby-platform-spike`, and `/usr/bin/cabby-launcher`. No installer or package is provided at this phase, and nothing has been installed into the user's session. Use the test harness for current validation; packaging arrives later.

## Automated lifecycle test

```sh
cargo build --workspace --locked
python3 scripts/test_session_lifecycle.py
```

Requires the Fedora development package set, an accessible systemd user manager, and `setsid` from util-linux. This script rewrites the shipped units to unique `cabby-test-*` names, links them under the user's runtime-only unit directory, and uses stand-ins for compositor, bar, launcher, and lock. The real CLI/coordinator/IPC run unchanged and launch actual sleep/Python application processes into separate transient units. It does not start, stop, or modify the real niri.service or any installed Cabby units. A finally block stops test units, removes only its own runtime symlinks, resets failures, and reloads the manager. Results/journal excerpts remain under ignored `.artifacts/lifecycle-*`.

Coverage includes unit verification, repeated starts, coordinator ownership, invalid/deleted reload, valid reload and UI restart, independent UI crash, coordinator crash/recovery, restart limits, cgroup descendant cleanup, lock/app lifetime boundaries, compositor loss, CLI shell/UI restart, inactive-session refusal, literal launch arguments, on-demand launcher ownership, and desktop-entry launch working directories. Actual UI graphics still use the separate nested-niri harness; stand-ins do not establish lock security or graphical application behavior.
