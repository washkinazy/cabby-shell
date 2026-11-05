# Internal plugin design intent

Status: intent recorded on 2026-11-04; the user subsequently authorized starting the refactor. Built-in feature modules share startup, and a common surface host now runs bar and wallpaper. They now share the production desktop process. Launcher and notification plugins have separate host adapters; audio contributes the first on-demand device panel in the desktop host.

## Purpose and scope

Cabby is a personal shell maintained in a public repository. Competing with other shells or building a platform for broad adoption is not the goal. Plugins are an internal design pattern to make adding features consistent, easier to understand, and easier to maintain.

Features such as audio, Bluetooth, wallpaper, and notifications are natural candidates for plugin boundaries. Plugins belong to this repository, compile with Cabby, and evolve with the project. There is no requirement for dynamic loading, third-party code permissions, distribution infrastructure, a stable public plugin API, or compatibility with independently released plugins.

The value should come from familiar feature structure and shared machinery. Introduce abstractions where working features show repeated needs; avoid building a general-purpose framework in advance. A feature should not need irrelevant methods merely to satisfy a universal interface.

## Responsibility boundaries to explore

The intended separation is that a plugin owns feature behavior and the shell owns how features participate in the interface and lifecycle. The following responsibilities guide the direction; the implemented startup, module, and passive-surface boundaries are described below:

| Feature/plugin responsibility | Shared shell responsibility |
| --- | --- |
| Backend state, system-service subscriptions, typed commands and events | Common task ownership, cancellation, and error presentation conventions |
| Feature UI and interactions | Bar/panel composition, surface placement, shared styling, and output tracking |
| Feature configuration schema and validation | Configuration discovery, complete validation, and coordinated reload |
| Release of feature-owned tasks and helpers | Supervised process lifetime and coordinated shutdown |

Keep backend logic testable independently of Iced widgets. Features may contribute different kinds of UI: audio provides a bar control and device panel; wallpaper needs background surfaces. This does not commit to configurable bar composition, new settings, a generic event bus, or any particular registry or trait design.

## Plugins and services are separate decisions

A plugin is a code and responsibility boundary. A process or systemd service is a supervision, isolation, and lifetime boundary. A plugin does not automatically get its own daemon. Several plugins could share a process; a separate process needs a concrete lifecycle or isolation reason.

The current topology uses a shared desktop host for bar/wallpaper, plus the coordinator, notification owner, and on-demand launcher. Their separate lifetimes and failure policies are documented in services.md. Further consolidation should have a concrete benefit and preserve those policies. Sharing a process also shares crash impact; an internal plugin contract alone does not provide process isolation.

Existing ownership requirements continue to apply: every Cabby process and helper has a supervisor and shutdown policy; the active lock survives ordinary shell stop/restart; launched applications have independent ownership; BlueZ and PipeWire remain external services. The current implemented topology and reload behavior are documented in [services.md](services.md). The initial extractions preserved behavior; the subsequent desktop consolidation intentionally couples bar/wallpaper restart and failure lifetime, documented in the service and configuration references.

## Implemented first increment

`crates/shell` now owns the production UI implementation. Its `host::Plugin` enum registers Bar, Launcher, Wallpaper, and Notifications; `host::run` validates configuration once, applies renderer selection before worker threads start, initializes diagnostics, and applies the existing clipboard policy. The launcher retains text-input clipboard support; other migrated features disable it. A typed startup context carries configuration and its source to the selected plugin.

Feature modules under `crates/shell/src/plugins/` own typed feature messages, state, subscriptions, and views. Bar and wallpaper surface settings/output ownership have moved into the shared surface host; the launcher host owns its overlay and focus lifecycle, and the notification host owns card windows and presentation acknowledgement. Existing notification policy/transport and launcher discovery crates remain separate from UI widgets. Shared theme and helper invocation live in `ui`; the niri client remains separately testable. There is no universal plugin trait or untyped command bus.

The production binaries in `crates/shell/src/bin` are thin selectors for the built-in plugins. `crates/platform-spike` now contains test probes only. Their names remain stable for the CLI, units, and test harness; guarded probes remain test-only. The `cabby-desktop` executable selects both bar and wallpaper plugins. Standalone plugin executables remain available for isolated development. The earlier composition probe has been replaced by testing the production `cabby desktop` wrapper.

To add a built-in feature at this stage, keep backend policy independently testable, add its UI module, and register a typed entry point with the host when a standalone surface is appropriate. The audio panel provides the first concrete shared-panel integration; this convention does not require a new executable/service for every future feature. Extend configuration through the existing production schema, reference, examples, and coverage checks.

## Shared passive-surface host

`host/surfaces.rs` owns the Iced daemon, output-to-window registry, layer roles, input policy, theme, and resize subscriptions for bar and wallpaper. Plugins provide views and typed feature events; they no longer start their own event loops or track Wayland surface identities. Bar receives the output name for its view and retains its niri state/subscription. Static wallpaper contributes a view without artificial command or lifecycle methods.

On output arrival the host creates one surface per selected plugin, addressed by Wayland output identity. Output metadata updates preserve existing window IDs; removal closes only that output's surfaces. Bar reserves its configured edge and height without keyboard focus. Wallpaper fills the background without intercepting input. Host resize events rebuild static image placement, including software rendering after scale changes.

