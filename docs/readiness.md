# Readiness evidence

The required feature implementations pass the recorded automated/nested scope through Phase 6. This page records Phase 7 packaging and performance evidence. A sustained daily-use trial and real-device/session validation remain unverified; Phase 7 is not declared complete on the strength of nested tests.

## Tested baseline

The 2026-11-05 release build uses Fedora 44, Rust/Cargo 1.99.1, niri 26.04, systemd 259.10, kernel 7.2.15-200.fc44.x86_64, an AMD Ryzen 7 7700 and NVIDIA GeForce RTX 4070. Iced 0.14.0 and the patched Waycrate 0.20.1 combination are pinned by Cargo.lock and [vendor provenance](../vendor/README.md). The selected Vulkan adapter reports NVIDIA driver 615.71.10. Earlier Fedora 45/Rust 1.98.1/systemd 262 observations in the development guide are historical evidence, not the current host description. Other distributions, older runtimes, OpenGL and other proprietary driver/backend combinations are unvalidated. Tiny-skia has separate fallback evidence. Subsequent OSD/Fluent regression runs also select RTX 4070 Vulkan on NVIDIA 620.70.06 after reboot; see PLAN.md for artifacts. OSD remains disabled in the comparison performance probe, so the historical resource numbers below exclude its LED observer.

The Fedora installer contains the complete maintained build/runtime/test package set. It installs packages without enabling services. Local installation owns binaries and vendor user units; preferences/bindings remain in dotfiles. Distribution packages and third-party plugin delivery are outside the current scope.

## Packaging and recovery

`cargo build --release --locked --offline -p cabby -p cabby-shell -p cabby-lock --bins` passes using the existing dependency cache. `scripts/check_release_install.py` exercises the actual eight ELF binaries and six units under a temporary HOME/XDG tree with intercepted manager calls: install, repeated upgrade, source/installed digest equality, executable modes, installed CLI validation, invalid-config rejection before mutation and uninstall preserving preferences. No service activation is allowed by its manager stub. Evidence: `.artifacts/release-install-result.json`. This establishes a clean user installation on the existing Fedora host; it does not establish dependency completeness on a clean OS image. The five installer policy tests additionally cover personal overrides, masks, modified files, migration, active-lock refusal and staged unit syntax.

The final screenshot integration ran all 14 isolated service-lifecycle scenarios (`.artifacts/lifecycle-5gyvgfft`): duplicate start, invalid/deleted/live config, desktop/coordinator crashes, notification owner conflict, application/lock independence, descendant cleanup, restart limits and compositor loss. Those scenarios use explicit component stand-ins to test real systemd relationships. Actual clients, protocol behavior, rendering and private dependencies are tested separately. Full desktop Vulkan/software regressions pass (`.artifacts/phase0-67i2mzb8`, `.artifacts/phase0-rc14tsdz`), as do the native screenshot flows (`.artifacts/phase0-6glto108`, `.artifacts/phase0-81i0_gmi`). Each scope reports inactive after teardown; runtime test units are removed. Lock authentication/recovery, gamma, Caffeine and idle/display protocol evidence is recorded in [PLAN.md](../PLAN.md). These observations are complementary; stand-ins do not prove client rendering or hardware security.

Use the [setup and recovery guide](setup.md) for commands. A UI restart leaves the independently supervised active lock intact. Coordinator restart loses in-memory clipboard/manual Caffeine state. Unsaved screenshot previews are discarded with the desktop host. Shared BlueZ/PipeWire/NetworkManager services and independently launched user applications retain their documented ownership.

## Measured release behavior

The isolated readiness probe runs the real release coordinator, desktop and notification owner together, initializes/then expires one synthetic notification, samples three consecutive five-second idle windows, then opens the real launcher three times. Default configuration is used, except explicit renderer selection. Audio and the system bus are deliberately unavailable, with their retry workers included; clipboard uses the private compositor and empty memory-only history. Automatic lock/display/sleep and automatic Caffeine are off. Healthy-device load, active playback, populated histories, open panels/captures and sustained daily operation are not measured.

| Observation | Vulkan | Tiny-skia |
| --- | --- | --- |
| Idle CPU, percent of one core | 0.80–1.00% | 0.20–0.40% |
| Combined resident memory, RSS | 380.2–380.3 MiB | 61.2–61.3 MiB |
| Combined proportional memory, PSS | 198.0–198.1 MiB | 43.4 MiB |
| Coordinator snapshot/socket ready | 10 ms | 10 ms |
| Desktop spawn to first buffer | 385 ms | 31 ms |
| Notification bus name ready | 4 ms | 4 ms |
| Notification request to first buffer | 226 ms | 53 ms |
| Launcher spawn to buffer + keyboard focus, three trials | 296 / 183 / 173 ms | 41 / 61 / 81 ms |

Evidence: Vulkan `.artifacts/phase0-s1qwv8ay`, software `.artifacts/phase0-1rc64uk4`. Vulkan explicitly selects RTX 4070 for desktop, notifications and each launcher; software is requested separately. Three ordinary root processes were present at each memory sample. CPU includes live descendants and CPU charged for reaped helpers. RSS sums shared pages repeatedly; PSS apportions shared pages. Both exclude GPU allocations and the compositor. Renderer/Wayland diagnostics are enabled; timing uses 10 ms observation polling, existing host/compositor load and warm OS/driver caches. First buffer is an application submission metric, not physical presentation latency. These short observations are a baseline, not a broad renderer benchmark or a claim that software always outperforms hardware.

Initial regression review budgets below use roughly twice the measured maxima, rounded upward with at least 100 ms for short timings. Apply them only to repeated observations on this recorded machine and probe conditions. Exceeding one calls for investigation/remeasurement; it does not establish a universal compatibility failure. The probe reports raw results rather than enforcing machine-specific limits in generic CI.

| Review budget | Vulkan | Tiny-skia |
| --- | --- | --- |
| Idle CPU, one core | 2% | 1% |
| Combined PSS / RSS | 400 / 800 MiB | 100 / 150 MiB |
| Coordinator ready | 100 ms | 100 ms |
| Desktop first buffer | 1000 ms | 100 ms |
| Notification owner / first buffer after request | 100 / 500 ms | 100 / 150 ms |
| Launcher buffer + keyboard focus | 750 ms | 200 ms |

Reproduce after building the release binaries and the development probes described in [development.md](development.md):

```sh
python3 scripts/check_release_install.py --output .artifacts/release-install-result.json
python3 scripts/phase0-session.py --only readiness
python3 scripts/phase0-session.py --only readiness --renderer software
```

Run timing probes sequentially without concurrent builds/tests. Each owns a bounded private compositor/D-Bus scope and cleans it on success/failure. No live clipboard, screenshot, notification owner, device state, gamma, lock or suspend operation is used.

## Remaining acceptance limits

Physical PAM/input/DRM lock security, DPMS, gamma, suspend/resume, actual logind authorization, real Bluetooth/audio operations, multiple physical outputs/seats and healthy-device performance remain unverified. Single-output nested geometry, fractional scale, rotation and protocol fixtures cover the feasible automated cases. Clipboard currently serves the first seat and supported text/PNG MIME policy. Screenshot saves require Linux O_TMPFILE support; unsupported filesystems retain the preview with an error. Tiny-skia's preview backdrop is currently opaque black. See each feature document for its bounded scope.

The next evidence is ordinary daily use and any reported regressions, with a deliberately agreed live test window for disruptive hardware/security checks. No clean-OS install or sustained trial was performed by these scripts. None of these gaps is silently counted as a pass.
