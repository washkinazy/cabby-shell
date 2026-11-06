# Development and validation

## Fedora prerequisites

Install the development and test packages with:

```sh
just deps
```

If `just` is not installed yet, run `bash scripts/install-fedora-deps.sh` directly; it also installs just.

Inspect the package list without installing anything:

```sh
bash scripts/install-fedora-deps.sh --print
```

The script uses DNF and requests sudo when needed. It installs packages only; it does not enable services or install Cabby. Keep it updated with every build/test dependency change.

| Packages | Purpose |
| --- | --- |
| cargo, rust, rustfmt, clippy | Compiler, dependency/build tool, formatter, linting |
| curl | Bounded opt-in HTTPS location lookup for night light |
| just | Build/install task runner (`just build`, `just install`) |
| gcc, pkgconf-pkg-config | Native linker and library discovery |
| wayland-devel, libxkbcommon-devel | Wayland and keyboard development libraries |
| niri, wayland-utils | Nested compositor and protocol inspection |
| grim | Production screenshot capture and nested graphical capture checks |
| wl-clipboard | Private native clipboard source/consumer acceptance; production monitoring/ownership is native |
| dbus-daemon, systemd | Private D-Bus session and service lifecycle tooling |
| python3 | Isolated-session and lifecycle harnesses |
| pulseaudio-utils | pactl audio snapshots, event subscription, and controls |
| pam | Linux-PAM runtime plus permit/deny modules for private authentication tests; no real-user authentication is performed |
| NetworkManager | Read-only network status/events for the bar; never enabled by the installer |
| bluez | System Bluetooth service/API dependency; never enabled by the installer |
| util-linux | setsid for detached-descendant cleanup tests |
| mesa-dri-drivers, mesa-vulkan-drivers | Mesa rendering support, including software fallback where available |

Initial environment: Fedora 45, Rust/Cargo 1.98.1, niri 26.04, systemd 262, Wayland 1.26.0, libxkbcommon 1.13.1. Other distributions/versions are not yet validated. Existing proprietary GPU installations may supply a different Vulkan driver.

## Rust baseline

The workspace contains configuration, the control coordinator/CLI, launcher discovery, notification policy/transport, audio and Bluetooth backends, and the `cabby-shell` library. Production bar/launcher/wallpaper/notification/audio/Bluetooth UI lives in `crates/shell/src/plugins`, selected through shared host startup. UI executables are thin wrappers in `crates/shell/src/bin`, with existing names preserved. The platform-spike crate now contains only test probes; guarded popup/lock probes remain test-only. See [internal plugins](plugins.md) for responsibility boundaries and the next composition decisions. Phases 0–6 are complete under the recorded acceptance scope; the required feature implementations are present. Phase 7 automated packaging and measured release behavior are recorded in [readiness.md](readiness.md), with sustained daily use and physical validation still unverified. Direct dependencies are pinned to Iced 0.14.0 and iced_exwlshell 0.20.1; Cargo.lock fixes transitive dependencies. This is a selected validation baseline, not a claim that these are the newest versions. The declared Rust floor is 1.98; 1.98.1 supplied the original graphical baseline, and 1.99.1 passes the subsequent workspace checks documented below. Iced's wgpu and tiny-skia software backends are compiled with Wayland enabled; GTK and automatic desktop-theme detection are not used.

```sh
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build --workspace --locked
cargo build -p cabby-bluetooth --example bluez-fixture --locked
cargo build -p cabby-tray --example tray-fixture --locked
python3 scripts/phase0-session.py
python3 scripts/phase0-session.py --renderer software
python3 -m unittest discover -s scripts -p 'test_*.py'
```

Synthetic Rust tests cover duplicate popup opens, Escape dismissal/reopening, and delayed close events. Python tests check that renderer evidence comes from the selected adapter, not merely an enumeration of available devices. These tests do not prove real keyboard focus delivery. The production config parser supplies renderer, layout, and theme settings to the bar; see [configuration.md](configuration.md).

