# Internal plugin design intent

Status: intent recorded on 2026-11-04; architecture discussion pending. This note does not specify an implemented plugin API or authorize refactoring. Implementation work is paused for discussion after the completed notification increment.

## Purpose and scope

Cabby is a personal shell maintained in a public repository. Competing with other shells or building a platform for broad adoption is not the goal. Plugins are an internal design pattern to make adding features consistent, easier to understand, and easier to maintain.

Features such as audio, Bluetooth, wallpaper, and notifications are natural candidates for plugin boundaries. Plugins belong to this repository, compile with Cabby, and evolve with the project. There is no requirement for dynamic loading, third-party code permissions, distribution infrastructure, a stable public plugin API, or compatibility with independently released plugins.

The value should come from familiar feature structure and shared machinery. Introduce abstractions where working features show repeated needs; avoid building a general-purpose framework in advance. A feature should not need irrelevant methods merely to satisfy a universal interface.

## Responsibility boundaries to explore

The intended separation is that a plugin owns feature behavior and the shell owns how features participate in the interface and lifecycle. The following responsibilities are discussion guidance, not a settled Rust interface:

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

Existing ownership requirements continue to apply: every Cabby process and helper has a supervisor and shutdown policy; the active lock survives ordinary shell stop/restart; launched applications have independent ownership; BlueZ and PipeWire remain external services. The current implemented topology and reload behavior are documented in [services.md](services.md). No service, command, or configuration behavior changes as a result of this note.

## Discussion before implementation

Before refactoring, agree on:

- Which conventions should be shared, and which feature differences should remain explicit.
- The host/coordinator responsibilities and process boundaries, including failure and recovery behavior.
- How feature state, UI contributions, configuration reload, and task cleanup fit the existing Rust/Iced implementation.
- A small first migration and its acceptance checks, preserving established behavior and evidence.

No migration order, framework implementation, or audio/Bluetooth backend choice is settled here. Continue the architecture discussion with the user before starting refactoring or the next feature implementation. Phase 3 remains incomplete.
