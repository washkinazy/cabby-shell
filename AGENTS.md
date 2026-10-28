# Working on Cabby Shell

## Product direction

Read README.md for scope and PLAN.md for phases and acceptance criteria before changing code. Keep both current when a decision changes. Rust with Iced is the implementation direction to validate in Phase 0; systemd user services own the component lifecycle. No GTK interface, settings application, display manager, login manager, or Polkit agent.

User configuration belongs in files. Ordinary controls such as volume, Bluetooth pairing, and clipboard selection belong in the UI. Favor a small, coherent feature set over copying entire reference applications.

## Branch, commit, and merge workflow

The user has authorized local feature branches, validation, commits, and merges into main for these initial phases. A pull request is not required. Do not ask for permission again for these routine steps.

1. Inspect the current branch, working tree, and relevant instructions. Preserve unrelated changes. Never reset, discard, or amend someone else's work.
2. Start each reviewable feature or fix from current local main on a fresh branch: `feat/<topic>`, `fix/<topic>`, `docs/<topic>`, or `chore/<topic>`. Do not implement directly on main. A phase may contain several feature branches.
3. State the intended scope and acceptance criteria. Implement a coherent increment; avoid unrelated cleanup and premature framework abstractions.
4. Run checks appropriate to the change. Record commands, outcomes, and unavailable checks. Documentation-only changes need consistency, link/path, and whitespace checks, not a Cargo build.
5. Review the diff and stage explicit paths. Commit with a descriptive imperative subject. Never commit credentials, runtime state, clipboard history, captures, or generated build artifacts.
6. Merge validated branches into main with `git merge --no-ff <branch>` so each feature has an identifiable merge boundary. If main changed, validate the integrated result before calling it complete. Resolve routine conflicts while preserving intent; stop for genuinely ambiguous competing changes.
7. Record phase progress and remaining limitations in PLAN.md as part of the branch. Leave main checked out after the merge; create the next branch when the next unit of work begins. Do not mark a phase complete until its exit criteria pass.

Push only when requested or when the user establishes an automatic push policy. Never force-push or rewrite shared history. PRs become part of the workflow only when requested. If a required acceptance check cannot run, keep the branch unmerged and explain the missing evidence; optional future checks do not block a complete increment.

For regressions, create a fix branch from main and test the failing behavior. Prefer a new corrective commit or an explicitly authorized revert over history rewriting. Do not batch unrelated features into a phase-sized merge.

## Architecture and lifecycle

### Configuration contract

The default user configuration root is `$HOME/.config/cabby-shell`. Any supported XDG or command-line override must be explicitly documented and tested; do not silently change the default. Use subdirectories/files where they improve clarity, with documented discovery, relative-path resolution, and precedence.

Maintain `docs/configuration.md` as the user-facing configuration reference and `examples/config/` as the complete example configuration set. Every added, changed, renamed, deprecated, or removed setting must update the reference and affected examples in the same feature branch before merging. This applies to defaults and behavior changes even if the key name stays the same.

For every setting document its full key and file, purpose, type, default/omission behavior, valid values/range and units, example, dependencies/interactions, and reload-versus-restart behavior. Explain migration for incompatible changes. Examples must cover every supported setting, including alternatives that need separate mutually exclusive examples, and contain no private paths or credentials.

Once the parser exists, validate every runnable example with the production parser and semantic validator. Keep an automated setting-coverage check tied to the actual schema/metadata so adding a setting without documentation/example coverage fails validation. Do not add speculative config keys before implementation; label future layout/format proposals explicitly. Never overwrite a user's configuration during install or upgrade.

### Implementation guidelines

- Use Rust and Iced for Cabby-owned implementation, subject to Phase 0 validation. Native system libraries, existing Linux services, and wrapped tools such as grim are allowed dependencies; reimplementing them in Rust is not a requirement. Invoke helpers with explicit argument arrays, handle errors/timeouts/cancellation, and keep their processes under supervision.
- Keep service logic independent of UI widgets. Use typed commands/events and event subscriptions; avoid recurring command execution or polling when a suitable event interface exists.
- Start with a few processes justified by lifecycle or isolation needs. Do not create one daemon per widget.
- Every Cabby process and child must have an explicit supervisor and shutdown policy. Use systemd user units, cgroup cleanup, bounded restart behavior, and compositor-session dependencies. A target alone does not establish process ownership or crash propagation.
- Keep the active lock independent of ordinary UI restart/stop. Never unlock as a cleanup action. Define locker failure recovery and session-ending behavior before enabling it for daily use.
- BlueZ, PipeWire, and other shared system services are dependencies, not processes Cabby should terminate. Applications opened by the launcher are user applications and must not remain in a Cabby service's cleanup cgroup.
- Handle missing dependencies, disconnects, output changes, and invalid config explicitly. Keep the previous valid configuration on reload failure.
- Keep persistent preferences, runtime state, and caches separate using XDG locations. Treat clipboard content, notification bodies, credentials, and screenshots as sensitive data; do not log their contents.
- Pin the tested dependency combination and commit Cargo.lock once the Rust workspace exists. Record decisions and measured limitations; do not claim performance or compatibility without evidence.
- Maintain `scripts/install-fedora-deps.sh` and `docs/development.md` whenever a build or test prerequisite changes. The installer must list the full Fedora development/test package set, support a non-mutating `--print` mode, and never enable services or overwrite user configuration.

## Testing and reporting

Follow the test tiers and event matrix in PLAN.md. Prefer behavior tests for policy, protocol boundaries, and recovery. Do not write tests that simply duplicate implementation or add tests for trivial documentation edits.

Prioritize automated and synthetic tests. The user has a single physical output: physical multi-monitor acceptance is deferred, not a merge or phase gate. Test output models and geometry synthetically where practical and label hardware assumptions as unverified. Skip manual popup acceptance; automate what can reasonably be exercised and record remaining coverage gaps. Do not repeatedly ask for manual testing or block progress solely on unavailable hardware/untestable interactions. This does not convert a failed automated check into a pass or authorize disruptive live-session tests.

Once a Cargo workspace exists, the standard merge checks are `cargo fmt --all -- --check`, `cargo check --workspace --all-targets --locked`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, and `cargo test --workspace --locked`. Add feature-specific integration checks. Feature combinations must be deliberately supported before adding them to CI; do not blindly enable mutually exclusive backends with `--all-features`.

Run systemd unit verification for unit changes. Use private D-Bus sessions, test-specific units/config/state, and a nested compositor or disposable VM for integration. Do not replace the live notification owner, lock the user's current session, change real audio/Bluetooth state, suspend the host, or kill the user's compositor as an incidental test. Arrange an explicit live test window for checks that cannot be isolated.

Distinguish unit-tested behavior, nested-session observations, and real-hardware validation. Nested success does not prove real lock security, display power behavior, or gamma support. Report what changed, what passed, what was skipped, and the branch/merge result. Do not leave test processes running.
