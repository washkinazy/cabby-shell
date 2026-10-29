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
        config.write_text(f'renderer = "{"vulkan" if renderer == "gpu" else "software"}"\nbar_height = 40\n')
        with (directory / "bar.log").open("w") as log:
            bar = subprocess.Popen([ROOT / "target/debug/cabby", "ui", "--config", config], stdout=log, stderr=log)
            children.append(bar)
            deadline = time.monotonic() + 30
            while time.monotonic() < deadline:
                if bar.poll() is not None:
                    raise RuntimeError("Bar exited; see bar.log")
                layers = run("niri", "msg", "--json", "layers")
                rendered = ".attach(wl_buffer" in (directory / "bar.log").read_text()
                if "cabby-phase0" in layers and rendered:
                    break
                time.sleep(0.2)
            else:
                raise RuntimeError("Bar did not map within 30 seconds")
            (directory / "layers.json").write_text(layers)
            time.sleep(1)
            subprocess.run(["grim", str(directory / "capture.png")], check=True, timeout=15)
            if (directory / "capture.png").read_bytes()[:8] != b"\x89PNG\r\n\x1a\n":
                raise RuntimeError("Capture did not produce a PNG")
            for name in outputs:
                run("niri", "msg", "output", name, "scale", "1.5")
            time.sleep(1)
            if bar.poll() is not None:
                raise RuntimeError("Bar exited after scale change")
            scaled = run("niri", "msg", "--json", "outputs")
            (directory / "scaled-outputs.json").write_text(scaled)
            for name in outputs:
                if json.loads(scaled)[name]["logical"]["scale"] != 1.5:
                    raise RuntimeError(f"Output {name} did not adopt the requested scale")
            if interactive:
                (directory / "interactive-ready").touch()
                # User can inspect popup/focus before creating this request file.
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
                for client in ("bar", "lock"):
                    adapter = selected_adapter((directory / f"{client}.log").read_text())
                    if adapter["backend"] != "Vulkan" or adapter["device_type"] not in ("DiscreteGpu", "IntegratedGpu"):
                        raise RuntimeError(f"{client} did not use hardware Vulkan: {adapter}")
                    adapters[client] = adapter
            (directory / "result.json").write_text(json.dumps({
                "status": "passed", "renderer_request": os.environ.get("ICED_BACKEND", "automatic"),
                "adapters": adapters,
                "checks": ["protocols", "bar-mapped-and-buffer-attached", "PNG-capture-via-grim",
                "scale-change-process-survival", "lock-confirmed", "lock-survives-bar-crash"],
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
    parser.add_argument("--interactive", action="store_true", help="Wait for popup inspection and lock release")
    parser.add_argument("--renderer", choices=("gpu", "software"), default="gpu",
                        help="gpu requires hardware Vulkan; software explicitly selects tiny-skia")
    parser.add_argument("--inside", type=Path, help=argparse.SUPPRESS)
    args = parser.parse_args()
    if args.inside:
        inside(args.inside, args.interactive, args.renderer)
        return
    for binary in ("cabby", "cabby-platform-spike", "lock-probe"):
        if not (ROOT / "target/debug" / binary).is_file():
            parser.error("Run cargo build --workspace --locked first")
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
    config.write_text('prefer-no-csd\nhotkey-overlay { skip-at-startup; }\nbinds { Mod+Shift+E { quit; }; }\n')
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
    print("Interactive: inspect popup, then touch ARTIFACT_DIRECTORY/request-lock" if args.interactive
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
