# Cabby implementation plan

## Direction and status

The required feature set and exclusions are in README.md. Working direction: Rust application code, Iced UI, niri first, file-driven configuration, and systemd user supervision. Phase 0 must validate the Iced/Wayland combination before this becomes a committed technical baseline.

Multiple cooperating services are acceptable. A UI crash may leave healthy components running under systemd; a shell shutdown must clean up ordinary components, and compositor-session loss must clean up all Cabby components. An active lock is session-owned and survives an ordinary UI stop/restart. This exception must be explicit in commands and units.

Planning documents are prepared. No implementation phase has started, and no runtime capability has been verified locally. Local branches and validated merges into main are the delivery workflow; PRs are not required. Push policy is request-only for now.

## Decisions to settle at the relevant phase

- Phase 0: minimum Rust/niri/systemd versions; exact Iced and shell-binding revisions; renderer; development environment; supported hardware test environment.
- Phase 1: TOML as the proposed config format; validation and reload semantics; component/process split; service dependencies; restart limits; CLI/IPC contract; launcher application ownership outside Cabby's cgroups.
- Phase 2: bar layout and monitor selection, launcher matching/launch behavior, wallpaper fit modes. System tray support is not yet a committed requirement.
- Phase 3: notification history retention and markup policy; Bluetooth pairing scope; audio profiles, ports, and per-application routing scope.
- Phase 4: clipboard MIME types, size/history bounds, disk persistence and deletion. Proposed default is bounded in-memory history; persistence is opt-in.
- Phase 5: PAM integration, failed-lock recovery, manual and application inhibitor semantics, suspend/resume sequencing, night-light schedules and unsupported-output behavior.
- Phase 6: screenshot target modes, save location, and desired editing tools. Proposed initial scope is region and monitor capture, preview, copy, save, cancel. Annotation, OCR, uploads, and recording are not baseline requirements.

Decide these just before the affected implementation, documenting the reason and consequences here or in a focused design note. Do not silently convert proposals into requirements.

## Delivery phases

Each phase is split into small feature branches. The listed branch topics are examples, not branches to create in advance. Merge a complete, tested increment; do not wait for every feature in a phase to accumulate on one branch.

### Phase 0 — Prove the platform

Suggested topics: `chore/rust-workspace`, `feat/wayland-spike`, `test/nested-session`.

- Establish a reproducible Cargo workspace and pin compatible Iced/shell bindings.
- Run a minimal layer-shell bar and keyboard-focused popup under niri; verify opening, dismissal, and focus restoration.
- Exercise multiple outputs and scale changes, an independent session-lock surface in a disposable session, and a screenshot capture path.
- Inspect advertised protocols; report unavailable capabilities rather than assuming all niri versions match current documentation.
- Build a repeatable isolated test harness and document invocation/cleanup.

Exit: build/lint/tests pass; the listed experiments have recorded results and blockers; a repeatable smoke test exits without leftover processes. If a critical capability fails, document the finding and revisit the stack before expanding UI work.

### Phase 1 — Configuration and service ownership

Suggested topics: `feat/config`, `feat/session-lifecycle`, `feat/control-cli`.

- Implement typed config, defaults, useful parse errors, and atomic valid-config reload.
- Add `cabby` control commands and a small set of systemd user services grouped by a target.
- Bind services to the actual compositor service/session. Test dependency behavior on unexpected exit; `PartOf=` alone is not a crash detector.
- Set bounded shutdown and restart policies; prevent duplicate instances and stale IPC endpoints.
- Separate the lock lifecycle and user-launched application ownership from ordinary shell components.

Exit: start, stop, UI crash, service crash, repeated start, compositor crash, and failed reload behave as specified. Cgroups and sockets show no abandoned Cabby work. No persistent services are installed or enabled merely by building/testing.

### Phase 2 — Everyday shell surface

Suggested topics: `feat/bar`, `feat/launcher`, `feat/wallpaper`.

- Implement bar and niri workspace/window integration, shared Iced theme, launcher, and wallpaper.
- Use file-configured colors, typography, spacing, and layout; no settings GUI.
- Handle output arrival/removal, differing scale factors, and keyboard navigation.

Exit: a usable basic session; correct workspace actions and application launches; launching an app then stopping Cabby does not terminate the app; no idle rendering loop without a reason.

### Phase 3 — Notifications and device controls

Suggested topics: `feat/notifications`, `feat/audio`, `feat/bluetooth`.

- Provide the notification service, actions/replacement/expiry, and focused UI.
- Add audio device selection, volume/mute, and agreed routing/profile controls.
- Add Bluetooth discovery, pairing interaction, connect/disconnect, and removal.
- Reconnect to dependencies after restarts and expose useful unavailable/error states.

Exit: protocol/service integration tests pass; mock-driven UI states are exercised; real device operations are recorded in an agreed hardware test session. No duplicate notification owner or indefinite discovery remains after shutdown.

### Phase 4 — Clipboard workflow

Suggested topics: `feat/clipboard-service`, `feat/clipboard-picker`.

- Implement selection monitoring/ownership, bounded history, search, reselect, and clear.
- Set explicit MIME and size policies; support screenshot image transfer when image support lands.
- Avoid recording Cabby's own re-selections as an unbounded feedback loop. Honor available sensitive-content hints without claiming they detect every secret.

