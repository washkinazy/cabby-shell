#!/usr/bin/env bash
# Explicit opt-in installation only. Never enables or starts a locker.
set -euo pipefail
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
unit="${XDG_DATA_HOME:-$HOME/.local/share}/systemd/user/cabby-lock.service"
policy=/etc/pam.d/cabby-lock
[[ -f "$unit" ]] || { echo 'Run just install first: locker unit is missing.' >&2; exit 1; }
for binary in cabby cabby-lock cabby-auth; do
    test -x "$HOME/.local/bin/$binary" || { echo "Run just install first: missing $binary" >&2; exit 1; }
done
# Preserve custom administrator-owned PAM policy.
if [[ -e "$policy" || -L "$policy" ]]; then
    cmp -s "$root/pam.d/cabby-lock" "$policy" || { echo "Existing $policy differs; review it manually." >&2; exit 1; }
fi
if [[ ! -e "$policy" ]]; then
    sudo install -o root -g root -m 0644 "$root/pam.d/cabby-lock" "$policy"
fi
printf '%s\n' 'Lock authentication support installed. No service started or keybinding changed.'
