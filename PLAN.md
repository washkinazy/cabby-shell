# Cabby implementation plan

## Direction and status

The required feature set and exclusions are in README.md. Working direction: Rust application code, Iced UI, niri first, file-driven configuration, and systemd user supervision. Phase 0 must validate the Iced/Wayland combination before this becomes a committed technical baseline.

Multiple cooperating services are acceptable. A UI crash may leave healthy components running under systemd; a shell shutdown must clean up ordinary components, and compositor-session loss must clean up all Cabby components. An active lock is session-owned and survives an ordinary UI stop/restart. This exception must be explicit in commands and units.

Phase 0 is complete under the revised automated/synthetic acceptance scope: pinned Rust/Iced, hardware Vulkan and explicit software rendering, nested capture/lock/scale checks, synthetic popup lifecycle tests, and supervised test cleanup are validated. Physical multi-monitor and compositor-delivered popup focus remain unverified as agreed. See docs/development.md for evidence and limitations. Phase 1 is complete: typed TOML configuration, documented examples, supervised coordinator/UI ownership, reload, control CLI, and independent application launch are validated. Phase 2 is in progress: the niri workspace/window bar is implemented; launcher and wallpaper remain. Local branches and validated merges into main are the delivery workflow; PRs are not required. Push policy is request-only for now.

## Decisions to settle at the relevant phase

- Phase 0: minimum Rust/niri/systemd versions; exact Iced and shell-binding revisions; renderer; development environment; supported hardware test environment.
- Phase 1 (settled): TOML config format; validation and reload semantics; component/process split; service dependencies; restart limits; CLI/IPC contract; launcher application ownership outside Cabby's cgroups.
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
- Run a minimal layer-shell bar and popup under niri. Cover opening/dismissal and focus behavior with automated/synthetic checks where practical; manual popup acceptance is waived, and any unexercised behavior is recorded as unverified.
- Exercise single-output scale changes, an independent session-lock surface in a disposable session, and a screenshot capture path. A wrapped tool such as grim satisfies the capture-path requirement; an all-Rust capture backend is not required.
- Model multiple outputs and geometry synthetically where practical. Physical multi-monitor testing is deferred because the available setup has one output; document assumptions and handle reported failures as fixes rather than blocking Phase 0.
- Inspect advertised protocols; report unavailable capabilities rather than assuming all niri versions match current documentation.
- Build a repeatable isolated test harness and document invocation/cleanup.

Exit: build/lint/tests pass; feasible automated/synthetic experiments have recorded results, assumptions, and coverage gaps; a repeatable smoke test exits without leftover processes. Manual popup and physical multi-monitor checks are not completion gates. If a critical capability fails, document the finding and resolve it or explicitly select a working fallback before expanding UI work. Unavailable tests remain unverified rather than being reported as passes.

### Phase 1 — Configuration and service ownership

Suggested topics: `feat/config`, `feat/session-lifecycle`, `feat/control-cli`.

- Implement typed config, defaults, useful parse errors, and atomic valid-config reload.
- Use `$HOME/.config/cabby-shell` as the default configuration root. Document any subfiles/directories, discovery rules, path resolution, and precedence in `docs/configuration.md`.
- Ship a complete annotated example set in `examples/config/` alongside the parser. Validate all runnable examples with the production parser/semantic validator and enforce documentation/example coverage for every setting using schema metadata.
- Add `cabby` control commands and a small set of systemd user services grouped by a target.
- Bind services to the actual compositor service/session. Test dependency behavior on unexpected exit; `PartOf=` alone is not a crash detector.
- Set bounded shutdown and restart policies; prevent duplicate instances and stale IPC endpoints.
- Separate the lock lifecycle and user-launched application ownership from ordinary shell components.

Exit: start, stop, UI crash, service crash, repeated start, compositor crash, and failed reload behave as specified. Cgroups and sockets show no abandoned Cabby work. No persistent services are installed or enabled merely by building/testing. Configuration documentation and examples cover every implemented setting and pass parser, semantic, and coverage checks.

### Phase 2 — Everyday shell surface

Suggested topics: `feat/bar`, `feat/launcher`, `feat/wallpaper`.

- Implement bar and niri workspace/window integration, shared Iced theme, launcher, and wallpaper.
- Use file-configured colors, typography, spacing, and layout; no settings GUI.
- Handle output arrival/removal, differing scale factors, and keyboard navigation.

Exit: automated/synthetic checks cover workspace actions and application launches; launching an app then stopping Cabby does not terminate the app; no idle rendering loop without a reason. Record any unverified physical-output behavior.

### Phase 3 — Notifications and device controls

Suggested topics: `feat/notifications`, `feat/audio`, `feat/bluetooth`.

