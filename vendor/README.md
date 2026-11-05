# Pinned Wayland binding patch

These are crates.io 0.20.1 sources from [Waycrate exwlshelleventloop](https://github.com/waycrate/exwlshelleventloop). The upstream [MIT license](https://raw.githubusercontent.com/waycrate/exwlshelleventloop/master/LICENSE) is retained in LICENSE-MIT. Registry archive checksums:

- `exwlshellev`: `b2c3b9dab8cede8cf7432bf176bba7acdf95b7362d314ef2748b1014154843c7`
- `iced_exwlshell`: `ef2435294d7d255c723a5db916c121413116e34fd5fcbf4250f0d39e28b19768`
- `iced_wayland_subscriber`: `5c0cd54cedfab3230ff5487cbeec868c2afc552def98bf2ebb469af1b6f84a39`

Cabby changes preserve Caps Lock and active layout name from each Wayland keyboard modifiers event. The binding sends a typed KeyboardIndicators event through the existing shell broadcast, replays its latest state to subscribers, and clears it on keyboard leave. It never exposes key text, modifies input, or alters session-lock ownership. Layout names use the existing libxkbcommon dependency; its stable layout-name query is absent from xkbcommon-dl's reduced table, so a narrow native declaration calls it while the keymap remains alive.

Changed upstream files: exwlshellev `src/seat.rs` and `src/events.rs`; iced_exwlshell `src/event.rs` and `src/multi_window.rs`; iced_wayland_subscriber `src/shell.rs`. Two `build_pattern` imports in iced_exwlshell are gated with their existing debug feature to avoid unused-import warnings in local builds. One trailing space in the packaged iced_exwlshell README is removed. Other files are unchanged. Cargo patches retain the pinned versions; vendor crates are excluded from workspace membership. Registry metadata and per-package lockfiles are omitted, and the workspace lockfile governs the build.

This is an internal compatibility patch, not a general plugin API. Reevaluate it when upgrading the binding; remove the patch if upstream supplies equivalent keyboard-state events. Cabby policy tests and nested layout/recovery tests cover its consumer. Physical Caps Lock input and multiple-seat behavior remain unverified.