`cabby desktop` selects both plugins in one process and event loop. The nested harness checks both layer namespaces and attached buffers, wallpaper and bar pixels at scale 1 and 1.5, workspace IPC, selected hardware Vulkan or explicit software rendering, and lock independence after host termination. Synthetic tests cover multiple output identities and late cleanup events. These tests do not establish physical hotplug behavior.

The existing `cabby ui` and `cabby wallpaper` wrappers each use this same host with a single plugin selected. The production `cabby-ui.service` now selects both via `cabby desktop`. Its restart/recovery and configuration migration are documented in services.md and configuration.md. Notification card surfaces use their own host adapter because replacement, first-draw acknowledgement, and queue promotion have different requirements.

## Launcher overlay host

`host/launcher.rs` owns active-output placement, exclusive keyboard policy, initial/refocused search input, theme, and the on-demand event loop. The launcher plugin owns discovery, selection, keyboard semantics, launch requests, errors, and its view. It returns a typed `Action::Close` for dismissal or successful launch; it does not exit the application itself. The current standalone host interprets that as process exit, leaving a future composed host free to close only the overlay. Failed launches keep the picker open. Existing helper supervision, single-instance service behavior, source-config forwarding, and application ownership are preserved.

## Notification card host

`host/notifications.rs` owns card window IDs, output geometry, layer creation/removal, redraw acknowledgement, and process exit codes. The feature owns the service subscription, snapshots, commands, action errors, and card content. It exposes bounded visible revision tokens and typed changes/failures; it does not know window IDs or exit the process. The host passes the token of a drawn revision back to the feature, preserving stale-draw protection and presentation-triggered expiry.

The subscription still owns the D-Bus service future directly, with no detached backend task. Ownership conflicts remain exit 73 in the standalone host. In-memory retention, queue bounds, action semantics, shared theme, and reload discarding notices are unchanged. Specialized host adapters keep these lifecycle differences explicit instead of forcing every feature into one trait.

## Remaining design work

Before subsequent increments, settle the affected decisions:

- Which conventions should be shared, and which feature differences should remain explicit.
- The host/coordinator responsibilities and process boundaries, including failure and recovery behavior.
- How feature state, UI contributions, configuration reload, and task cleanup fit the existing Rust/Iced implementation.
- The next migration and its acceptance checks, preserving established behavior and evidence.

The first migration covers the existing bar, launcher, wallpaper, and notification frontends. The initial plugin refactor now covers all implemented UI features and the combined desktop service. Audio now uses a bounded pactl backend and a shared device panel; Bluetooth now follows the same shared-panel pattern with a direct BlueZ backend. Each later increment must preserve existing behavioral acceptance checks or explicitly document an agreed behavior change. Phase 3 is complete under the documented automated/synthetic acceptance scope.

## Adding a feature consistently

1. Keep backend policy and protocol handling testable without widgets. Use a separate crate when that separation is useful, as notification policy/transport and launcher discovery already do.
2. Put UI state, typed messages, subscriptions and views in `crates/shell/src/plugins/<feature>.rs` (or a feature directory when needed). Return typed effects/errors to the host; feature code should not start an Iced daemon or terminate the process. A static feature can supply only a view.
3. Put surface placement, window IDs, focus, output routing and application lifetime in `host/`. Reuse the desktop host for compatible contributions; use an adapter when the feature has different surface semantics. A new plugin does not imply a new executable or service.
4. Extend the production config schema, reference and complete examples together when settings are needed. Preserve the previous valid snapshot on failed reload.
5. Cover feature behavior independently, then add host/protocol integration and isolated graphical checks as appropriate. Keep helper tasks/processes supervised, and cancellation bounded.

The current feature entry points are statically selected by `host::Plugin`; they are not an external API. Production UI binaries build from the `cabby-shell` package. The historical standalone bar executable name `cabby-platform-spike` remains for CLI compatibility, but its source belongs to the shell package. Experimental lock/popup and graphical/IPC probes remain in the separate prototype package and are never production plugins.

The audio plugin is the first device-control contribution: the desktop host maps its typed events to a single on-demand panel, while the plugin subscription owns backend helpers. No audio service or executable is added for production. Device panels can follow this pattern without changing the lifecycle of passive desktop surfaces.


Audio and Bluetooth now share a typed device-panel slot in the desktop host. The host owns placement/focus/output lifetime and routes typed plugin messages; each plugin owns its view/state and backend subscription. Switching plugins drops the previous subscription. Bluetooth's backend remains independent of Iced. This concrete composition requires no dynamic registry, third-party API, or one-service-per-plugin convention.

The tray plugin owns its session-bus worker and bar icons. The desktop host reuses the panel slot for its menu surface; closing that surface leaves tray observation active. `cabby-tray` contains the UI-independent StatusNotifier/DBusMenu adapter and fixtures.

Bar contributions are selected by typed `BarWidget` values in three ordered config lists. Clock formatting and the read-only NetworkManager adapter remain independent of view construction; the bar starts subscriptions only for configured contributions. This is static in-repository composition, with no public plugin-loading API.

The lock plugin owns credential editing and authentication state; its dedicated host owns compositor confirmation, READY and the sole authenticated unlock exit. `cabby-lock` is a UI-independent PAM/policy crate, `cabby-auth` is its bounded helper process, and the `cabby-lock` executable belongs to `cabby-shell`. A separate compositor-bound service protects active locks from ordinary shell lifecycle changes.
