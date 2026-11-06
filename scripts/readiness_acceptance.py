"""Release timing/resource observations inside phase0-session's private compositor."""
import json
import os
from pathlib import Path
import re
import subprocess
import time

ROOT = Path(__file__).resolve().parents[1]


def wait(predicate, children, seconds=25):
    end = time.monotonic() + seconds
    while not predicate():
        if any(child.poll() is not None for child in children):
            raise RuntimeError("Readiness client exited unexpectedly")
        if time.monotonic() >= end:
            raise TimeoutError("Readiness observation timed out")
        time.sleep(.01)


def processes(roots):
    """Include live owned descendants and CPU already charged for reaped children."""
    table = {}
    for entry in Path("/proc").iterdir():
        if not entry.name.isdigit():
            continue
        try:
            fields = (entry / "stat").read_text().rsplit(")", 1)[1].split()
            # Remaining fields start at stat field 3 (state).
            table[int(entry.name)] = (int(fields[1]), int(fields[19]),
                                     sum(int(fields[i]) for i in (11, 12, 13, 14)))
        except (OSError, ValueError):
            continue
    owned = set(roots)
    while True:
        more = {pid for pid, (parent, _, _) in table.items() if parent in owned}
        if more <= owned:
            break
        owned |= more
    result = {}
    for pid in owned:
        if pid not in table:
            continue
        try:
            rollup = Path(f"/proc/{pid}/smaps_rollup").read_text()
            memory = {key: int(re.search(rf"^{key}:\s+(\d+)", rollup, re.M)[1])
                      for key in ("Rss", "Pss")}
            result[pid] = {"start": table[pid][1], "ticks": table[pid][2], **memory}
        except (OSError, TypeError):
            raise RuntimeError("Cannot measure an owned process")
    if not set(roots) <= result.keys():
        raise RuntimeError("Missing readiness process")
    return result


