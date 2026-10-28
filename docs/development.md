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
| grim | Capture oracle for the Phase 0 harness; not the planned production Rust backend |
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
ICED_BACKEND=tiny-skia python3 scripts/phase0-session.py
```

The small experiment has no application policy unit tests yet; its meaningful checks are compilation and the protocol/runtime probes. No Cabby configuration parser or settings have been introduced.

## Isolated runtime harness

Run inside an existing Wayland session with a working systemd user manager. The harness opens a nested niri window using a private runtime/config/cache/state/data tree and a private D-Bus session. It never uses niri's `--session` flag, imports environment into the user's service manager, or connects the locker to the host display. A uniquely named transient `cabby-phase0-*.scope` owns the disposable session, including descendants that detach from process groups. The harness stops this scope on success, failure, or timeout, checks its inactive state, and reaps its wrapper. A six-minute systemd runtime limit also bounds cleanup if the harness itself is killed. No persistent unit is installed.

Results live in an ignored `.artifacts/phase0-*` directory: protocol inventory, output/layer snapshots, a nested-only PNG, client logs, session log, and a JSON result listing both checks and unproven capabilities. These are developer artifacts, not installed user configuration. Protocol traces can contain typed test input; use synthetic text only. They remain locally for debugging and are not committed.

The automatic run verifies bar mapping, a PNG capture through grim, survival after a scale change, compositor lock confirmation, and locker survival after the bar is killed. It then destroys the disposable session. The lock probe intentionally has **no authentication** and must never be installed as a real locker. Its environment guard is an accident-prevention measure, not a security boundary.

For manual focus inspection:

```sh
ICED_BACKEND=tiny-skia python3 scripts/phase0-session.py --interactive
```

Click Open popup, type synthetic text, dismiss with Escape and by clicking outside, reopen, and check focus returns to another nested client. Then create `request-lock` in the printed artifact directory to continue to the lock probe. Click Release test lock to finish. The interactive run times out after five minutes; it does not automatically mark manual observations as passed.

Still required for full Phase 0: observed popup keyboard/focus behavior, multiple outputs and scale correctness (not just process survival), visual lock surfaces, an evaluated Rust capture path, and failure/cleanup checks. A single nested output cannot establish multi-monitor support. Real DRM lock security, PAM authentication, gamma, and suspend/resume belong to later explicit hardware acceptance.

## Sources used for the prototype

- [iced_exwlshell 0.20.1 API and examples](https://docs.rs/iced_exwlshell/0.20.1/iced_exwlshell/)
- [niri nesting and CLI options](https://niri-wm.github.io/niri/Getting-Started.html)

The production architecture remains undecided until Phase 0 exit criteria are met.

## Initial results (2026-10-28)

Compilation, formatting, Clippy, and the Cargo test command pass (there are currently zero Rust unit tests). The nested software-renderer run passed protocol, bar, capture, scaling-survival, lock-confirmation, and lock-survival probes. The captured bar was visually inspected. Test state and captures remain ignored under `.artifacts/`.

The initial wgpu-only build failed to obtain a compatible graphics adapter; trying `WGPU_BACKEND=gl` did not resolve it. The host advertises NVIDIA RTX 4070, AMD integrated graphics, and llvmpipe Vulkan adapters, so missing GPU hardware is not an established explanation. The root cause remains open. Software rendering lets protocol validation proceed but is not evidence of GPU performance or compatibility.

`ICED_BACKEND=tiny-skia` is an upstream Iced developer override, not a Cabby config setting. Omit it to investigate automatic renderer selection, or use `ICED_BACKEND=wgpu` to require GPU rendering. Do not mark GPU validation complete just because automatic fallback succeeds.
