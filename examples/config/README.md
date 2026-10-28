# Example configurations

This directory will contain Cabby's complete, annotated example configuration set. The default installation root for user configuration is `$HOME/.config/cabby-shell`.

No configuration settings or parser exist yet, so no runnable files are provided at this stage. Phase 1 will introduce the example files with the implemented schema rather than publishing speculative settings.

## Required example set

- A minimal runnable configuration demonstrating which settings can be omitted.
- A complete annotated configuration, or documented group of component files, covering every supported setting.
- Focused runnable variants where mutually exclusive settings or feature-specific behavior cannot be represented in a single example.

The directory structure should mirror the installed configuration structure where practical. Add an index of files here as they land, explaining their purpose, destination, prerequisites, and how to validate/use them. Link settings to the [configuration reference](../../docs/configuration.md).

## Maintenance requirement

Every feature/setting change must update the relevant examples and reference in the same branch. This includes removals, new accepted values, and changed defaults or semantics. Keep comments consistent with runtime behavior, cover all settings, and use portable sample values without credentials or personal data.

Once the parser exists, all runnable examples must pass its semantic validation, and automated coverage checks must detect missing settings. Do not mistake this directory's documentation scaffold for a completed example set.
