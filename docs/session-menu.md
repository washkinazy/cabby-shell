# Session menu

Run `cabby session-menu` to toggle the desktop's session menu. The dotfiles bind **Ctrl+Alt+Delete** to this command with key repeat disabled. Install the new build before applying that binding. A running coordinator and desktop are required; `--runtime-dir PATH` selects the same runtime override as other coordinator controls. The private desktop socket accepts menu activation, never raw power actions.

The menu opens in the center of the focused output when focus information is available, otherwise on the last selected/first available output. It uses the existing desktop panel slot, opaque Fluent surfaces, vector icons and configured typography/palette. Opening it closes the previous device/tray/picker panel. Escape, Close, outside click, a repeated activation, output removal or ordinary focus loss dismisses it. Ordinary valid config reload preserves its state and repositions it; renderer changes restart the desktop.

- **Lock** immediately requests the independent compositor-confirmed locker.
- **Suspend** immediately starts the same lock-confirmation/session recheck path as `cabby suspend`, then requests sleep with inhibitor checks.
- **Log out** asks for confirmation, then stops the compositor's user service (`niri.service` by default), ending the graphical session. It does not terminate every session belonging to the Unix user.
- **Reboot** and **Power off** ask for confirmation, then submit the corresponding inhibitor-respecting systemd request.

Up/Down selects a row; Enter chooses it. Destructive confirmations initially select **Cancel**; use Left/Right (or Up/Down) to change the selection and Enter to activate it. Mouse buttons work directly. Holding Enter on a destructive row cannot confirm the action without first choosing its confirmation control.

Only one helper runs at a time. Controls coalesce while it is pending, with a 60-second UI-client deadline covering lock readiness and the bounded CLI operations. Lock acquisition can take keyboard focus; that focus change preserves the pending request so suspend can finish. Closing/replacing the menu cancels its owned client. An already accepted systemd lock or power job cannot be withdrawn, and cancellation never unlocks an accepted locker. Successful requests dismiss the menu; rejected requests retain it with an error. Suspend failures point to Caffeine or unavailable lock support; existing independent lockers remain untouched by UI cleanup.

The menu does not install PAM policy, enable units, acquire elevated privileges or start an authorization agent. Lock/suspend require the existing lock/PAM setup. Missing services, failed readiness or denied authorization are reported as failed requests. Real machine power transitions are separate from request acceptance.

Direct explicit commands `cabby logout`, `cabby reboot` and `cabby poweroff` do not display menu confirmation. Each first verifies an active compositor user unit; `--session-unit NAME.service` overrides it. Reboot/poweroff use `systemctl --no-ask-password --check-inhibitors=yes --no-block`; log out uses `systemctl --no-ask-password --user --no-block stop NAME.service`. These use literal argument arrays and bounded owned clients, with no shell evaluation. `cabby lock`/`cabby suspend` retain their existing unit overrides and safety contract.

No new daemon, installed binary, user unit or configuration key is added. Isolated command fixtures intercept every systemctl call. Native renderer acceptance copies the desktop beside a recording-only CLI, so clicking any menu action cannot lock, suspend, log out, reboot or power off the host. Keyboard policy and the lock-to-suspend focus handoff have synthetic tests. Physical keybind delivery and actual power authorization remain live-use validation items.
