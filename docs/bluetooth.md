# Bluetooth controls

## Scope

The built-in Bluetooth button opens a device panel in the desktop host. It lists adapters and known/discovered devices, controls adapter power, searches for devices, pairs, connects/disconnects, and forgets devices after a confirmation. Audio and Bluetooth share one panel slot: opening either closes the other. Close, Escape, output removal, desktop restart, or renderer change also closes the panel. Ordinary configuration reload preserves it.

Pairing supports numeric comparison, PIN entry (1–16 ASCII letters/digits), passkey entry (up to six digits, 0–999999), display instructions for keyboards, and explicit authorization. Compare displayed codes before choosing **Codes match**. **Cancel pairing** rejects a pending prompt and cancels the pairing attempt. Input is masked, retained only while needed, and cleared on prompt changes. This is an ordinary Bluetooth pairing agent, not a Polkit agent.

Pairing does not automatically trust a device or connect every profile. Use **Connect** after pairing when needed; BlueZ chooses supported profiles. Trust/block controls, incoming pairing outside a Cabby-initiated operation, discoverability, custom profiles, file transfers and adapter aliases are outside the baseline. Cabby does not modify discoverable/pairable settings, install authorization policy, or take over the system default agent. Existing BlueZ preferences and bonds remain owned by BlueZ.

## Protocol and ownership

The UI-independent `cabby-bluetooth` crate uses the pinned zbus dependency directly on the system bus. It uses ObjectManager snapshots and PropertiesChanged/InterfacesAdded/InterfacesRemoved signals, and watches the BlueZ bus owner. There is no bluetoothctl subprocess or healthy-state polling. Commands target the current BlueZ unique owner and objects in the last valid snapshot. Service loss clears cached devices and prompts; reconnect attempts occur after a two-second delay, without replaying queued commands or restarting discovery.

The implementation follows the [BlueZ adapter API](https://bluez.readthedocs.io/en/latest/adapter-api/), [device API](https://bluez.readthedocs.io/en/latest/device-api/), and [agent API](https://bluez.readthedocs.io/en/latest/agent-api/). Its KeyboardDisplay agent is registered only for its private connection. Agent calls must come from the current BlueZ unique owner and refer to the device currently being paired; unsolicited requests and service authorization are rejected. Prompt IDs distinguish stale responses, including across reconnects. No PIN, passkey, device identity, raw D-Bus error, or notification content is logged by Cabby.

**Find devices** starts one caller-owned discovery session on one powered adapter, for at most 60 seconds after acknowledgement. **Stop searching** releases that session; other applications' discovery remains theirs. A failed/uncertain start or stop closes the connection so Cabby cannot leave an untracked lease. Normal shutdown attempts cancellation, discovery stop, and agent unregister, then closes the connection. Dropping the UI subscription immediately drops its connection, streams and pending operation; BlueZ owns cleanup on caller loss. This is also the crash boundary. The upstream [discovery disconnect handler](https://github.com/bluez/bluez/blob/master/src/adapter.c) releases the departing client's discovery session; the [pairing requestor exit handler](https://github.com/bluez/bluez/blob/master/src/device.c) cancels authentication and disconnects an unfinished bonding request.

Ordinary calls, connection setup and snapshot refreshes have five-second deadlines; pairing has a 120-second deadline, followed by cancellation on failure. Closing the panel does not undo a completed pairing, power change, connection, or removal. A server operation may have completed even when its reply is lost; reopen or Refresh to reconcile state. The client serializes operations while continuing to handle pairing requests and dependency events. The desktop systemd service owns the process; BlueZ is an external dependency and is never started, stopped, or killed by Cabby.

Snapshots accept at most 4096 managed objects, 16 adapters and 256 devices. Names are stripped of control characters and limited to 160 characters. Agent bodies are limited to 4 KiB before decoding. These are application bounds after zbus wire reception; zbus's 128 MiB raw-message limit is not reduced by them. Command and event queues are bounded; published UI snapshots coalesce. No Bluetooth preferences, cache, or credentials are written by Cabby.

## Configuration and dependencies

No Bluetooth-specific settings are added. The panel uses the shared renderer, base font size, colors, and bar clearance, with fixed heading sizes and the same scrollable 440×500 logical-pixel maximum as Audio. File configuration/reload semantics are in [configuration.md](configuration.md). Device actions are runtime controls with BlueZ's persistence rules.

Fedora requires `bluez` and the existing system D-Bus environment. The dependency installer lists the package but does not enable its service or change permissions. Missing services, adapters, denied policy, radio blocks and rejected operations are shown as unavailable/rejected states. Cabby does not bypass system policy or unblock rfkill automatically.

## Validation and limits

Private dbus-daemon fixtures cover discovery start/stop/expiry, adapter power, pairing prompts and stale replies, foreign agent callers, cancellation, connect/disconnect/rejection/removal, external property changes, owner loss/recovery, bus loss, and graceful/abrupt client cleanup. UI tests cover confirmation before removal, PIN/passkey validation, secret clearing, and shared-panel/stale-window behavior.

The nested compositor probe starts a private BlueZ fixture, opens the real panel, starts discovery, shows a pending pairing confirmation, checks keyboard focus and rendered pixels, then dismisses the panel and verifies agent/discovery cleanup while the desktop remains alive. Its explicit private bus and disposable-display guards are test-only; production uses the system bus. The fixture is built separately as a test example and adds no production executable or service.

Physical adapters/devices, real pairing/connect behavior, policy/rfkill combinations, physical hotplug, pointer/key delivery, focus restoration and multiple physical outputs remain unverified. Automated tests never mutate real Bluetooth state. Synthetic success is not a claim of hardware compatibility.

Ordinary configuration reload now updates the existing host in place and preserves its active UI state. Renderer changes still restart it. See [configuration](configuration.md) for the current reload contract.

## Passive status and device ordering

The desktop observes BlueZ while the Bluetooth contribution is configured. The bar shows unavailable, powered off, or the connected-device count. This read-only connection registers no agent, never starts discovery and rejects control commands. Opening the panel replaces it with an interactive connection; dismissal replaces that connection with passive observation, releasing its agent, discovery and pairing lease. Generation-qualified events prevent the previous connection from updating a replacement. Omitting the contribution with no open panel stops observation.

Devices are grouped as Connected, Paired and Available. Within each group first-seen ordering is retained through property/name/battery updates; removed devices leave that ordering. BlueZ Battery1 percentages are displayed when available (0–100%); absence is not interpreted as zero. No battery polling or extra process is added. Private fixtures verify passive isolation and battery parsing; actual device reporting remains unverified.