- Provide the notification service, actions/replacement/expiry, and focused UI.
- Add audio device selection, volume/mute, and agreed routing/profile controls.
- Add Bluetooth discovery, pairing interaction, connect/disconnect, and removal.
- Reconnect to dependencies after restarts and expose useful unavailable/error states.

Exit: protocol/service integration tests pass; mock-driven UI states are exercised; real device operations are recorded if safely automatable in the available environment, otherwise explicitly unverified. No duplicate notification owner or indefinite discovery remains after shutdown.

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

Exit: feasible isolated automated failure tests pass for lock/authentication, suspend/resume policy, inhibitors, display changes, and gamma handling. Killing the UI never requests unlock. A killed locker must not expose the desktop; document the protocol guarantee, available test evidence, and recovery path. Untestable real-session behavior stays explicitly unverified; do not silently replace a working daily-use locker based only on mocks.

### Phase 6 — Screenshot GUI

Suggested topics: `feat/screenshot-capture`, `feat/screenshot-preview`.

- Capture without including Cabby's selection overlay; handle logical versus physical coordinates, monitor transforms, and scale.
- Reuse a capture tool/service where useful. Keep capture behind a Rust interface so grim or niri actions can supply pixels while Cabby owns its UI workflow; no all-Rust-backend requirement.
- Provide selection, preview, copy/save/cancel and clear errors for failed capture/write.
- Keep captures out of logs and remove temporary files on cancellation and recovery.

Exit: automated/synthetic tests verify region and monitor capture, scaling geometry, clipboard image transfer, save failures, and cancellation where the environment permits. Record untestable interaction and hardware behavior as coverage gaps; manual inspection is optional. Additional editing tools require an explicit scope decision.

### Phase 7 — Daily-use readiness

Suggested topics: `chore/packaging`, `test/session-recovery`, `docs/setup`.

- Document dependencies, configuration, install/uninstall, niri keybindings, service control, and recovery.
- Exercise a full session lifecycle and dependency failures with all components enabled.
- Measure idle CPU/memory, startup, and launcher responsiveness on recorded hardware. Set budgets from measurements rather than inventing targets now.
- Document supported versions, known limitations, and fallback/recovery procedures.

Exit: all required features have acceptance evidence; packaging works in a clean environment; logout/crash leaves no Cabby-owned processes; an agreed daily-use trial has no unresolved blocking failures.

## How we test

Automated and synthetic testing is the default. Physical multi-monitor tests and manual popup acceptance are waived for the available single-output setup. Mocked geometry/output events can validate application logic but do not prove physical behavior. Where automation is impractical or hardware unavailable, record assumptions and limitations and continue; do not invent evidence or retain impossible acceptance gates. Known reproducible failures still require a fix or an explicit supported fallback.

Every phase and feature branch that changes configuration must update `docs/configuration.md` and `examples/config/` in the same branch, including default/behavior changes, renames, and removals. This is a merge requirement throughout development, not a final documentation task. Reference entries include type, default, valid values/units, omission behavior, example, interactions, and reload requirements. Breaking changes include migration instructions.

### Tier 1 — Fast deterministic checks

Run formatting, compilation, Clippy, and Rust tests on code branches (commands in AGENTS.md). Test configuration/reload, idle state transitions, notification replacement/expiry, clipboard retention, launcher matching, screenshot geometry, and schedule calculations. Inject clocks and service interfaces to avoid timing-dependent sleeps and real hardware in unit tests.

### Tier 2 — Protocol and service integration

Use a private D-Bus session and fixtures for notifications and device backends. Exercise reconnect, cancellation, malformed input, unavailable services, and bounded resource use. Verify unit syntax with `systemd-analyze --user verify` against the staged units and their test dependencies. Syntax validation is not proof of runtime lifecycle behavior.

### Tier 3 — Isolated graphical/session tests

Use a nested niri session with dedicated sockets, config, cache/state, and test-specific systemd units, or a disposable VM with a real user service manager. Never attach destructive lifecycle tests to the host's `niri.service` or `graphical-session.target`. Ensure cleanup runs after success/failure and verify the test cgroups are empty.

Test focus, overlays, capture, process exits, IPC reconnection, and service restart with actual clients where automation is practical. Visual inspection is optional, not a recurring user task. A nested compositor may not expose the same gamma, suspend, or lock behavior as a real DRM session.

### Tier 4 — Optional hardware/session evidence

