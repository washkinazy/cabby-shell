# Phase 3 notification design

Status: design accepted for the first Phase 3 feature; not implemented. No notification service, configuration keys, or activation files are shipped by this increment. Audio and Bluetooth follow as separate features. This design narrows the initial implementation and defines its acceptance evidence; it does not mark Phase 3 complete.

## Scope and presentation

Provide passive text notifications with explicit action buttons and dismissal. Use the shared Iced theme. Show at most three cards on the active output, top-right below a top bar, without reserving screen area or requesting keyboard focus. A new card uses the active output at arrival; existing cards retain their output. When an output disappears, move its cards to a surviving output. If no output exists, retain pending cards within the global bound and start their expiry only after presentation. A scrollable card body makes bounded text accessible on small outputs. No notification center, disk history, sounds, images, application icons, hyperlink launching, or rich-text rendering in the first feature.

The UI renders model snapshots and submits typed dismiss/action commands. Protocol logic, expiry, capacity, and replacement live in a separate Rust library with an injected monotonic clock. Passive surfaces must not intercept input outside the cards. Physical placement and pointer delivery remain explicitly unverified unless isolated automation exercises them.

## Retention and resource policy

Cabby-specific initial policy:

- Retain only active/pending notifications in memory; delete content when closed. No archive, replay, cache, or persistence across service restart. Clearing runtime state cannot recover notification text.
- Bound the combined pending/visible set to 64 entries. Reject a new entry at capacity with a resource-limit error; allow a valid replacement without allocating another slot. Do not evict a critical card silently. At most three cards are visible, with remaining entries queued in arrival order.
- Limit summary and application name to 256 UTF-8 bytes each, body to 16 KiB, and actions to eight pairs with keys/labels of at most 256 bytes each. Reject oversize or malformed requests without mutating existing state; never truncate an action identifier. Ignore unsupported hints without retaining their values. Bound the transport message/queue separately before decoding untrusted large payloads; exact library controls must be verified in implementation.
- For server-selected expiry use five seconds for low/normal urgency. Critical cards do not expire automatically. An explicit zero timeout also remains until closed. Use the protocol timeout units and sentinel values; reject values below -1. Replacement resets presentation/deadline state atomically and invalidates any old deadline token.
- Close and expiry remove an entry exactly once, even when commands race. Stale actions and old timers cannot affect a replacement. No content, action labels, or hints in diagnostics, tracing, test failure dumps, or persistent state. Fixtures use synthetic text only.

These are implementation policies, not supported configuration settings. If exposed as settings later, update the schema, reference, examples, and coverage tests in that implementation branch.

## Protocol and text policy

Implement the standard methods and closure/action signals from the [freedesktop notification protocol](https://specifications.freedesktop.org/notification/latest/protocol.html). Advertise only `body` and `actions` once both work end to end; do not claim persistence, markup, images, sound, or activation-token support. Return nonzero IDs, replace active IDs atomically, and test the specification's nonzero replacement-ID behavior explicitly. Closing an unknown ID returns an error. Close reasons distinguish expiry, user dismissal, and client closure. Actions emit the supplied key, never execute text or launch a shell. A default action is available only when supplied by the client.

Plain text is the initial rendering contract. Do not advertise `body-markup`; display supplied tags literally rather than interpreting them. The [capability contract](https://specifications.freedesktop.org/notification/latest/protocol.html) requires clients to strip markup for servers without that capability. This chooses literal display despite the [markup chapter's recommendation](https://specifications.freedesktop.org/notification/latest/markup.html) to filter unsupported tags. Preserve newlines, normalize other control characters for display, and never fetch links or files named in content.

Consume correctly typed urgency and resident hints. Unknown or wrongly typed optional hints are ignored. A resident entry remains after an action; other actions dismiss the card after emitting the action event. Transient entries already receive the no-history policy. Critical expiry follows the [urgency guidance](https://specifications.freedesktop.org/notification/latest/urgency-levels.html); hint interpretation follows the [hint reference](https://specifications.freedesktop.org/notification/latest/hints.html). These choices do not promise application activation or focus transfer.

## Ownership and recovery

Use one notification service process containing the transport, model, and Iced surfaces. This keeps the bus owner independent of bar restarts without creating an extra process for each card. The process belongs to the ordinary Cabby target and binds to the real compositor session, with cgroup cleanup and the existing bounded restart/stop policy. The coordinator will uphold it after recovery, consistently with existing ordinary components. No production lock coupling is introduced.

Acquire `org.freedesktop.Notifications` exclusively without replacing another owner or waiting in its name queue. An existing owner is a clear startup conflict: exit with a distinct status excluded from automatic restart, leave that owner untouched, and require an explicit retry after the conflict is resolved. Do not ship D-Bus autoactivation that could start Cabby outside its compositor-bound service lifecycle.

On session-bus loss, stop accepting requests, discard sensitive model state, and exit for bounded systemd recovery. A fresh instance reacquires the name and starts empty. No tight internal reconnection loop. Shell stop removes cards and releases the name; compositor loss stops the whole component. Ordinary bar restart must preserve notifications. Configuration reload may restart the notification component in the initial implementation; document its consequent loss of active cards before enabling managed reload for it.

## Implementation increments and acceptance

1. **Policy library:** typed requests/events, bounded active set, deterministic IDs, replacement, expiry, actions, and plain-text policy. Test with injected time: timeout boundaries, critical/zero timeouts, queued presentation, replacement with an old timer in flight, malformed/oversize requests, capacity with replacements, stale action keys, ID exhaustion/wrap, and exactly-once closure.
2. **Private D-Bus service:** implement and pin the chosen Rust transport dependency. Test real method signatures, introspection, capabilities, errors and signals, ownership conflict, bus loss, and bounds under a private `dbus-run-session`. Require explicit readiness and bounded client waits; capture no real notifications. The normal session bus must never be the integration-test target.
3. **Iced UI and lifecycle:** wire snapshots/commands into passive surfaces and ship the supervised unit. Extend isolated lifecycle fixtures for bar independence, notification crash/restart limits, name release, shell stop and compositor loss. Use nested niri with synthetic notifications for mapping, replacement without duplicate surfaces, action/dismiss state, scaling, and cleanup. Verify selected RTX 4070 Vulkan separately from tiny-skia. Synthetic output tests cover removal/reassignment; label unexercised input/hardware behavior.

Each code increment requires the standard fmt/check/Clippy/test commands. Unit changes require staged systemd verification and isolated lifecycle tests. Dependency changes must keep the Fedora installer and development reference current. Do not call the notification feature complete until protocol, UI, and lifecycle evidence all pass; do not call Phase 3 complete until audio and Bluetooth also meet their acceptance criteria.
