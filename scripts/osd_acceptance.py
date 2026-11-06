"""Native OSD surfaces driven only by private audio and LED fixtures."""
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import time

ROOT = Path(__file__).resolve().parents[1]


def passive_surfaces(trace):
    surfaces = list(re.finditer(r'get_layer_surface\(new id zwlr_layer_surface_v1#(\d+), wl_surface#(\d+),[^\n]*"cabby-osd"\)', trace))
    assert surfaces, "no OSD surface was created"
    for event in surfaces:
        layer, surface = event.groups()
        body = trace[event.end():].split("get_layer_surface(", 1)[0]
        assert re.search(rf'zwlr_layer_surface_v1#{layer}\.set_keyboard_interactivity\(0\)', body)
        region = re.search(rf'wl_surface#{surface}\.set_input_region\(wl_region#(\d+)\)', body)
        assert region, "OSD did not set its own empty input region"
        region_id = region[1]
        lifetime = re.search(rf'create_region\(new id wl_region#{region_id}\)(.*?)wl_region#{region_id}\.destroy\(\)', body, re.S)
        assert lifetime and not re.search(rf'wl_region#{region_id}\.add\(', lifetime[1]), "OSD accepts pointer input"
    assert not re.search(r"wl_keyboard#\d+\.enter\(", trace), "passive OSD took keyboard focus"


