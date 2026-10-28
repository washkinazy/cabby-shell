# Configuration reference

## Status

Cabby is in the planning stage. No configuration parser or settings are implemented yet. TOML is the proposed format; entrypoint filenames and any component files will be finalized with the parser in Phase 1. There are no runnable configuration examples yet, and this document does not claim otherwise.

## Location and ownership

The default configuration directory is `$HOME/.config/cabby-shell`. All Cabby user configuration files and any configuration subdirectories belong under this root. Shared themes or component configuration may be split into files when useful; each file must have a documented purpose and loading rule.

Any supported override, including `XDG_CONFIG_HOME` or a command-line path, must be documented here when implemented, together with exact precedence. Environment-variable and tilde expansion inside configuration values must not be assumed. Document path resolution for each path-valued setting and how relative paths resolve across subfiles.

Configuration is user-owned: installation and upgrades must not overwrite it. Runtime sockets, logs, caches, clipboard history, and other generated state do not belong in this directory. Their separate locations will be documented when implemented.

## Reference organization

As features land, this reference will describe:

- The entrypoint and directory tree, required/optional files, and missing-file behavior.
- Loading order, overrides, includes if supported, and duplicate/unknown-key handling.
- How to install the [example configurations](../examples/config/README.md), validate changes, and reload or restart affected components.
- Every supported setting, organized by feature, with links to the corresponding examples.
- Compatibility and migration notes for changed or removed settings.

Each setting's entry must specify:

| Field | Required description |
| --- | --- |
| File and key | Exact file location and complete setting key |
| Purpose | What user-visible behavior it controls |
| Type | Boolean, string, number, array, table, or other supported form |
| Default | Actual default and behavior when omitted, including environment-dependent values |
| Accepted values | Enum options, bounds, units, formats, and path semantics as applicable |
| Example | Valid representative value and link to a runnable example |
| Interactions | Dependencies, conflicts, prerequisites, or related settings; explicitly say when none |
| Application | Whether the change applies on reload or requires a component/session restart |
| Migration | Replacement or migration steps when compatibility changes |

## Documentation and example maintenance

Adding a feature is incomplete until its configuration is documented and represented in the example set. Key additions, removals, renames, deprecations, new enum choices, default changes, and changed behavior require updates in the same branch as the implementation.

The example set must cover all supported settings. Provide separate examples for incompatible alternatives rather than presenting an invalid combined configuration. Every runnable example must pass the production parser and semantic validation. Automated coverage checks must compare the implementation's schema/metadata with reference and example coverage once the configuration system exists.

Keep examples portable and synthetic. Explain any hardware-specific values or prerequisites, and distinguish runnable configurations from illustrative snippets. Tests must not install examples over a user's real configuration.
