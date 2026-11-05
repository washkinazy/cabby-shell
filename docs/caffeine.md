# Caffeine

Caffeine combines independent manual, audio and fullscreen reasons in the existing coordinator. It owns policy; the idle and night-light workers apply effects. It adds no daemon or user unit. The desktop contributes a status/toggle widget using typed commands and the shared runtime-file subscription.

Configure `[caffeine]` in `$HOME/.config/cabby-shell/config.toml`:

```toml
[caffeine]
audio = true
fullscreen = true
fullscreen_requires_focus = true
inhibit_idle = true
inhibit_sleep = true
manual_pause_nightlight = false
fullscreen_pause_nightlight = true
```

Audio and fullscreen automation default off. Other values above are defaults. Every setting applies on valid config reload; invalid edits retain the previous policy. The [configuration reference](configuration.md#caffeine) documents each setting, dependencies, defaults and alternatives. The [effects example](../examples/config/caffeine-effects.toml) demonstrates independent choices.

`cabby caffeine-on`, `caffeine-off`, `caffeine-toggle` and `caffeine-status` use the coordinator control socket, with optional `--runtime-dir PATH`. Manual state lasts until coordinator restart. Turning manual off leaves automatic reasons intact. The old `idle-inhibit-on|off|toggle` commands and optional idle widget now control this same manual reason, including configured sleep/night-light effects. Explicit locking remains available; suspend respects active sleep blockers. Caffeine never unlocks a session.

The default right bar includes `caffeine` before `nightlight`. Explicit existing layouts must add it. The compact vector coffee indicator shows active steam, distinguishes manual/automatic state through button styling, and reports failed sleep protection with an error style. Its tooltip lists reasons and source availability. Clicking toggles only the manual reason. Removing it does not disable coordinator policy.

## Sources and effects

- Audio uses a separate event-driven `pactl` observer, independent of desktop/audio-panel lifetime. Any RUNNING output or non-monitor recording input counts, including muted devices. Monitor sources do not count. No audio commands mutate sound state. Helper failures clear only the audio reason and retry after two seconds.
- Fullscreen uses foreign-toplevel fullscreen/minimized/activated state and output membership, committed on `done`. Focus is required by default. Maximized, minimized and off-output windows do not count. `fullscreen_requires_focus = false` includes background fullscreen windows, including hidden niri workspaces; niri output membership alone does not prove visibility. Multiple windows/outputs compose when focus gating is disabled. Stable output names map to the separate gamma connection; missing names conservatively pause all gamma outputs when that effect is enabled. Missing protocol or disconnect clears this source and reports availability. Titles and application IDs are ignored.
- Idle inhibition suppresses automatic lock/display-off and starts fresh countdowns when released. Compositor application inhibitors remain independent. Requests already accepted by the independent locker remain supervised.
- Sleep inhibition holds a close-on-exec logind `sleep/block` FD while an active reason requests it. Acquisition uses a three-second method timeout; owner/bus loss drops the FD and retries after two seconds. Status claims protection only after acquisition. This is separate from lock-before-sleep's finite `sleep/delay` lease. Logind authorization and external clients that bypass inhibitors remain outside Cabby's control.
- Night-light pauses release gamma controls without rewriting enabled/force/temperature preferences. Fullscreen pauses only affected outputs by default; manual pause is optional and covers all outputs. Audio never pauses night light. Clearing a pause recomputes the current schedule/forced target, including changes made during the pause.

Reasons are independent of effects: disabling an effect does not erase reasons. Disabling a source clears only that source. All subscriptions/helpers/FDs belong to the coordinator and are cancelled on disable/shutdown, with systemd cgroup cleanup as the crash boundary. Runtime snapshots are atomic 0600 `caffeine.json` files; no playback content, window titles or persistent manual state are stored.

Niri’s [foreign-toplevel implementation](https://github.com/niri-wm/niri/blob/main/src/protocols/foreign_toplevel.rs) reports keyboard focus through activated state and [assigns output membership across all workspaces](https://github.com/niri-wm/niri/blob/main/src/layout/mod.rs). The focused default avoids treating that membership as visibility.

## Replacing the external audio helper

Enable `caffeine.audio` before retiring `sway-audio-idle-inhibit`. The project dotfiles opt into audio/fullscreen and no longer ship its user unit. Applying dotfiles does not necessarily stop an already enabled/running legacy unit. After installing and starting the new Cabby version and confirming `cabby caffeine-status` reports audio `watching`, retire that legacy service with `systemctl --user disable --now wm-sway-audio-idle-inhibit.service`. Cabby tests/install do not stop live helpers or change audio state.

## Evidence and limits

Policy tests cover independent reasons and configurable effects. Private D-Bus tests verify a real block FD stays held until cancellation or owner loss. A synthetic Wayland/PulseAudio fixture exercises fullscreen/manual overlap, gamma restoration, playback/recording and config reload without touching the physical display or audio server. Nested renderer checks cover the widget on hardware Vulkan and software. Real logind authorization, physical gamma/DPMS/sleep and multiple physical outputs remain unverified.