def check(directory, renderer, children, selected_adapter):
    runtime = directory / "runtime"
    display = Path(os.environ["WAYLAND_DISPLAY"])
    if not display.is_absolute():
        display = runtime / display
    assert display.parent == runtime and display.is_socket()
    assert os.environ["CABBY_TEST_WAYLAND"] == os.environ["WAYLAND_DISPLAY"]
    binaries = ROOT / "target/release"
    config = directory / "readiness.toml"
    config.write_text(f'renderer="{"vulkan" if renderer == "gpu" else "software"}"\n')
    env = os.environ | {
        "DBUS_SYSTEM_BUS_ADDRESS": "unix:path=" + str(directory / "no-system-bus"),
        "PULSE_SERVER": "unix:" + str(directory / "no-audio-server"),
    }
    active = []
    handles = []

    def spawn(name, *arguments):
        log = directory / (name + ".log")
        handle = log.open("w")
        handles.append(handle)
        started = time.monotonic()
        child = subprocess.Popen([str(binaries / "cabby"), *arguments],
                                 stdout=handle, stderr=handle, env=env)
        children.append(child)
        active.append(child)
        return child, log, started

    def call(*arguments):
        return subprocess.run(arguments, env=env, check=True, capture_output=True,
                              text=True, timeout=10).stdout

    try:
        coordinator, _, started = spawn("readiness-coordinator", "serve", "--config", str(config))
        snapshot = runtime / "cabby-shell/active.toml"
        wait(lambda: snapshot.exists() and (runtime / "cabby-shell/control.sock").exists(), active)
        coordinator_ms = (time.monotonic() - started) * 1000
        desktop, desktop_log, started = spawn("readiness-desktop", "desktop", "--config", str(snapshot))
        wait(lambda: ".attach(wl_buffer" in desktop_log.read_text(), active)
        desktop_ms = (time.monotonic() - started) * 1000
        notifications, notice_log, started = spawn("readiness-notifications", "notifications", "--config", str(snapshot))
        address = os.environ["DBUS_SESSION_BUS_ADDRESS"]
        wait(lambda: "org.freedesktop.Notifications" in call("busctl", "--address=" + address, "--no-pager", "list"), active)
        notification_owner_ms = (time.monotonic() - started) * 1000
        # Render once so the idle sample includes an initialized notification renderer.
        started = time.monotonic()
        call("busctl", "--address=" + address, "call", "org.freedesktop.Notifications",
             "/org/freedesktop/Notifications", "org.freedesktop.Notifications", "Notify",
             "susssasa{sv}i", "cabby-readiness", "0", "", "Synthetic fixture", "", "0", "0", "500")
        wait(lambda: ".attach(wl_buffer" in notice_log.read_text(), active)
        notification_frame_ms = (time.monotonic() - started) * 1000
        # Finish initialization/expiration before sampling, without running launchers.
        time.sleep(3)
        roots = [coordinator.pid, desktop.pid, notifications.pid]
        samples = []
        for _ in range(3):
            before = processes(roots)
            started = time.monotonic()
            time.sleep(5)
            after = processes(roots)
            elapsed = time.monotonic() - started
            ticks = sum(value["ticks"] for value in after.values()) - sum(value["ticks"] for value in before.values())
            if ticks < 0:
                raise RuntimeError("Process accounting changed during sample")
            assert all(after[pid]["start"] == before[pid]["start"] for pid in roots)
            samples.append({"seconds": round(elapsed, 3), "cpu_percent_one_core": round(ticks / os.sysconf("SC_CLK_TCK") / elapsed * 100, 3),
                            "rss_mib": round(sum(v["Rss"] for v in after.values()) / 1024, 2),
                            "pss_mib": round(sum(v["Pss"] for v in after.values()) / 1024, 2),
                            "process_count": len(after)})
        launcher_ms = []
        adapters = {}
        for trial in range(3):
            launcher, log, started = spawn(f"readiness-launcher-{trial}", "launcher-ui", "--config", str(snapshot))
            wait(lambda: ".attach(wl_buffer" in log.read_text() and re.search(r"wl_keyboard#\d+\.enter\(", log.read_text()), active)
            launcher_ms.append(round((time.monotonic() - started) * 1000, 2))
            if renderer == "gpu":
                adapters[f"launcher-{trial}"] = selected_adapter(log.read_text())
            launcher.terminate()
            launcher.wait(timeout=5)
            active.remove(launcher)
        if renderer == "gpu":
            adapters["desktop"] = selected_adapter(desktop_log.read_text())
            adapters["notifications"] = selected_adapter(notice_log.read_text())
            for adapter in adapters.values():
                assert adapter["backend"] == "Vulkan" and adapter["device_type"] == "DiscreteGpu" and "RTX 4070" in adapter["name"]
        cpu = next(line.split(":", 1)[1].strip() for line in Path("/proc/cpuinfo").read_text().splitlines() if line.startswith("model name"))
        result = {"status": "passed", "renderer": renderer, "adapters": adapters,
                  "cpu": cpu, "kernel": os.uname().release,
                  "release_binaries": True, "idle_samples": samples,
                  "coordinator_ready_ms": round(coordinator_ms, 2),
                  "desktop_first_buffer_ms": round(desktop_ms, 2),
                  "notification_owner_ms": round(notification_owner_ms, 2),
                  "notification_request_to_buffer_ms": round(notification_frame_ms, 2),
                  "launcher_first_buffer_and_focus_ms": launcher_ms,
                  "conditions": ["private nested niri; existing host compositor also running",
                                 "default config; idle lock/display/sleep and automatic caffeine disabled",
                                 "private session bus; system bus/audio unavailable; bounded retry workers included",
                                 "clipboard healthy with empty memory-only history",
                                 "notification renderer initialized then expired; launcher excluded from idle",
                                 "three consecutive five-second idle windows; CPU is percent of one core",
                                 "RSS sums shared pages repeatedly; PSS apportions them; GPU allocations excluded",
                                 "Wayland/renderer diagnostics enabled; 10ms observation polling",
                                 "warm OS/driver caches; startup is not first visible physical scanout"],
                  "not_proven": ["healthy-real-device idle load", "physical scanout latency", "daily-use-trial"]}
        (directory / "readiness-result.json").write_text(json.dumps(result, indent=2))
        (directory / "result.json").write_text(json.dumps(result, indent=2))
    finally:
        # The outer harness owns termination/reaping and cgroup cleanup on every path.
        for handle in handles:
            handle.close()
