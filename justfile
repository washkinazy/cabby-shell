# Build the production CLI and UI binaries.
build:
    cargo build --release --locked -p cabby -p cabby-shell --bins

# Build, install locally, reload user units, and validate user configuration.
install: build
    #!/usr/bin/env bash
    set -euo pipefail
    mkdir -p "$HOME/.local/bin"
    for binary in cabby cabby-desktop cabby-launcher cabby-notifications cabby-platform-spike cabby-wallpaper; do
        install -m 0755 "target/release/$binary" "$HOME/.local/bin/$binary"
    done
    systemctl --user daemon-reload
    "$HOME/.local/bin/cabby" validate
