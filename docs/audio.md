# Audio controls

## Scope and backend

The first audio increment implements the backend; the device panel follows. Baseline interactions are output/input device selection, mute/unmute, and absolute volume from 0–100%. Profiles, ports, per-application routing, balance adjustment, automatic stream moves, and volume boost are outside this initial feature. Selecting a default delegates stream policy to the audio server; Cabby does not relocate existing streams or promise that every application follows the selection.

The backend wraps `pactl` with explicit argument arrays. JSON `info` and device lists supply snapshots; JSON `subscribe` supplies sink/source/server/card changes. Client/stream events are ignored so snapshot helpers cannot trigger a refresh loop. The tested helper format is pactl 17.0, checked against its [upstream implementation](https://github.com/pulseaudio/pulseaudio/blob/v17.0/src/utils/pactl.c). The intended session dependency is PipeWire with its [PulseAudio-compatible service](https://docs.pipewire.org/page_man_pipewire-pulse_1.html); native PulseAudio compatibility is not separately hardware-validated. Cabby never starts or stops either server.

Input monitor sources are omitted. Device descriptions are bounded and stripped of control characters. The displayed percentage uses the loudest channel; an absolute volume change sets all channels equally. Existing levels above 100% can be reported, but commands cannot increase volume above 100%. Mute and volume are independent. A missing default is represented explicitly, and an empty device list is valid.

## Ownership and bounds

`crates/audio` has no widget dependencies. It exposes typed snapshots/commands, a helper client, and an owned event stream. Commands must target a device in the supplied snapshot and validate the value before executing. The upcoming UI must discard stale snapshots on disconnect and refresh after commands. A device can disappear between validation and execution; rejection is recoverable and never silently retargets the server default.

Every helper remains in the caller's systemd cgroup. Requests have a three-second deadline and at most 1 MiB stdout; stderr is discarded rather than logging server/device data. Snapshots use three bounded requests, at most nine seconds total. Device lists are bounded to 256 entries per direction and 32 channels per device. Event lines are capped at 4 KiB and retained across cancellation while partially received. Explicit subscription close kills and reaps the child; dropping a request/subscription kills it through Tokio's child ownership and reaping support, with cgroup cleanup as the process-level boundary. No detached task or shell command is created.

Device names, descriptions, and raw helper output are not logged. Diagnostic types redact device identity. Audio-server preferences stay with that server; Cabby adds no persistent audio state, cache, or configuration keys. The Fedora prerequisite set includes `pulseaudio-utils` for pactl; existing Python is used only for isolated test helpers.

The subscribe CLI does not provide a readiness acknowledgement. A UI should offer explicit refresh for an initial subscribe/snapshot race, use event-driven refresh during operation, and reconnect with a delay after failure. No recurring healthy-state polling is required.

## Validation

`cargo test -p cabby-audio --locked` uses only private executable fixtures. It covers JSON validation, monitor filtering, default selection, channel volume policy, command bounds and literal arguments, event filtering, partial-event cancellation, oversized output, timeout, missing helpers, rejection, explicit close, and cancellation/drop cleanup. No live audio state is changed by tests. Real device operations, server-specific routing policy, and UI integration remain unverified at this backend stage.
