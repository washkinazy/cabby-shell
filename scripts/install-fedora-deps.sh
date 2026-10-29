#!/usr/bin/env bash
# Development and isolated integration-test dependencies. No services are enabled.
set -euo pipefail
packages=(
    cargo rust rustfmt clippy gcc pkgconf-pkg-config
    wayland-devel libxkbcommon-devel
    niri wayland-utils grim dbus-daemon systemd python3 util-linux
    mesa-dri-drivers mesa-vulkan-drivers
)
if [[ "${1:-}" == --print ]]; then
    printf '%s\n' "${packages[@]}"
elif [[ $# != 0 ]]; then
    echo "Usage: $0 [--print]" >&2
    exit 2
elif [[ $EUID == 0 ]]; then
    dnf install -y "${packages[@]}"
else
    sudo dnf install -y "${packages[@]}"
fi