def check(directory, renderer, children, selected_adapter):
    assert os.environ.get("CABBY_TEST_WAYLAND") == os.environ["WAYLAND_DISPLAY"]
    runtime = directory / "runtime"
    display = Path(os.environ["WAYLAND_DISPLAY"])
    if not display.is_absolute():
        display = runtime / display
    assert display.parent == runtime and display.is_socket()
    tools = directory / "fixtures"
    tools.mkdir()
    helper = tools / "pactl"
    shutil.copy2(ROOT / "scripts/fixtures/audio-helper.py", helper)
    helper.chmod(0o700)
    data = json.loads((ROOT / "scripts/fixtures/audio-state.json").read_text())
    state_file = tools / "audio-state.json"
    state_file.write_text(json.dumps(data))
    leds = directory / "leds"
    for kind in ("capslock", "numlock"):
        path = leds / ("input-test::" + kind)
        path.mkdir(parents=True)
        (path / "brightness").write_text("0\n")
    config = directory / "osd.toml"
    base = ('renderer="' + ("vulkan" if renderer == "gpu" else "software") + '"\n'
            'bar_left=[]\nbar_middle=[]\nbar_right=[]\nwallpaper_color="#19334d"\n')

    def configure(settings=""):
        config.write_text(base + "[osd]\n" + settings)

    configure()
    log = directory / "osd.log"
    env = os.environ | {"CABBY_TEST_AUDIO_HELPER": str(helper), "CABBY_TEST_LEDS": str(leds),
                        "DBUS_SYSTEM_BUS_ADDRESS": "unix:path=" + str(directory / "no-system-bus")}
    with log.open("w") as output:
        desktop = subprocess.Popen([ROOT / "target/debug/osd-probe", "--config", config],
                                   env=env, stdout=output, stderr=output)
        children.append(desktop)

        def wait(test, seconds=15):
            end = time.monotonic() + seconds
            while not test():
                assert desktop.poll() is None, "OSD client exited"
                assert time.monotonic() < end, "OSD acceptance timed out"
                time.sleep(.04)

        def call(*args):
            return subprocess.run(args, check=True, capture_output=True, text=True, timeout=10).stdout

        def layers():
            return [v for v in json.loads(call("niri", "msg", "--json", "layers"))
                    if v.get("namespace") == "cabby-osd"]

        def events():
            return log.read_text().count("osd-probe:")

        def change(kind="sinks", index=0, percent=None, mute=None):
            item = data[kind][index]
            if percent is not None:
                for channel in item["volume"].values():
                    channel["value"] = round(percent * 65536 / 100)
            if mute is not None:
                item["mute"] = mute
            data["version"] += 1
            temporary = tools / "state.tmp"
            temporary.write_text(json.dumps(data))
            temporary.replace(state_file)

        def volume(percent, mute=False, kind="sinks"):
            previous = events()
            change(kind=kind, percent=percent, mute=mute)
            wait(lambda: events() > previous and len(layers()) == 1)

        def capture(name, percent, margin=96):
            destination = directory / (name + ".png")
            logical = next(iter(json.loads(call("niri", "msg", "--json", "outputs")).values()))["logical"]

            def painted():
                subprocess.run(["grim", str(destination)], check=True, timeout=10)
                return subprocess.run([ROOT / "target/debug/wallpaper-pixel-probe", destination,
                                       f'osd-meter-{101 if percent < 0 else percent}-{logical["width"]}-{logical["height"]}-{margin}'],
                                      capture_output=True, timeout=10).returncode == 0
            wait(painted)

        def key(kind, on):
            before = events()
            (leds / ("input-test::" + kind) / "brightness").write_text(str(int(on)) + "\n")
            wait(lambda: events() > before and len(layers()) == 1)

        try:
            wait(lambda: (tools / "audio-subscribed").exists() and ".attach(wl_buffer" in log.read_text())
            time.sleep(.5)
            assert not layers() and events() == 0, "startup displayed an OSD"
            change(index=1, percent=95)
            time.sleep(.4)
            assert not layers() and events() == 0, "non-default device displayed an OSD"
            configure("volume=false\n")
            time.sleep(.4)
            change(percent=60)
            time.sleep(.4)
            assert not layers() and events() == 0, "disabled volume category emitted"
            configure()
            time.sleep(.4)
            volume(75)
            first = layers()[0]["namespace"]
            capture("osd-volume-75", 75)
            creates = log.read_text().count('"cabby-osd")')
            volume(25)
            capture("osd-volume-25", 25)
            assert len(layers()) == 1 and layers()[0]["namespace"] == first
            assert log.read_text().count('"cabby-osd")') == creates, "burst replaced the OSD surface"
            configure("timeout_ms=2500\nbottom_margin=120\n")
            time.sleep(.4)
            capture("osd-repositioned", 25, 120)
            time.sleep(1.3)
            assert layers(), "stale timeout dismissed refreshed OSD"
            wait(lambda: not layers())
            configure()
            time.sleep(.4)
            volume(125)
            capture("osd-overamplified", 100)
            volume(125, mute=True)
            capture("osd-muted", 0)
            wait(lambda: not layers())
            volume(40, mute=False, kind="sources")
            capture("osd-microphone", 40)
            assert "input: true" in log.read_text()
            volume(40, mute=True, kind="sources")
            capture("osd-microphone-muted", 0)
            wait(lambda: not layers())
            key("capslock", True)
            capture("osd-caps-on", -1)
            key("capslock", False)
            capture("osd-caps-off", -1)
            wait(lambda: not layers())
            before = events()
            (leds / "input-test::numlock/brightness").write_text("1\n")
            time.sleep(.4)
            assert events() == before and not layers(), "disabled Num Lock source emitted"
            configure("num_lock=true\n")
            time.sleep(.6)
            assert events() == before, "reconfiguration emitted a baseline"
            key("numlock", False)
            capture("osd-num-off", -1)
            output_name = next(iter(json.loads(call("niri", "msg", "--json", "outputs"))))
            call("niri", "msg", "output", output_name, "scale", "1.5")
            volume(50)
            capture("osd-scaled", 50)
            configure("enabled=false\n")
            wait(lambda: not layers())
            pids = set((tools / "audio-pids").read_text().splitlines())
            wait(lambda: all(not Path("/proc", pid).exists() for pid in pids))
            change(percent=80)
            (leds / "input-test::capslock/brightness").write_text("1\n")
            time.sleep(.4)
            assert not layers(), "disabled OSD reappeared"
            text = log.read_text()
            passive_surfaces(text)
            adapters = {}
            if renderer == "gpu":
                adapter = selected_adapter(text)
                assert adapter["backend"] == "Vulkan" and adapter["device_type"] == "DiscreteGpu" and "RTX 4070" in adapter["name"]
                adapters["osd"] = adapter
            result = {"status": "passed", "adapters": adapters,
                      "checks": ["startup-and-non-default-silent", "volume-progress-proportions",
                                 "single-reused-surface", "refresh-and-stale-timeout", "live-placement",
                                 "overamplification-clamped-meter", "mute-and-microphone", "caps-on-off",
                                 "num-lock-opt-in", "fractional-scale", "passive-input-and-focus",
                                 "disable-removes-and-reaps"],
                      "not_proven": ["physical-keyboard-LED-driver", "multiple-physical-seats"]}
            (directory / "result.json").write_text(json.dumps(result, indent=2))
        finally:
            desktop.terminate()
            desktop.wait(timeout=5)
