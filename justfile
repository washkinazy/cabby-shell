# Build the production CLI and UI binaries.
build:
    cargo build --release --locked -p cabby -p cabby-shell -p cabby-lock --bins

# Build, install locally, reload user units, and validate user configuration.
install: build
    python3 scripts/install-local.py install

# Remove the local install after Cabby is stopped and the session unlocked.
uninstall:
    python3 scripts/install-local.py uninstall

# Install Fedora build/runtime/test dependencies (uses sudo when needed).
deps:
    bash scripts/install-fedora-deps.sh

# Opt in to the Fedora PAM policy; does not start locking.
install-lock-support:
    bash scripts/install-lock-support.sh