When hardware and safe automation are available, use an isolated or explicitly agreed session for real Bluetooth/audio, monitor hotplug, fractional scaling, gamma, suspend/resume, and PAM locking. Record environment and results. Preserve access through a recovery path before lock tests. Distinguish skipped, failed, and passed checks; mocks cannot establish hardware behavior. Unavailable hardware and manual tests are not completion gates, and do not justify requesting a manual test cycle from the user.

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
- Configuration contract: `docs/configuration-contract` documents the default root, reference/example layout, and ongoing merge checks. Whitespace and local-link checks pass; parser/example/schema validation starts with Phase 1 because no settings are implemented yet.
- Phase 0 initial increment (`feat/phase0-platform`): Cargo workspace/lockfile, Iced bar and popup prototype, guarded test locker, Fedora dependency installer, and private nested-niri harness. Rust fmt/check/Clippy/test/build pass (zero Rust unit tests). Software-rendered nested probes pass for advertised protocols, rendered bar, PNG capture through grim, scale-change survival, lock confirmation, and locker survival after a bar crash. GPU adapter initialization failed in the initial wgpu-only runs. Popup focus, multiple outputs, Rust capture, and full Phase 0 acceptance remain open; this increment does not complete Phase 0.
- Scope revision (`docs/capture-and-test-scope`): external capture helpers are permitted; the existing grim probe satisfies the capture-path experiment. Physical multi-monitor and manual popup acceptance are waived; automate or synthesize feasible checks and record remaining gaps. This supersedes the original increment's Rust-capture/manual/multi-monitor gates. GPU investigation and feasible automated coverage remain work for Phase 0; it is not marked complete by this documentation change.
- Phase 0 completion (`fix/phase0-gpu-validation`): inherited `WGPU_BACKEND=gl` explained the forced OpenGL selection; that backend fails presentation compatibility in this nested setup. Explicit Vulkan passes and logs RTX 4070 / DiscreteGpu for both bar and locker. The harness now defaults to strict hardware Vulkan and separately tests tiny-skia; CPU Vulkan adapters cannot count as GPU success. Three synthetic Rust tests cover duplicate popup opens, Escape/reopen, and stale close events; two Python tests validate selected-adapter evidence. Formatting/check/Clippy/test/build and both nested renderer runs pass, including scale acknowledgement and systemd inactive-state cleanup. No new Fedora packages. Baseline is Rust 1.98.1, Iced 0.14.0, iced_exwlshell 0.20.1, niri 26.04, systemd 262 on Fedora 45; older runtime versions are not claimed supported. Physical multiple outputs, real focus delivery, PAM, DRM lock security, gamma, and suspend remain documented gaps/future work rather than inferred successes.

- Phase 1 config (`feat/config`): TOML parser with renderer/bar-height settings consumed by the UI; fixed default path and explicit override; complete examples/reference; generated field metadata enforces setting coverage. Invalid/deleted reload candidates preserve the previous snapshot. Rust checks and both nested renderers pass; live reload awaits the control service. No new Fedora system packages.

- Phase 1 lifecycle (`feat/session-lifecycle`): coordinator READY notification, owned private IPC/runtime snapshot, atomic reload publication, managed UI restart, bounded systemd recovery and session dependencies. Shipped unit relationships pass verification and isolated runtime tests for duplicate owners, invalid/deleted reloads, UI/coordinator crashes, descendant cleanup, shell-stop versus session-stop boundaries, and restart limits. Compositor/UI/lock/app are labeled stand-ins; actual graphics retain Phase 0 coverage. Runtime-only fixture units are removed after testing. Added util-linux to the Fedora installer for setsid. Control commands and application launch integration follow on the next branch.

- Phase 1 completion (`feat/control-cli`): strict start/stop/restart/restart-ui/status/reload/launch controls, active-session preflight, and literal-argument launches in independent compositor-bound transient services. Required Rust checks pass (seven tests), Python adapter tests pass (two), and nine isolated lifecycle scenarios cover command controls, app survival across shell stop/restart, session-loss cleanup, reload failures and crash recovery. Both nested Vulkan and tiny-skia runs exercise the cabby ui wrapper. No new Fedora packages. UI remains the platform prototype; authenticated locking and complete desktop features belong to later phases. No persistent units installed or enabled.

- Phase 2 bar (`feat/bar`): per-output workspace buttons ordered by compositor index, stable-ID workspace actions, active-window titles, urgency markers, disconnection/error states, bounded asynchronous IPC and two-second reconnect delay. All-output layer surfaces follow runtime output associations; top/bottom placement, spacing, text size, colors, and title visibility are documented with complete examples. Uses the system sans-serif font and a fixed workspace/title/status section order; arbitrary module placement and font-family selection are deferred. Keyboard workspace navigation remains with niri bindings, so the bar never steals keyboard focus. Original popup experiment retained as a separate probe. Required Rust checks, synthetic output/title/protocol tests, nested Vulkan/top and software/bottom probes, and existing lifecycle checks pass. Nested IPC verifies stable-ID action acknowledgement and focus events; physical multi-output/hotplug, pointer delivery, real-window title rendering, and idle resource measurements remain unverified. No new Fedora packages. This completes the bar increment, not all of Phase 2.