## Isolated runtime harness

Run inside an existing Wayland session with a working systemd user manager. The harness opens a nested niri window using a private runtime/config/cache/state/data tree and a private D-Bus session. It never uses niri's `--session` flag, imports environment into the user's service manager, or connects the locker to the host display. A uniquely named transient `cabby-phase0-*.scope` owns the disposable session, including descendants that detach from process groups. The harness stops this scope on success, failure, or timeout, checks its inactive state, and reaps its wrapper. A six-minute systemd runtime limit also bounds cleanup if the harness itself is killed. No persistent unit is installed.

Results live in an ignored `.artifacts/phase0-*` directory: protocol inventory, output/layer snapshots, a nested-only PNG, client logs, session log, and a JSON result listing both checks and unproven capabilities. These are developer artifacts, not installed user configuration. Protocol traces can contain typed test input; use synthetic text only. They remain locally for debugging and are not committed.

The automatic run verifies bar mapping, a PNG capture through grim, survival after a scale change, compositor lock confirmation, and locker survival after the bar is killed. It then destroys the disposable session. The lock probe intentionally has **no authentication** and must never be installed as a real locker. Its environment guard is an accident-prevention measure, not a security boundary.

Optional developer inspection (not a required test or user task):

```sh
python3 scripts/phase0-session.py --interactive
```

Optionally inspect the bar and its workspace buttons. The original popup experiment is retained separately as `target/debug/popup-probe`; it is no longer the production bar. Create `request-lock` in the printed artifact directory to continue to the lock probe. Click Release test lock to finish. The interactive run times out after five minutes; it does not automatically mark manual observations as passed.

Phase 0 now prioritizes automated/synthetic checks. Manual popup acceptance and physical multi-monitor tests are waived for the available single-output setup. Test application state and geometry synthetically where useful and record unexercised focus/physical behavior as unverified. The existing grim capture satisfies the capture-path experiment; a Rust-native backend is not required. Hardware Vulkan rendering now passes; retain the documented limits on real input delivery and untested outputs. Real DRM lock security, PAM authentication, gamma, and suspend/resume remain distinct from nested-session evidence.

The harness's `not_proven` entries are coverage disclosures, not automatically phase gates. `multiple-outputs` remains a documented physical-validation gap; native Rust capture is not a requirement.

## Screenshot backend direction

Niri 26.04 implements screenshots inside the compositor. Its screenshot UI copies/maps captured renderer textures; it does not wrap grim. The available actions are:

```sh
niri msg action screenshot
niri msg action screenshot-screen --path /absolute/path/capture.png
niri msg action screenshot-window --path /absolute/path/capture.png
```

These are reference commands, not tests to run against the user's live display. `screenshot` opens niri's interactive selector; screen/window actions capture their respective targets and also update the clipboard. The local 26.04 CLI accepts an absolute output path, and the window action supports a window ID. Action acknowledgement must not be assumed to mean asynchronous file writing has completed.

For Cabby's own selection/preview/copy/save flow, wrapping grim is a reasonable initial backend: it accepts a region or output and can write the capture to a file (or stdout using `-`). Niri actions remain an alternative when its built-in selection UI or window targeting is useful. Backend choice is separate from the Iced UI; use a narrow Rust interface and test helper failures, timeout/cancellation, result decoding, and temporary-file cleanup. Capture helpers must remain in the invoking service's cgroup. The full production wrapper belongs to Phase 6.

