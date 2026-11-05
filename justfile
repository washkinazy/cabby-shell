# Build the production CLI and UI binaries.
build:
    cargo build --release --locked -p cabby -p cabby-shell -p cabby-lock --bins

# Build, install locally, reload user units, and validate user configuration.
install: build
    #!/usr/bin/env bash
    set -euo pipefail
    mkdir -p "$HOME/.local/bin"
    for binary in cabby cabby-auth cabby-lock cabby-desktop cabby-launcher cabby-notifications cabby-platform-spike cabby-wallpaper; do
        install -m 0755 "target/release/$binary" "$HOME/.local/bin/$binary"
    done
    systemctl --user daemon-reload
    "$HOME/.local/bin/cabby" validate

# Install Fedora build/runtime/test dependencies (uses sudo when needed).
deps:
    bash scripts/install-fedora-deps.sh

# Opt in to the Fedora PAM policy and local locker unit; does not start locking.
install-lock-support:
    bash scripts/install-lock-support.sh
