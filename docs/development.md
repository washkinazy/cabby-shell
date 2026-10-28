# Development and Phase 0 experiments

## Fedora prerequisites

Install the development and test packages with:

```sh
bash scripts/install-fedora-deps.sh
```

Inspect the package list without installing anything:

```sh
bash scripts/install-fedora-deps.sh --print
```

The script uses DNF and requests sudo when needed. It installs packages only; it does not enable services or install Cabby. Keep it updated with every build/test dependency change.

| Packages | Purpose |
| --- | --- |
| cargo, rust, rustfmt, clippy | Compiler, dependency/build tool, formatter, linting |
| gcc, pkgconf-pkg-config | Native linker and library discovery |
| wayland-devel, libxkbcommon-devel | Wayland and keyboard development libraries |
| niri, wayland-utils | Nested compositor and protocol inspection |
| grim | Working capture backend for the Phase 0 harness; also a permitted production helper |
| dbus-daemon, systemd | Private D-Bus session and service lifecycle tooling |
| python3 | Isolated-session harness |
| mesa-dri-drivers, mesa-vulkan-drivers | Mesa rendering support, including software fallback where available |

Initial environment: Fedora 45, Rust/Cargo 1.98.1, niri 26.04, systemd 262, Wayland 1.26.0, libxkbcommon 1.13.1. Other distributions/versions are not yet validated. Existing proprietary GPU installations may supply a different Vulkan driver.

## Rust baseline

The workspace currently contains only a platform experiment, not the production shell. Direct dependencies are pinned to Iced 0.14.0 and iced_exwlshell 0.20.1; Cargo.lock fixes transitive dependencies. This is a selected validation baseline, not a claim that these are the newest versions. The declared Rust floor is 1.98; only 1.98.1 has been exercised. Iced's wgpu and tiny-skia software backends are compiled with Wayland enabled; GTK and automatic desktop-theme detection are not used.

```sh
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build --workspace --locked
python3 scripts/phase0-session.py
python3 scripts/phase0-session.py --renderer software
python3 -m unittest discover -s scripts -p 'test_*.py'
```

Synthetic Rust tests cover duplicate popup opens, Escape dismissal/reopening, and delayed close events. Python tests check that renderer evidence comes from the selected adapter, not merely an enumeration of available devices. These tests do not prove real keyboard focus delivery. The production config parser now supplies renderer and bar-height settings to the UI prototype; see [configuration.md](configuration.md).

## Isolated runtime harness

Run inside an existing Wayland session with a working systemd user manager. The harness opens a nested niri window using a private runtime/config/cache/state/data tree and a private D-Bus session. It never uses niri's `--session` flag, imports environment into the user's service manager, or connects the locker to the host display. A uniquely named transient `cabby-phase0-*.scope` owns the disposable session, including descendants that detach from process groups. The harness stops this scope on success, failure, or timeout, checks its inactive state, and reaps its wrapper. A six-minute systemd runtime limit also bounds cleanup if the harness itself is killed. No persistent unit is installed.

Results live in an ignored `.artifacts/phase0-*` directory: protocol inventory, output/layer snapshots, a nested-only PNG, client logs, session log, and a JSON result listing both checks and unproven capabilities. These are developer artifacts, not installed user configuration. Protocol traces can contain typed test input; use synthetic text only. They remain locally for debugging and are not committed.

The automatic run verifies bar mapping, a PNG capture through grim, survival after a scale change, compositor lock confirmation, and locker survival after the bar is killed. It then destroys the disposable session. The lock probe intentionally has **no authentication** and must never be installed as a real locker. Its environment guard is an accident-prevention measure, not a security boundary.

Optional developer inspection (not a required test or user task):

```sh
python3 scripts/phase0-session.py --interactive
```

Click Open popup, type synthetic text, dismiss with Escape and by clicking outside, reopen, and check focus returns to another nested client. Then create `request-lock` in the printed artifact directory to continue to the lock probe. Click Release test lock to finish. The interactive run times out after five minutes; it does not automatically mark manual observations as passed.

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

The validated platform baseline is Rust/Iced with hardware Vulkan, an explicit software fallback, and systemd-owned component lifecycles. Production configuration and services follow in Phase 1.

## Renderer diagnosis and results (2026-10-28)

The inherited environment contains `WGPU_BACKEND=gl`. Consequently, the initial runs selected only OpenGL, not wgpu's unrestricted adapter selection. Diagnostics identify the NVIDIA OpenGL adapter but report an incompatible presentation surface. This OpenGL path remains unsupported in the tested nested setup; we have not modified the driver or global environment.

Explicit Vulkan selection succeeds. Both bar and lock probe select **NVIDIA GeForce RTX 4070 / Vulkan / DiscreteGpu** and pass the nested smoke test, including buffer submission, capture, scale change, lock confirmation, bar-crash isolation, and inactive systemd scope after cleanup. This establishes a working GPU path on this machine, not universal driver compatibility. The explicit tiny-skia run also passes; no performance comparison has been made.

The harness defaults to `--renderer gpu`, which sets `ICED_BACKEND=wgpu`, `WGPU_BACKEND=vulkan`, and `CABBY_RENDERER_DIAGNOSTICS=1` only for its probe children. It records the selected adapter for both clients and rejects CPU adapters or silent software fallback. `--renderer software` explicitly selects tiny-skia. Renderer flags are development harness options, not persistent Cabby configuration. The UI prototype reads config and re-execs with the selected backend when inherited renderer variables disagree. The standalone lock probe still uses upstream environment selection; the harness sets it explicitly.

Diagnostics use the existing Rust log ecosystem and filter to Iced/wgpu renderer messages. `CABBY_RENDERER_DIAGNOSTICS` enables these messages when present; leave it unset to disable them for direct runs. The Fedora installer needs no new system packages for this change. The renderer setting now has a full entry and examples in the configuration reference.

COSMIC's [libcosmic documentation](https://github.com/pop-os/libcosmic#cargo-features) describes an Iced-based toolkit with software rendering by default and optional wgpu GPU rendering. Its [manifest](https://github.com/pop-os/libcosmic/blob/master/Cargo.toml) exposes that selection. This is the application toolkit, distinct from COSMIC's compositor renderer. Cabby similarly keeps both renderer paths available, but tests GPU and software separately instead of treating fallback as GPU success.
