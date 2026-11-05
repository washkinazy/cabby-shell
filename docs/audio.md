# Audio controls

## Scope and backend

The audio plugin provides a device panel inside the desktop host. Baseline interactions are output/input device selection, mute/unmute, and absolute volume from 0–100%. Profiles, ports, per-application routing, balance adjustment, automatic stream moves, and volume boost are outside this initial feature. Selecting a default delegates stream policy to the audio server; Cabby does not relocate existing streams or promise that every application follows the selection.

The backend wraps `pactl` with explicit argument arrays. JSON `info` and device lists supply snapshots; JSON `subscribe` supplies sink/source/server/card changes. Client/stream events are ignored so snapshot helpers cannot trigger a refresh loop. The tested helper format is pactl 17.0, checked against its [upstream implementation](https://github.com/pulseaudio/pulseaudio/blob/v17.0/src/utils/pactl.c). The intended session dependency is PipeWire with its [PulseAudio-compatible service](https://docs.pipewire.org/page_man_pipewire-pulse_1.html); native PulseAudio compatibility is not separately hardware-validated. Cabby never starts or stops either server.

Input monitor sources are omitted. Device descriptions are bounded and stripped of control characters. The displayed percentage uses the loudest channel; an absolute volume change sets all channels equally. Existing levels above 100% can be reported, but commands cannot increase volume above 100%. Mute and volume are independent. A missing default is represented explicitly, and an empty device list is valid.

## Ownership and bounds

`crates/audio` has no widget dependencies. It exposes typed snapshots/commands, a helper client, and an owned event stream. Commands must target a device in the supplied snapshot and validate the value before executing. The UI discards stale snapshots on disconnect and refreshes after every command. A device can disappear between validation and execution; rejection is recoverable and never silently retargets the server default.

Every helper remains in the caller's systemd cgroup. Requests have a three-second deadline and at most 1 MiB stdout; stderr is discarded rather than logging server/device data. Snapshots use three bounded requests, at most nine seconds total. Device lists are bounded to 256 entries per direction and 32 channels per device. Event lines are capped at 4 KiB and retained across cancellation while partially received. Explicit subscription close kills and reaps the child; dropping a request/subscription kills it through Tokio's child ownership and reaping support, with cgroup cleanup as the process-level boundary. No detached task or shell command is created.

Device names, descriptions, and raw helper output are not logged. Diagnostic types redact device identity. Audio-server preferences stay with that server; Cabby adds no persistent audio state, cache, or configuration keys. The Fedora prerequisite set includes `pulseaudio-utils` for pactl; existing Python is used only for isolated test helpers.

The subscribe CLI does not provide a readiness acknowledgement. The panel offers Refresh for an initial subscribe/snapshot race, refreshes on events during operation, and reconnects after a two-second delay following failure. No recurring healthy-state polling is required.

## Validation

`cargo test -p cabby-audio --locked` uses only private executable fixtures. It covers JSON validation, monitor filtering, default selection, channel volume policy, command bounds and literal arguments, event filtering, partial-event cancellation, oversized output, timeout, missing helpers, rejection, explicit close, and cancellation/drop cleanup. No live audio state is changed by tests. Real device operations, server-specific routing policy, physical hotplug, and compositor-delivered pointer interaction remain unverified.

## Panel interaction and lifecycle

Click **Audio** in the bar to open the panel beside the clicked widget/output. **Use** selects a device as default, marked with a check and “selected”; **Mute/Unmute** changes mute state. Drag the volume slider (0–100%); it sends one command on release, while **−/+** retain five-point steps. Pending requests suppress further device commands. Errors remain visible and server snapshots confirm values. Disappearing devices discard unfinished slider edits.

The desktop owns the audio model and event subscription whenever the audio bar contribution is configured or its panel is open. The bar shows default-output volume, muted status or an unavailable indicator. Closing or switching panels preserves passive status and any already-submitted bounded command; unfinished slider edits are discarded. Removing the contribution with no open audio panel cancels the subscription. Desktop shutdown cancels helpers and cleans the service cgroup. Reconnection waits two seconds and never replays commands.

The shared panel fits the originating output (maximum 440×500 logical pixels), follows the configured bar edge and scrolls long content. Outside click, focus loss, Escape, Close or output removal dismiss it. Stale window callbacks cannot affect a replacement. Healthy idle operation waits on events; bursts are limited to one refresh start per 100 ms plus request time.

No new settings are added. Renderer, base font size and theme colors come from the shared configuration; bar height/position affect panel placement. Section headings use fixed larger sizes. Ordinary managed reloads update appearance without closing the panel; renderer changes and explicit restarts close it. Audio preferences remain owned by the audio server.

The nested renderer harness uses `audio-panel-probe` with an explicit private fixture helper under the disposable-session guard. It verifies a populated panel, keyboard focus, rendered text pixels, and passive status after a probe-only one-shot dismissal and helper cleanup after removing the contribution; abrupt process exit uses the enclosing service cgroup. The same host close path is used by the UI. Worker fixtures cover default selection, volume, input mute, rejected commands, external changes, disconnect/reconnect and cleanup; policy tests cover pending controls and stale panel events. No test invokes live audio mutations.

Ordinary configuration reload now updates the existing host in place and preserves its active UI state. Renderer changes still restart it. See [configuration](configuration.md) for the current reload contract.

[Caffeine](caffeine.md) adds coordinator-owned playback/recording inhibition using this backend, independent of the desktop panel. Its subscription starts only with `caffeine.audio = true`; no extra daemon or sound-state change is involved.
