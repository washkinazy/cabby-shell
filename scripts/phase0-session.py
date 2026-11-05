#!/usr/bin/env python3
"""Disposable nested niri experiment; never imports a global session environment."""
import argparse
import json
import os
import re
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]


def run(*args):
    return subprocess.run(args, check=True, capture_output=True, text=True, timeout=15).stdout


def selected_adapter(log):
    """Read the adapter actually selected by Iced, not the enumeration list."""
    match = re.search(r"Selected: AdapterInfo \{(.*?)\n\}", log, re.S)
    if not match:
        raise RuntimeError("No selected GPU adapter in renderer diagnostics")
    block = match.group(1)
    name = re.search(r'name: "([^"]+)"', block)
    backend = re.search(r'backend: (\w+)', block)
    kind = re.search(r'device_type: (\w+)', block)
    if not (name and backend and kind):
        raise RuntimeError("Incomplete GPU adapter diagnostics")
    return {"name": name.group(1), "backend": backend.group(1), "device_type": kind.group(1)}


def inside(directory, interactive, renderer):
    """Invoked by the nested compositor, with its private display and IPC socket."""
    directory = Path(directory)
    runtime = directory / "runtime"
    display = Path(os.environ["WAYLAND_DISPLAY"])
    display = display if display.is_absolute() else runtime / display
    if display.parent != runtime or not display.is_socket():
        raise RuntimeError("Refusing to test outside the disposable compositor")
    os.environ["CABBY_TEST_WAYLAND"] = os.environ["WAYLAND_DISPLAY"]
    os.environ["WAYLAND_DEBUG"] = "1"
    # Scope these overrides to probe children. Inherited WGPU_BACKEND=gl caused
    # the original failure; never mutate the user's global environment.
    os.environ["ICED_BACKEND"] = "wgpu" if renderer == "gpu" else "tiny-skia"
    os.environ["WGPU_BACKEND"] = "vulkan"
    os.environ["CABBY_RENDERER_DIAGNOSTICS"] = "1"
    children = []
    try:
        protocols = run("wayland-info")
        (directory / "protocols.txt").write_text(protocols)
        for protocol in ("zwlr_layer_shell_v1", "ext_session_lock_manager_v1", "zwlr_screencopy_manager_v1"):
            if protocol not in protocols:
                raise RuntimeError(f"Required probe protocol missing: {protocol}")
        outputs = json.loads(run("niri", "msg", "--json", "outputs"))
        (directory / "outputs.json").write_text(json.dumps(outputs, indent=2))
        config = directory / "cabby.toml"
        config.write_text(f'renderer = "{"vulkan" if renderer == "gpu" else "software"}"\nbar_height = 40\nbar_position = "{"top" if renderer == "gpu" else "bottom"}"\n')
        with (directory / "bar.log").open("w") as log:
            bar = subprocess.Popen([ROOT / "target/debug/cabby", "ui", "--config", config], stdout=log, stderr=log)
            children.append(bar)
            deadline = time.monotonic() + 30
            while time.monotonic() < deadline:
                if bar.poll() is not None:
                    raise RuntimeError("Bar exited; see bar.log")
                layers = run("niri", "msg", "--json", "layers")
                rendered = ".attach(wl_buffer" in (directory / "bar.log").read_text()
                if "cabby-bar" in layers and rendered:
                    break
                time.sleep(0.2)
            else:
                raise RuntimeError("Bar did not map within 30 seconds")
            (directory / "layers.json").write_text(layers)
            probe = subprocess.run([ROOT / "target/debug/bar-ipc-probe"], capture_output=True, text=True, timeout=15)
            (directory / "bar-ipc-probe.log").write_text(probe.stdout + probe.stderr)
            probe.check_returncode()
            wallpaper = None
            for mode in ("solid", "fill", "fit", "stretch", "center"):
                if wallpaper is not None:
                    wallpaper.terminate()
                    wallpaper.wait(timeout=5)
                wallpaper_config = directory / f"wallpaper-{mode}.toml"
                wallpaper_config.write_text(config.read_text() + f'wallpaper_color = "#19334d"\nwallpaper_mode = "{"fill" if mode == "solid" else mode}"\n' +
                                            ('' if mode == "solid" else f'wallpaper_image = "{ROOT / "examples/config/assets/sample.png"}"\n'))
                wallpaper_log_path = directory / f"wallpaper-{mode}.log"
                with wallpaper_log_path.open("w") as wallpaper_log:
                    wallpaper = subprocess.Popen([ROOT / "target/debug/cabby", "wallpaper", "--config", wallpaper_config], stdout=wallpaper_log, stderr=wallpaper_log)
                    children.append(wallpaper)
                    deadline = time.monotonic() + 20
                    while time.monotonic() < deadline:
                        if wallpaper.poll() is not None:
                            raise RuntimeError(f"Wallpaper {mode} exited")
                        if "cabby-wallpaper" in run("niri", "msg", "--json", "layers") and ".attach(wl_buffer" in wallpaper_log_path.read_text():
                            break
                        time.sleep(0.1)
                    else:
                        raise RuntimeError(f"Wallpaper {mode} did not map")
                    time.sleep(0.3)
                    capture = directory / f"wallpaper-{mode}.png"
                    subprocess.run(["grim", str(capture)], check=True, timeout=15)
                    pixels = subprocess.run([ROOT / "target/debug/wallpaper-pixel-probe", capture, mode], capture_output=True, text=True, timeout=15)
                    (directory / f"wallpaper-{mode}-pixels.txt").write_text(pixels.stdout + pixels.stderr)
                    pixels.check_returncode()
                    if renderer == "gpu":
                        adapter = selected_adapter(wallpaper_log_path.read_text())
                        if adapter["backend"] != "Vulkan" or adapter["device_type"] not in ("DiscreteGpu", "IntegratedGpu"):
                            raise RuntimeError("Wallpaper did not select hardware Vulkan")
            (directory / "wallpaper.log").write_text(wallpaper_log_path.read_text())
            time.sleep(1)
            subprocess.run(["grim", str(directory / "capture.png")], check=True, timeout=15)
            if (directory / "capture.png").read_bytes()[:8] != b"\x89PNG\r\n\x1a\n":
                raise RuntimeError("Capture did not produce a PNG")
            applications = directory / "data/applications"
            applications.mkdir(parents=True, exist_ok=True)
            (applications / "cabby-notes.desktop").write_text('[Desktop Entry]\nType=Application\nName=Cabby Notes\nExec=/usr/bin/true\n')
            (applications / "cabby-terminal.desktop").write_text('[Desktop Entry]\nType=Application\nName=Cabby Terminal\nExec=/usr/bin/true\nTerminal=true\n')
            # XDG_DATA_DIRS also locates Vulkan ICD manifests; preserve it for graphics.
            with (directory / "notifications.log").open("w") as notifications_log:
                notifications = subprocess.Popen([ROOT / "target/debug/cabby", "notifications", "--config", config], stdout=notifications_log, stderr=notifications_log)
                children.append(notifications)
                probe_bin = str(ROOT / "target/debug/notification-probe")
                deadline = time.monotonic() + 15
                while True:
                    seeded = subprocess.run([probe_bin, "notify"], capture_output=True, text=True, timeout=12)
                    if seeded.returncode == 0:
                        notice_id = seeded.stdout.strip()
                        break
                    if notifications.poll() is not None or time.monotonic() > deadline:
                        raise RuntimeError("Notification service did not become ready")
                    time.sleep(0.1)
                def notification_layers():
                    return sum(layer.get("namespace") == "cabby-notifications" for layer in json.loads(run("niri", "msg", "--json", "layers")))
                def wait_notices(count):
                    deadline = time.monotonic() + 10
                    while time.monotonic() < deadline:
                        if notification_layers() == count:
                            return
                        if notifications.poll() is not None:
                            raise RuntimeError("Notification UI exited")
                        time.sleep(0.1)
                    raise RuntimeError(f"Expected {count} notification surfaces")
                wait_notices(1)
                deadline = time.monotonic() + 15
                while ".attach(wl_buffer" not in (directory / "notifications.log").read_text():
                    if time.monotonic() > deadline:
                        raise RuntimeError("Notification buffer not attached")
                    time.sleep(0.1)
                run(probe_bin, "notify", notice_id)
                wait_notices(1)
                extra = [run(probe_bin, "notify").strip() for _ in range(3)]
                wait_notices(3)
                deadline = time.monotonic() + 10
                while len(set(re.findall(r"wl_surface#(\d+)\.attach\(wl_buffer", (directory / "notifications.log").read_text()))) < 3:
                    if time.monotonic() > deadline:
                        raise RuntimeError("Three notification buffers did not render")
                    time.sleep(0.1)
                time.sleep(0.2)
                run("grim", str(directory / "notifications.png"))
                edge = "top" if renderer == "gpu" else "bottom"
                run(str(ROOT / "target/debug/wallpaper-pixel-probe"), str(directory / "notifications.png"), f"notifications-{edge}")
                for name in outputs:
                    run("niri", "msg", "output", name, "scale", "1.5")
                time.sleep(0.5)
                assert notifications.poll() is None
                run("grim", str(directory / "notifications-scaled.png"))
                run(str(ROOT / "target/debug/wallpaper-pixel-probe"), str(directory / "notifications-scaled.png"), f"notifications-scaled-{edge}")
                duplicate = subprocess.run([ROOT / "target/debug/cabby", "notifications", "--config", config], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=10)
                assert duplicate.returncode == 73, "notification owner conflict must exit 73"
                run(probe_bin, "close", notice_id)
                wait_notices(3)
                for notice in extra:
                    run(probe_bin, "close", notice)
                wait_notices(0)
                run(probe_bin, "expire")
                wait_notices(0)
                trace = (directory / "notifications.log").read_text()
                assert not re.search(r"wl_keyboard#\d+\.enter\(", trace), "passive notifications took keyboard focus"
                assert "set_keyboard_interactivity(0)" in trace
                notifications.terminate()
                notifications.wait(timeout=5)

            launcher_env = dict(os.environ)
            with (directory / "launcher.log").open("w") as launcher_log:
                launcher = subprocess.Popen([ROOT / "target/debug/cabby", "launcher-ui", "--config", config], env=launcher_env, stdout=launcher_log, stderr=launcher_log)
                children.append(launcher)
                deadline = time.monotonic() + 30
                while time.monotonic() < deadline:
                    if launcher.poll() is not None:
                        raise RuntimeError("Launcher exited; see launcher.log")
                    launcher_layers = json.loads(run("niri", "msg", "--json", "layers"))
                    launcher_trace = (directory / "launcher.log").read_text()
                    if "cabby-launcher" in json.dumps(launcher_layers) and ".attach(wl_buffer" in launcher_trace and re.search(r"wl_keyboard#\d+\.enter\(", launcher_trace):
                        break
                    time.sleep(0.2)
                else:
                    raise RuntimeError("Launcher did not map and receive keyboard focus")
                time.sleep(0.5)
                subprocess.run(["grim", str(directory / "launcher.png")], check=True, timeout=15)
                launcher.terminate()
                launcher.wait(timeout=5)
            for name in outputs:
                run("niri", "msg", "output", name, "scale", "1.5")
            time.sleep(1)
            if bar.poll() is not None or wallpaper.poll() is not None:
                raise RuntimeError("Bar or wallpaper exited after scale change")
            scaled_capture = directory / "wallpaper-scaled.png"
            subprocess.run(["grim", str(scaled_capture)], check=True, timeout=15)
            run(str(ROOT / "target/debug/wallpaper-pixel-probe"), str(scaled_capture), "center")
            scaled = run("niri", "msg", "--json", "outputs")
            (directory / "scaled-outputs.json").write_text(scaled)
            for name in outputs:
                if json.loads(scaled)[name]["logical"]["scale"] != 1.5:
                    raise RuntimeError(f"Output {name} did not adopt the requested scale")
            # Exercise the production desktop wrapper with both plugins in one host.
            bar.terminate()
            bar.wait(timeout=5)
            wallpaper.terminate()
            wallpaper.wait(timeout=5)
            with (directory / "composition.log").open("w") as composition_log:
                bar = subprocess.Popen([ROOT / "target/debug/cabby", "desktop", "--config", wallpaper_config], stdout=composition_log, stderr=composition_log)
                children.append(bar)
                deadline = time.monotonic() + 30
                while time.monotonic() < deadline:
                    if bar.poll() is not None:
                        raise RuntimeError("Composed plugin host exited")
                    composed_layers = json.loads(run("niri", "msg", "--json", "layers"))
                    counts = {ns: sum(layer.get("namespace") == ns for layer in composed_layers) for ns in ("cabby-bar", "cabby-wallpaper")}
                    trace = (directory / "composition.log").read_text()
                    if all(count == len(outputs) for count in counts.values()) and len(set(re.findall(r"wl_surface#(\d+)\.attach\(wl_buffer", trace))) >= 2 * len(outputs):
                        break
                    time.sleep(0.1)
                else:
                    raise RuntimeError("Composed host did not render both plugin surfaces")
                for scale in (1, 1.5):
                    for name in outputs:
                        run("niri", "msg", "output", name, "scale", str(scale))
                    time.sleep(0.7)
                    capture = directory / f"composition-{scale}.png"
                    subprocess.run(["grim", str(capture)], check=True, timeout=15)
                    run(str(ROOT / "target/debug/wallpaper-pixel-probe"), str(capture), "center")
                    run(str(ROOT / "target/debug/wallpaper-pixel-probe"), str(capture), "bar-top" if renderer == "gpu" else "bar-bottom")
                    if bar.poll() is not None:
                        raise RuntimeError("Composed host exited after scale change")
                run(str(ROOT / "target/debug/bar-ipc-probe"))
            # Run the real panel/worker with an explicit private helper, never live audio.
            bar.terminate()
            bar.wait(timeout=5)
            audio_fixture = directory / "audio-fixture"
            audio_fixture.mkdir()
            for name in ("audio-helper.py", "audio-state.json"):
                (audio_fixture / name).write_bytes((ROOT / "scripts/fixtures" / name).read_bytes())
            helper = audio_fixture / "audio-helper.py"
            helper.chmod(0o700)
            audio_env = dict(os.environ, CABBY_TEST_AUDIO_HELPER=str(helper))
            with (directory / "audio.log").open("w") as audio_log:
                bar = subprocess.Popen([ROOT / "target/debug/audio-panel-probe", "--config", wallpaper_config], env=audio_env, stdout=audio_log, stderr=audio_log)
                children.append(bar)
                deadline = time.monotonic() + 20
                while time.monotonic() < deadline:
                    trace = (directory / "audio.log").read_text()
                    audio_layers = json.loads(run("niri", "msg", "--json", "layers"))
                    args_path = audio_fixture / "audio-args"
                    if any(layer.get("namespace") == "cabby-audio" for layer in audio_layers) and args_path.exists() and '"sources"' in args_path.read_text() and re.search(r"wl_keyboard#\d+\.enter\(", trace):
                        break
                    if bar.poll() is not None:
                        raise RuntimeError("Audio panel probe exited")
                    time.sleep(0.1)
                else:
                    raise RuntimeError("Audio panel did not connect and receive keyboard focus")
                time.sleep(0.5)
                capture = directory / "audio.png"
                subprocess.run(["grim", str(capture)], check=True, timeout=15)
                run(str(ROOT / "target/debug/wallpaper-pixel-probe"), str(capture), "audio-top" if renderer == "gpu" else "audio-bottom")
                # Probe-only one-shot dismissal uses the same close path as the UI.
                deadline = time.monotonic() + 15
                while any(layer.get("namespace") == "cabby-audio" for layer in json.loads(run("niri", "msg", "--json", "layers"))):
                    if time.monotonic() > deadline:
                        raise RuntimeError("Audio panel did not dismiss")
                    time.sleep(0.1)
                pids = (audio_fixture / "audio-pids").read_text().splitlines()
                deadline = time.monotonic() + 3
                while any(Path("/proc", pid).exists() for pid in pids):
                    if time.monotonic() > deadline:
                        raise RuntimeError("Audio helpers survived panel dismissal")
                    time.sleep(0.05)
                if bar.poll() is not None:
                    raise RuntimeError("Panel dismissal stopped desktop")
            # Exercise discovery + a pending pairing prompt against private BlueZ.
            bar.terminate()
            bar.wait(timeout=5)
            bluetooth_env = dict(os.environ, CABBY_TEST_BLUEZ_BUS=os.environ["DBUS_SESSION_BUS_ADDRESS"])
            with (directory / "bluez-fixture.log").open("w") as fixture_log, (directory / "bluetooth.log").open("w") as bluetooth_log:
                fixture = subprocess.Popen([ROOT / "target/debug/examples/bluez-fixture"], env=bluetooth_env, stdout=fixture_log, stderr=fixture_log)
                children.append(fixture)
                deadline = time.monotonic() + 10
                while "fixture-ready" not in (directory / "bluez-fixture.log").read_text():
                    if fixture.poll() is not None or time.monotonic() > deadline:
                        raise RuntimeError("Private BlueZ fixture did not start")
                    time.sleep(0.05)
                bar = subprocess.Popen([ROOT / "target/debug/bluetooth-panel-probe", "--config", wallpaper_config], env=bluetooth_env, stdout=bluetooth_log, stderr=bluetooth_log)
                children.append(bar)
                deadline = time.monotonic() + 20
                while True:
                    trace = (directory / "bluetooth.log").read_text()
                    if "bluetooth-probe-prompt" in trace and re.search(r"wl_keyboard#\d+\.enter\(", trace):
                        break
                    if bar.poll() is not None or time.monotonic() > deadline:
                        raise RuntimeError("Bluetooth panel did not show pairing prompt with focus")
                    time.sleep(0.05)
                status_args = ("busctl", "--address=" + bluetooth_env["CABBY_TEST_BLUEZ_BUS"], "call", "org.bluez", "/test", "org.cabby.Test", "Status")
                if run(*status_args).strip() != "bu true 1":
                    raise RuntimeError("Bluetooth fixture has no owned discovery session")
                time.sleep(0.5)
                capture = directory / "bluetooth.png"
                subprocess.run(["grim", str(capture)], check=True, timeout=15)
                run(str(ROOT / "target/debug/wallpaper-pixel-probe"), str(capture), "audio-top" if renderer == "gpu" else "audio-bottom")
                deadline = time.monotonic() + 15
                while any(layer.get("namespace") == "cabby-bluetooth" for layer in json.loads(run("niri", "msg", "--json", "layers"))):
                    if time.monotonic() > deadline:
                        raise RuntimeError("Bluetooth panel did not dismiss")
                    time.sleep(0.1)
                deadline = time.monotonic() + 5
                while run(*status_args).strip() != "bu false 0":
                    if time.monotonic() > deadline:
                        raise RuntimeError("Bluetooth discovery or agent survived dismissal")
                    time.sleep(0.05)
                if bar.poll() is not None:
                    raise RuntimeError("Bluetooth dismissal stopped desktop")
                fixture.terminate()
                fixture.wait(timeout=5)
            if interactive:
                (directory / "interactive-ready").touch()
                # User can inspect the bar before creating this request file.
                while not (directory / "request-lock").exists():
                    if bar.poll() is not None:
                        raise RuntimeError("Bar exited during manual test")
                    time.sleep(0.2)
            with (directory / "lock.log").open("w") as lock_log:
                lock = subprocess.Popen([ROOT / "target/debug/lock-probe"], stdout=lock_log, stderr=lock_log)
                children.append(lock)
                deadline = time.monotonic() + 20
                while time.monotonic() < deadline:
                    if lock.poll() is not None:
                        raise RuntimeError("Locker exited before confirmation")
                    if ".locked()" in (directory / "lock.log").read_text():
                        break
                    time.sleep(0.2)
                else:
                    raise RuntimeError("No compositor lock confirmation")
                # Kill the ordinary UI while locked; the lock client must survive.
                bar.kill()
                bar.wait(timeout=5)
                time.sleep(1)
                if lock.poll() is not None:
                    raise RuntimeError("Locker exited when bar died")
                if interactive:
                    lock.wait(timeout=180)
            adapters = {}
            if renderer == "gpu":
                for client in ("bar", "launcher", "wallpaper", "notifications", "composition", "audio", "bluetooth", "lock"):
                    adapter = selected_adapter((directory / f"{client}.log").read_text())
                    if adapter["backend"] != "Vulkan" or adapter["device_type"] not in ("DiscreteGpu", "IntegratedGpu"):
                        raise RuntimeError(f"{client} did not use hardware Vulkan: {adapter}")
                    adapters[client] = adapter
            (directory / "result.json").write_text(json.dumps({
                "status": "passed", "renderer_request": os.environ.get("ICED_BACKEND", "automatic"),
                "adapters": adapters,
                "checks": ["protocols", "bar-mapped-and-buffer-attached", "PNG-capture-via-grim", "niri-workspace-event-and-action", "launcher-mapped-with-keyboard-focus", "notifications-render-replace-queue-expire-conflict-passive", "wallpaper-solid-and-four-image-modes-pixels", "wallpaper-scaled-pixels",
                "scale-change-process-survival", "composed-bar-wallpaper-pixels-scales", "audio-panel-fixture-focus-render-close-cleanup", "bluetooth-panel-pairing-discovery-close-cleanup", "lock-confirmed", "lock-survives-bar-crash"],
                "not_proven": ["popup-keyboard-focus", "focus-restoration", "multiple-outputs",
                "PAM", "DRM-lock-security", "gamma", "suspend"],
            }, indent=2))
    except Exception as error:
        (directory / "error.txt").write_text(f"{type(error).__name__}: {error}\n")
        raise
    finally:
        for child in reversed(children):
            if child.poll() is None:
                child.terminate()
            try:
                child.wait(timeout=3)
            except subprocess.TimeoutExpired:
                child.kill()
                child.wait()
        (directory / "finished").touch()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--interactive", action="store_true", help="Wait for bar inspection and lock release")
    parser.add_argument("--renderer", choices=("gpu", "software"), default="gpu",
                        help="gpu requires hardware Vulkan; software explicitly selects tiny-skia")
    parser.add_argument("--inside", type=Path, help=argparse.SUPPRESS)
    args = parser.parse_args()
    if args.inside:
        inside(args.inside, args.interactive, args.renderer)
        return
    for binary in ("cabby", "cabby-platform-spike", "cabby-launcher", "lock-probe", "bar-ipc-probe", "cabby-wallpaper", "wallpaper-pixel-probe", "cabby-notifications", "notification-probe", "cabby-desktop", "audio-panel-probe", "bluetooth-panel-probe", "examples/bluez-fixture"):
        if not (ROOT / "target/debug" / binary).is_file():
            parser.error("Run cargo build --workspace --locked and cargo build -p cabby-bluetooth --example bluez-fixture --locked first")
    parent_display = os.environ.get("WAYLAND_DISPLAY")
    if not parent_display:
        parser.error("Run inside an existing Wayland session")
    parent_display = Path(parent_display)
    if not parent_display.is_absolute():
        parent_display = Path(os.environ["XDG_RUNTIME_DIR"]) / parent_display
    artifacts = ROOT / ".artifacts"
    artifacts.mkdir(exist_ok=True)
    directory = Path(tempfile.mkdtemp(prefix="phase0-", dir=artifacts))
    env = os.environ.copy()
    for key, name in (("XDG_RUNTIME_DIR", "runtime"), ("XDG_CONFIG_HOME", "config"),
                      ("XDG_CACHE_HOME", "cache"), ("XDG_STATE_HOME", "state"), ("XDG_DATA_HOME", "data")):
        path = directory / name
        path.mkdir(mode=0o700)
        env[key] = str(path)
    env["WAYLAND_DISPLAY"] = str(parent_display)
    removed = ("NIRI_SOCKET", "NIRI_CONFIG", "DISPLAY", "WAYLAND_DEBUG", "CABBY_TEST_WAYLAND")
    for key in removed:
        env.pop(key, None)
    config = directory / "niri.kdl"
    config.write_text('workspace "test-one"\nworkspace "test-two"\nprefer-no-csd\nhotkey-overlay { skip-at-startup; }\nbinds { Mod+Shift+E { quit; }; }\n')
    subprocess.run(["niri", "validate", "-c", config], check=True, env=env)
    unit = f"cabby-{directory.name}.scope"
    isolated_env = ["env", *[f"--unset={key}" for key in removed],
                    *[f"{key}={env[key]}" for key in ("XDG_RUNTIME_DIR", "XDG_CONFIG_HOME", "XDG_CACHE_HOME",
                                                    "XDG_STATE_HOME", "XDG_DATA_HOME", "WAYLAND_DISPLAY")]]
    command = ["systemd-run", "--user", "--scope", "--quiet", "--collect", f"--unit={unit}",
               "--property=RuntimeMaxSec=360", "--", *isolated_env, "dbus-run-session", "--", "niri", "-c", str(config), "--", sys.executable,
               str(Path(__file__).resolve()), "--inside", str(directory), "--renderer", args.renderer]
    if args.interactive:
        command.append("--interactive")
    print(f"Artifacts: {directory}", flush=True)
    print("Interactive: inspect bar, then touch ARTIFACT_DIRECTORY/request-lock" if args.interactive
          else "Running bounded nested session smoke test", flush=True)
    with (directory / "session.log").open("w") as log:
        session = subprocess.Popen(command, stdout=log, stderr=log, start_new_session=True)
        try:
            deadline = time.monotonic() + (300 if args.interactive else 90)
            while not (directory / "finished").exists():
                if session.poll() is not None:
                    raise RuntimeError("Nested compositor exited; see session.log")
                if time.monotonic() > deadline:
                    raise TimeoutError("Nested session timed out")
                time.sleep(0.2)
            result = directory / "result.json"
            if not result.exists():
                detail = (directory / "error.txt").read_text() if (directory / "error.txt").exists() else "see client logs"
                raise RuntimeError(f"Probe failed: {detail}")
            print(result.read_text())
        finally:
            # A scope owns even descendants that detach into another process group.
            stopped = subprocess.run(["systemctl", "--user", "stop", unit],
                                     capture_output=True, text=True, timeout=15)
            state = subprocess.run(["systemctl", "--user", "show", unit, "--property=ActiveState", "--value"],
                                   capture_output=True, text=True, timeout=5)
            try:
                os.killpg(session.pid, signal.SIGTERM)
                time.sleep(0.5)
                os.killpg(session.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            session.wait(timeout=5)
            (directory / "cleanup.txt").write_text(
                f"Unit: {unit}\nStop result: {stopped.returncode}\nState: {state.stdout.strip()}\n"
                "Wrapper process group terminated and parent reaped.\n")
            if state.returncode != 0 or state.stdout.strip() != "inactive":
                raise RuntimeError(f"Test scope did not stop: {unit}")


if __name__ == "__main__":
    main()