References: [niri screenshot actions](https://niri-wm.github.io/niri/Configuration:-Key-Bindings.html#screenshot-screenshot-screen-screenshot-window), [niri 26.04 screenshot implementation](https://github.com/niri-wm/niri/blob/v26.04/src/ui/screenshot_ui.rs), and locally verified `niri msg action screenshot-screen --help` / `grim -h`.

## Sources used for the prototype

- [iced_exwlshell 0.20.1 API and examples](https://docs.rs/iced_exwlshell/0.20.1/iced_exwlshell/)
- [niri nesting and CLI options](https://niri-wm.github.io/niri/Getting-Started.html)

The validated platform baseline is Rust/Iced with hardware Vulkan, an explicit software fallback, and systemd-owned component lifecycles. Phase 1 added production configuration and supervised services; Phase 2 added the bar, launcher, and wallpaper.

## Renderer diagnosis and results (2026-10-28)

The inherited environment contains `WGPU_BACKEND=gl`. Consequently, the initial runs selected only OpenGL, not wgpu's unrestricted adapter selection. Diagnostics identify the NVIDIA OpenGL adapter but report an incompatible presentation surface. This OpenGL path remains unsupported in the tested nested setup; we have not modified the driver or global environment.

Explicit Vulkan selection succeeds. Both bar and lock probe select **NVIDIA GeForce RTX 4070 / Vulkan / DiscreteGpu** and pass the nested smoke test, including buffer submission, capture, scale change, lock confirmation, bar-crash isolation, and inactive systemd scope after cleanup. This establishes a working GPU path on this machine, not universal driver compatibility. The explicit tiny-skia run also passes; no performance comparison has been made.

The harness defaults to `--renderer gpu`, which sets `ICED_BACKEND=wgpu`, `WGPU_BACKEND=vulkan`, and `CABBY_RENDERER_DIAGNOSTICS=1` only for its probe children. It records the selected adapter for both clients and rejects CPU adapters or silent software fallback. `--renderer software` explicitly selects tiny-skia. Renderer flags are development harness options, not persistent Cabby configuration. The bar reads config and re-execs with the selected backend when inherited renderer variables disagree. The standalone lock probe still uses upstream environment selection; the harness sets it explicitly.

Diagnostics use the existing Rust log ecosystem and filter to Iced/wgpu renderer messages. `CABBY_RENDERER_DIAGNOSTICS` enables these messages when present; leave it unset to disable them for direct runs. The Fedora installer needs no new system packages for this change. The renderer setting now has a full entry and examples in the configuration reference.

COSMIC's [libcosmic documentation](https://github.com/pop-os/libcosmic#cargo-features) describes an Iced-based toolkit with software rendering by default and optional wgpu GPU rendering. Its [manifest](https://github.com/pop-os/libcosmic/blob/master/Cargo.toml) exposes that selection. This is the application toolkit, distinct from COSMIC's compositor renderer. Cabby similarly keeps both renderer paths available, but tests GPU and software separately instead of treating fallback as GPU success.

## Coordinator lifecycle

See [services.md](services.md) for systemd units, IPC, and the isolated lifecycle test. Run `python3 scripts/test_session_lifecycle.py` after building. Tests stage only uniquely named runtime units and remove them afterward; no production service is installed.

## Bar integration

The nested harness now runs the production bar (`cabby ui`, retaining the historical `cabby-platform-spike` executable name). It configures two named workspaces, then runs the guarded `bar-ipc-probe` using the same Rust IPC client as workspace buttons. The probe receives the snapshot, switches by stable workspace ID, and waits for focus confirmation. Vulkan tests the top edge; software tests the bottom edge. `capture.png` and `bar-ipc-probe.log` record rendering and action results. Rust tests cover multi-output reassignment, stale IDs, titles, disconnection/reset, future event types, rejected actions, socket closure, and oversized frames. No new Fedora system packages are needed; the lockfile pins the added Tokio socket support.

Workspace/window data uses the [official niri JSON IPC protocol](https://github.com/niri-wm/niri/wiki/IPC). Cabby deserializes only consumed fields, ignores unrelated event types, and reconnects with fresh snapshots after invalid known events or disconnects. Socket tests require permission to bind local Unix sockets. Physical hotplug, multiple displays, actual pointer delivery, and real-window title rendering remain unverified; output/title behavior has synthetic coverage. No frame timer or periodic niri subprocess runs while connected; performance budgets await measurement.

## Launcher validation

See [launcher.md](launcher.md) for scope and controls. `cargo test --workspace --locked` includes desktop-entry fixtures and search/keyboard policies; `python3 scripts/test_session_lifecycle.py` also covers the shipped on-demand launcher unit and real desktop-entry app ownership. The nested renderer harness now captures `launcher.png` and `launcher.log`, observes `wl_keyboard.enter`, and requires hardware Vulkan selection for bar, launcher, and lock probe. XDG_DATA_DIRS is preserved for the graphical test because it also locates Vulkan driver manifests; fixture applications are added under the private data home and system application metadata may also appear. No installed application is launched by that graphical test. The lifecycle app fixture uses isolated discovery directories.

Dependencies added are Rust crates only: pinned freedesktop-desktop-entry with gettext disabled and Tokio process support. The Fedora install script's system package set remains sufficient; optional terminal programs are user choices.

## Wallpaper validation

The Rust config crate pins image 0.25.10 with PNG/JPEG codecs only; Iced enables its image widget without default codecs. There are no new Fedora system packages, so the installer package list is unchanged. `cabby validate --config examples/config/wallpaper.toml` checks the bundled synthetic PNG and config-relative path resolution. The example coverage test now loads each example and validates its assets as well as its TOML.

The nested harness starts `cabby wallpaper` on a private compositor, captures solid/fill/fit/stretch/center results, and uses `wallpaper-pixel-probe` to compare expected pixels. It also checks center placement after scale 1.5 and records the selected wallpaper GPU. Results are `wallpaper-*.png`, `wallpaper-*-pixels.txt`, and `wallpaper-*.log` under ignored artifacts. Runtime fixtures verify the shipped wallpaper unit alongside bar/launcher/session ownership. See [wallpaper.md](wallpaper.md) for limits; these tests do not establish physical multi-monitor behavior or performance budgets.

## Notification policy validation

`cargo test -p cabby-notifications --locked` runs deterministic policy tests without a compositor or D-Bus connection. The crate uses only the Rust standard library and introduces no new Fedora packages or configuration settings. Workspace merge checks still apply. Private D-Bus and graphical acceptance belong to subsequent notification increments.

The policy increment uses Rust/Cargo 1.99.1 on Fedora 44, with matching rustfmt/Clippy. The earlier Rust 1.98.1 / Fedora 45 graphical evidence remains historical; this toolchain check does not establish new GPU or compositor compatibility. On this environment, rustfmt, Clippy, libxkbcommon-devel and wayland-devel were downloaded as Fedora RPMs and extracted under `/tmp/cabby-dev-tools/root` for validation because system installation required a password. `PATH` selected that directory's `usr/bin`; `PKG_CONFIG_PATH` selected its relocated `usr/lib64/pkgconfig` files, with development linker symlinks resolving the installed native libraries. No system packages or live services were changed. Normal development should install the already documented package set using `scripts/install-fedora-deps.sh`.

## Notification D-Bus validation

`cargo test -p cabby-notification-service --locked` runs the private-bus integration suite (also included in the standard workspace test command). It requires the existing `dbus-daemon` package and local Unix-socket permissions. Every test owns a foreground daemon at a unique temporary address, waits for readiness with a timeout, and kills/reaps it on exit. No service is installed or enabled, and no test consults the ambient session-bus address. Systemd and graphical tests are not applicable to this adapter-only increment.

The adapter pins zbus 5.19.0 with the Tokio backend, uses the existing pinned Tokio/Serde combination, and adds futures-util as a direct dependency. The lockfile fixes transitive versions. No new Fedora development package is needed; `scripts/install-fedora-deps.sh --print` already includes dbus-daemon. See [notifications.md](notifications.md) for queue limits, the distinction between application and raw transport size bounds, and remaining UI/supervision work.

## Notification UI validation

The standard workspace build now produces `cabby-notifications` and the guarded `notification-probe`. The existing nested harness starts the frontend on its private bus, sends only synthetic text, checks card rendering/replacement/queueing/expiry and duplicate-owner exit, and verifies passive keyboard behavior. It records `notifications.log`/`notifications.png` and the selected adapter. Run both `python3 scripts/phase0-session.py` and `python3 scripts/phase0-session.py --renderer software`. The lifecycle harness stages the new unit with an explicit stand-in and tests reload, bar independence, crash recovery, conflict exit/retry, and cleanup. New UI/model tests cover stale revision/surface events and synthetic output geometry. No new Fedora packages are required; wayland-utils remains part of the documented prerequisite set.

## Plugin surface-host validation

`cargo build --workspace --locked` builds `cabby-desktop`; the nested harness exercises it through the production `cabby desktop` wrapper. The standard nested harness runs the bar and wallpaper separately first, then replaces them with both plugins in one surface host. It records `composition.log` and captures at scale 1 and 1.5, verifies bar/background pixels, checks workspace IPC, and requires a selected hardware Vulkan adapter in the GPU run. The software run exercises the same composition. Synthetic Rust tests cover per-output creation, stable IDs on metadata updates, scoped removal, stale cleanup, and layer/input policies. No new dependencies, settings, or installed units are needed. The shipped UI unit now hosts bar and wallpaper together; see [plugins.md](plugins.md).

Launcher plugin tests also exercise typed close requests versus recoverable launch errors. The existing nested launcher focus checks and isolated on-demand unit tests validate its host adapter.

Notification plugin tests cover bounded visible snapshots, revision replacement and typed fatal outcomes; surface tests now live under `host::notifications`. The existing private-bus suite and nested card/expiry/conflict tests cover the unchanged protocol and host integration.

Production UI binaries can be built independently with `cargo build -p cabby-shell --bins --locked`; the coordinator/CLI is still the `cabby` package. Use the workspace build for the complete isolated test harness, which also needs the probe binaries. No executable path, configuration, or unit changes accompany this source-package move.

## Audio backend validation

See [audio.md](audio.md) for scope, bounds and isolated fixture tests. The runtime helper is pactl from `pulseaudio-utils` (tested format 17.0). Python fixture helpers are launched through explicit paths; tests never invoke the live helper for mutations. The installer lists the prerequisite but does not enable audio services.

The audio panel uses the existing desktop host. `cargo test -p cabby-shell plugins::audio --locked` exercises the worker with copied private fixtures; consistent process visibility is needed for helper-reaping assertions. The standard nested GPU/software harness now runs `audio-panel-probe`, which requires `CABBY_TEST_WAYLAND` and an absolute `CABBY_TEST_AUDIO_HELPER` pointing to the private fixture. It captures `audio.png`/`audio.log`, verifies focus/rendering, and checks that helpers disappear on panel dismissal while the desktop stays alive. These fixture variables are test-only, not production configuration overrides.


Bluetooth adds the Fedora `bluez` runtime package; `scripts/install-fedora-deps.sh --print` includes it without enabling services. The Rust backend reuses pinned zbus/Tokio dependencies. `cargo test -p cabby-bluetooth --locked` runs only private D-Bus fixtures, including a real 60-second discovery expiry test; allow at least 70 seconds. Process/socket isolation permissions are needed as for the existing service tests. Before graphical tests also run `cargo build -p cabby-bluetooth --example bluez-fixture --locked
cargo build -p cabby-tray --example tray-fixture --locked`. Both renderer harness modes now exercise a Bluetooth pairing prompt and owned discovery cleanup on the disposable session bus. Production never reads `CABBY_TEST_BLUEZ_BUS`; only the guarded test entrypoint uses it. No real adapter state changes are part of acceptance. See [bluetooth.md](bluetooth.md) for protocol bounds and hardware limitations.

`just build` builds the eight production binaries in release mode. `just install` installs them in `$HOME/.local/bin`, installs/updates Cabby's user units in the standard per-user vendor directory, reloads systemd, and validates existing configuration. Dotfiles supply preferences/keybindings/startup, not shipped units. `just uninstall` removes the tracked local install after Cabby is stopped/unlocked; it preserves configuration and overrides. See [installation ownership](services.md#local-install-ownership) and the isolated installer tests.

Tray tests use the existing dbus-daemon prerequisite and a guarded private-session fixture. PNG/SVG tray rendering adds pinned Rust dependencies through Iced’s SVG feature; no native development package is added.

The bar’s network adapter reads NetworkManager’s [root properties and change signals](https://networkmanager.dev/docs/api/latest/gdbus-org.freedesktop.NetworkManager.html) and maps its [state/connectivity values](https://networkmanager.dev/docs/api/latest/nm-dbus-types.html). Private D-Bus fixtures exercise property updates, owner replacement, and bus loss; the UI never invokes connection/radio operations. The clock uses local libc time conversion and a minute-boundary timer. Neither subscription starts when its widget is omitted.

`cargo test -p cabby-lock --locked` exercises private Linux-PAM stacks and helper cancellation without using `/etc/pam.d` or authenticating the logged-in account. The helper links the stable `libpam.so.0` ABI; no PAM development headers are required. Locker rendering/lifecycle validation is separate from these backend tests.

Lock-specific isolated integration: build the workspace, then run `python3 scripts/test_lock_session.py --renderer gpu`, repeat with `--renderer software`, and run `python3 scripts/test_lock_lifecycle.py`. The first two use a private nested compositor and synthetic credentials; the last substitutes a READY-aware process into the shipped unit. None submits a real password or locks the current session.

The three Waycrate 0.20.1 crates are patched locally for keyboard indicators; see [vendor provenance and patch scope](../vendor/README.md). Cargo.lock pins the remaining combination. The patch reuses the already-required `libxkbcommon-devel` library (including its stable layout-name query); Fedora prerequisites are unchanged. Do not drop the patch during dependency updates without equivalent modifier/layout event support and nested locker regression evidence.

### Lock-before-suspend checks

`cargo test -p cabby --locked` covers CLI power-request ordering, failed readiness/activity checks, rejection and helper timeout/reaping with intercepted commands. After `cargo build -p cabby --locked`, `python3 scripts/test_lock_lifecycle.py` exercises real isolated notify units and an intercepted suspend request, including rejection without locker teardown. It never submits a host power request. Systemd and Python are existing prerequisites; no package or unit change is needed. Actual sleep/resume and external logind coordination remain unverified.

Native idle integration: `cargo build -p cabby --locked`, then `python3 scripts/test_idle_session.py`. The private nested compositor and intercepted systemctl helper test event delivery and reload/inhibition without locking the host. Existing Wayland/systemd/Python prerequisites suffice.

The graphical harness also runs a private coordinator with automatic locking disabled, renders the optional idle bar contribution, requests manual inhibition through its private control socket and captures the changed label. Vulkan requires a selected hardware adapter; software remains separate. The synthetic image check uses button/text colors rather than the clock's foreground-text detector. Runtime snapshot replacement/removal/malformed-data behavior has a separate asynchronous subscription test.

Display policy extends `test_idle_session.py` with an intercepted niri helper: the nested compositor emits real idle notifications, while all lock and power operations stay in fixtures. `cargo test -p cabby --test power --locked` covers lock rejection before DPMS and safe restoration. Niri is an existing dependency; no new Fedora packages or service installation is needed.

Private logind coverage: `cargo test -p cabby --bin cabby sleep::tests --locked` creates its own D-Bus daemon/Manager and inhibitor sockets. It tests unique-owner signal filtering, readiness/timeout FD lifetime, resume reacquisition and owner loss with synthetic helpers. Existing zbus/Tokio versions and D-Bus/systemd/Python prerequisites are reused. No host sleep or live inhibitor is acquired.

Night-light prerequisite: `curl` supplies bounded HTTPS IP/address location lookups; `just deps` includes it. No package is enabled as a service. Native gamma/schedule code reuses pinned wayland-protocols-wlr. `python3 scripts/test_nightlight_protocol.py` owns a synthetic Wayland socket/server and a parent-death-bound coordinator, validating FD uploads/restoration without any physical gamma changes. The existing graphical harness additionally captures off/unavailable indicator states. See [night-light.md](night-light.md) for source provenance, provider behavior and hardware limitations.

Caffeine reuses the existing pactl, zbus and Wayland dependencies; no Fedora package or production-unit changes are needed. `cargo test -p cabby --bin cabby caffeine --locked` covers configurable policy and private logind block-FD lifetime. `python3 scripts/test_nightlight_protocol.py` also exercises private fullscreen/audio Caffeine sources, night-light restoration and reload; `phase0-session.py` captures manual off/on states on both renderers. These fixtures never acquire live system-bus inhibitors or change real audio/gamma.

Caffeine renderer acceptance uses a repository-owned SVG coffee indicator (no icon font). The centered middle region takes its content width and the two equal sides share the remainder; existing region pixel checks and tray visibility cover the layout. Cancellation now explicitly schedules reaping of an audio subscription child: the nested harness records surviving PID state on failure, which distinguished an exited zombie from an active helper.

Clipboard integration adds `wl-clipboard` to the full Fedora test package set (`scripts/install-fedora-deps.sh --print`); production monitoring uses pinned native Wayland bindings and needs no helper. `cargo test -p cabby-clipboard -p cabby-config --locked` exercises policy and settings. The nested renderer harness calls `scripts/clipboard_acceptance.py` for real isolated source/paste clients, ownership after source/desktop exit, PNG transfer, limits, reload and picker rendering. Coordinator-only fixtures explicitly use an unavailable/private Wayland display so tests cannot observe the live clipboard.

For a faster clipboard-only nested run, use `python3 scripts/phase0-session.py --only clipboard` (and `--renderer software` for fallback). It uses the same private runtime, native clients, adapter assertion and scope cleanup as the full harness.

Screenshot production uses the already listed grim dependency and pinned image/Tokio/Wayland crates; no new Fedora package is required. `cargo test -p cabby-screenshot -p cabby-clipboard --locked` covers geometry, anonymous saves, supervised helpers and the sealed-FD boundary. `python3 scripts/phase0-session.py --only screenshot` and `--renderer software` drive actual region/preview/copy/save/cancel controls through a private virtual pointer, including scale/rotation and recovery. The input fixture refuses displays outside the disposable runtime. Save support is validated on the available Linux filesystem; O_TMPFILE-incompatible filesystems return errors.

Screenshot capture finishes before any screenshot surface is mapped, including software rendering. The selector consumes per-event coordinates so buffered input cannot collapse a fast drag; the pinned `iced_exwlshell` adapter also emits the position carried by Wayland cursor-enter while preserving CursorEntered. Native pointer acceptance begins a drag without a preliminary motion and waits for actual painted controls, rather than treating a model-state update as a rendered frame.

## Release readiness checks

[readiness.md](readiness.md) records the current Fedora/toolchain/GPU baseline, release measurements, review budgets and acceptance limits. `python3 scripts/check_release_install.py` uses actual release binaries in a temporary home and intercepts manager calls; it does not install into the logged-in home. `python3 scripts/phase0-session.py --only readiness` measures default release coordinator/desktop/notifications plus on-demand launcher in a private compositor/bus, with separate `--renderer software` coverage. Build release and probe binaries first. Run measurements sequentially without compilation or other acceptance tests in parallel; no new prerequisite is introduced.
