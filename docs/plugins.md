# Internal plugin design intent

Status: intent recorded on 2026-11-04; the user subsequently authorized starting the refactor. The first increment implements built-in feature modules and shared startup. Process consolidation and shared UI composition remain future design work.

## Purpose and scope

Cabby is a personal shell maintained in a public repository. Competing with other shells or building a platform for broad adoption is not the goal. Plugins are an internal design pattern to make adding features consistent, easier to understand, and easier to maintain.

Features such as audio, Bluetooth, wallpaper, and notifications are natural candidates for plugin boundaries. Plugins belong to this repository, compile with Cabby, and evolve with the project. There is no requirement for dynamic loading, third-party code permissions, distribution infrastructure, a stable public plugin API, or compatibility with independently released plugins.

The value should come from familiar feature structure and shared machinery. Introduce abstractions where working features show repeated needs; avoid building a general-purpose framework in advance. A feature should not need irrelevant methods merely to satisfy a universal interface.

## Responsibility boundaries to explore

The intended separation is that a plugin owns feature behavior and the shell owns how features participate in the interface and lifecycle. The following responsibilities guide the direction; the first increment implements only the shared startup and module boundaries described below:

| Feature/plugin responsibility | Shared shell responsibility |
| --- | --- |
| Backend state, system-service subscriptions, typed commands and events | Common task ownership, cancellation, and error presentation conventions |
| Feature UI and interactions | Bar/panel composition, surface placement, shared styling, and output tracking |
| Feature configuration schema and validation | Configuration discovery, complete validation, and coordinated reload |
| Release of feature-owned tasks and helpers | Supervised process lifetime and coordinated shutdown |

Keep backend logic testable independently of Iced widgets. Features may contribute different kinds of UI: audio could provide a bar indicator and device panel; wallpaper needs background surfaces. This does not commit to configurable bar composition, new settings, a generic event bus, or any particular registry or trait design.

## Plugins and services are separate decisions

A plugin is a code and responsibility boundary. A process or systemd service is a supervision, isolation, and lifetime boundary. A plugin does not automatically get its own daemon. Several plugins could share a process; a separate process needs a concrete lifecycle or isolation reason.

The number of hosts, the role of the existing coordinator, and placement of individual features remain open. The earlier idea of one ordinary shell host is a candidate for discussion, not an approved topology. Sharing a process also shares crash impact; an internal plugin contract alone does not provide process isolation.

Existing ownership requirements continue to apply: every Cabby process and helper has a supervisor and shutdown policy; the active lock survives ordinary shell stop/restart; launched applications have independent ownership; BlueZ and PipeWire remain external services. The current implemented topology and reload behavior are documented in [services.md](services.md). The first refactor increment preserves existing services, commands, configuration, and reload behavior.

## Implemented first increment

`crates/shell` now owns the production UI implementation. Its `host::Plugin` enum registers Bar, Launcher, Wallpaper, and Notifications; `host::run` validates configuration once, applies renderer selection before worker threads start, initializes diagnostics, and applies the existing clipboard policy. The launcher retains text-input clipboard support; other migrated features disable it. A typed startup context carries configuration and its source to the selected plugin.

Feature modules under `crates/shell/src/plugins/` own their typed messages, state, subscriptions, views, and surface settings. Existing notification policy/transport and launcher discovery crates remain separate from UI widgets. Shared theme and helper invocation live in `ui`; the niri client remains separately testable. There is no universal plugin trait or untyped command bus.

The production binaries in `crates/platform-spike` are now thin selectors for the built-in plugins. Their names remain stable for the CLI, units, and test harness; guarded probes remain test-only. The host runs one selected plugin per existing process. This establishes code boundaries and removes repeated startup code; it does not yet compose several plugins into one event loop or reduce the number of services.

To add a built-in feature at this stage, keep backend policy independently testable, add its UI module, and register a typed entry point with the host when a standalone surface is appropriate. A bar indicator or shared panel should motivate the next composition interface; this convention does not require a new executable/service for every future feature. Extend configuration through the existing production schema, reference, examples, and coverage checks.

## Remaining design work

Before subsequent increments, settle the affected decisions:

- Which conventions should be shared, and which feature differences should remain explicit.
- The host/coordinator responsibilities and process boundaries, including failure and recovery behavior.
- How feature state, UI contributions, configuration reload, and task cleanup fit the existing Rust/Iced implementation.
- The next migration and its acceptance checks, preserving established behavior and evidence.

The first migration covers the existing bar, launcher, wallpaper, and notification frontends. Further process changes and audio/Bluetooth backend choices remain open. Each later increment must preserve existing behavioral acceptance checks or explicitly document an agreed behavior change. Phase 3 remains incomplete.