Exit: copying remains usable after the source application exits; reselect/paste works between real clients; large or unsupported offers are handled; persistence and deletion match the chosen policy.

### Phase 5 — Lock, idle, and night light

Suggested topics: `feat/session-lock`, `feat/idle-policy`, `feat/night-light`.

- Implement independent lock UI/authentication and explicit crash recovery behavior.
- Coordinate idle events, manual inhibition, application inhibition, display power, and system suspend notifications.
- Require compositor lock confirmation before a Cabby-initiated suspend; define behavior if locking fails or times out.
- Implement temperature scheduling, output restoration, hotplug/resume, and unsupported-output reporting.

Exit: isolated failure tests and real-session tests pass for lock/authentication, suspend/resume, inhibitors, display changes, and gamma. Killing the UI never unlocks the session. A killed locker must not expose the desktop; document how the user recovers access. Do not replace a working daily-use locker before these checks pass.

### Phase 6 — Screenshot GUI

Suggested topics: `feat/screenshot-capture`, `feat/screenshot-preview`.

- Capture without including Cabby's selection overlay; handle logical versus physical coordinates, monitor transforms, and scale.
- Provide selection, preview, copy/save/cancel and clear errors for failed capture/write.
- Keep captures out of logs and remove temporary files on cancellation and recovery.

Exit: verify region and monitor capture, mixed scaling, clipboard image paste, save failures, and cancellation. Tests demonstrate crop geometry; visual inspection confirms output and interaction quality. Additional editing tools require an explicit scope decision.

### Phase 7 — Daily-use readiness

Suggested topics: `chore/packaging`, `test/session-recovery`, `docs/setup`.

- Document dependencies, configuration, install/uninstall, niri keybindings, service control, and recovery.
- Exercise a full session lifecycle and dependency failures with all components enabled.
- Measure idle CPU/memory, startup, and launcher responsiveness on recorded hardware. Set budgets from measurements rather than inventing targets now.
- Document supported versions, known limitations, and fallback/recovery procedures.

Exit: all required features have acceptance evidence; packaging works in a clean environment; logout/crash leaves no Cabby-owned processes; an agreed daily-use trial has no unresolved blocking failures.

## How we test

### Tier 1 — Fast deterministic checks

Run formatting, compilation, Clippy, and Rust tests on code branches (commands in AGENTS.md). Test configuration/reload, idle state transitions, notification replacement/expiry, clipboard retention, launcher matching, screenshot geometry, and schedule calculations. Inject clocks and service interfaces to avoid timing-dependent sleeps and real hardware in unit tests.

### Tier 2 — Protocol and service integration

Use a private D-Bus session and fixtures for notifications and device backends. Exercise reconnect, cancellation, malformed input, unavailable services, and bounded resource use. Verify unit syntax with `systemd-analyze --user verify` against the staged units and their test dependencies. Syntax validation is not proof of runtime lifecycle behavior.

### Tier 3 — Isolated graphical/session tests

Use a nested niri session with dedicated sockets, config, cache/state, and test-specific systemd units, or a disposable VM with a real user service manager. Never attach destructive lifecycle tests to the host's `niri.service` or `graphical-session.target`. Ensure cleanup runs after success/failure and verify the test cgroups are empty.

Test focus, overlays, capture, process exits, IPC reconnection, and service restart with actual clients. Visual inspection complements behavioral assertions. A nested compositor may not expose the same gamma, suspend, or lock behavior as a real DRM session.

### Tier 4 — Explicit hardware/session acceptance

Use an agreed test session for real Bluetooth/audio, monitor hotplug, fractional scaling, gamma, suspend/resume, and PAM locking. Record environment and results. Preserve access through a recovery path before lock tests. Distinguish skipped, failed, and passed checks; mocks cannot satisfy hardware acceptance.

## Lifecycle acceptance matrix

| Event | Expected result |
| --- | --- |
| Start twice | One supervised instance of each enabled component |
| Ordinary UI crashes | Bounded restart; healthy services remain owned; active lock unaffected |
| One backend crashes | Defined restart/reconnect; other UI remains responsive |
| Explicit ordinary shell stop | Ordinary components and their children stop; active lock remains session-owned |
| Compositor exits or crashes | All associated Cabby services stop; no restart loop against a dead session |
| Rapid repeated crashes | Restart limit reached with actionable status/logs |
| Config becomes invalid | Previous valid config remains active; useful error reported |
| BlueZ/PipeWire/D-Bus disappears | No busy loop; unavailable state and defined reconnection |
| Output removed or scale changes | Surfaces and capture coordinates update; lock coverage remains correct |
| Suspend/resume | Lock ordering and inhibitor policy respected; state refreshes after resume |
| Launcher opens an application | Application has independent session ownership and survives ordinary Cabby stop |
| Test aborts | Cleanup removes test processes, units, sockets, and temporary state |

## Evidence and completion records

For each feature, add a brief record containing scope, branch, checks and outcomes, environment for graphical/hardware checks, and any remaining limitations. Keep summaries small; scripts and fixtures should reproduce the details. A merged scaffold is not a completed feature, and a successful compile is not proof of session behavior.

- Planning baseline: README requirements plus AGENTS.md and this plan. Documentation validation only; runtime checks are not applicable yet.
