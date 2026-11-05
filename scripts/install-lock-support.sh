#!/usr/bin/env bash
# Explicit opt-in installation only. Never enables or starts a locker.
set -euo pipefail
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
unit_dir="$HOME/.config/systemd/user"
unit="$unit_dir/cabby-lock.service"
policy=/etc/pam.d/cabby-lock
staged="$(mktemp)"
trap 'rm -f "$staged"' EXIT
sed 's|/usr/bin/cabby lock-ui|%h/.local/bin/cabby lock-ui|' "$root/systemd/user/cabby-lock.service" > "$staged"
for binary in cabby cabby-lock cabby-auth; do
    test -x "$HOME/.local/bin/$binary" || { echo "Run just install first: missing $binary" >&2; exit 1; }
done
# Check both destinations before writing either; preserve custom policy/units.
if [[ -e "$unit" || -L "$unit" ]]; then
    cmp -s "$staged" "$unit" || { echo "Existing $unit differs; review it manually." >&2; exit 1; }
fi
if [[ -e "$policy" || -L "$policy" ]]; then
    cmp -s "$root/pam.d/cabby-lock" "$policy" || { echo "Existing $policy differs; review it manually." >&2; exit 1; }
fi
if [[ ! -e "$policy" ]]; then
    sudo install -o root -g root -m 0644 "$root/pam.d/cabby-lock" "$policy"
fi
mkdir -p "$unit_dir"
if [[ ! -e "$unit" ]]; then
    install -m 0644 "$staged" "$unit"
fi
systemctl --user daemon-reload
printf '%s\n' 'Lock support installed. No service started or keybinding changed.'
