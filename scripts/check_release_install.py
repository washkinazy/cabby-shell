#!/usr/bin/env python3
"""Exercise actual release artifacts in a temporary home; intercept every manager call."""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("installer", ROOT / "scripts/install-local.py")
installer = importlib.util.module_from_spec(spec)
spec.loader.exec_module(installer)


def check(build):
    with tempfile.TemporaryDirectory(prefix="cabby-release-install-") as temporary:
        root = Path(temporary)
        home = root / "home"
        home.mkdir()
        tools = root / "tools"
        tools.mkdir()
        calls = root / "calls.jsonl"
        systemctl = tools / "systemctl"
        systemctl.write_text('''#!/usr/bin/python3
import json, os, sys
args = sys.argv[1:]
assert args[0] == '--user'
assert args[1] in ('daemon-reload', 'show'), args
with open(os.environ['INSTALL_TEST_LOG'], 'a') as log:
    log.write(json.dumps(args) + '\\n')
if args[1] == 'show':
    print('inactive')
''')
        systemctl.chmod(0o755)
        env = os.environ | {
            "HOME": str(home), "PATH": str(tools),
            "XDG_CONFIG_HOME": str(home / "config"),
            "XDG_DATA_HOME": str(home / "data"),
            "XDG_RUNTIME_DIR": str(root / "runtime"),
            "INSTALL_TEST_LOG": str(calls),
        }
        Path(env["XDG_RUNTIME_DIR"]).mkdir(mode=0o700)
        # Cabby's documented default intentionally follows HOME, not XDG_CONFIG_HOME.
        preferences = home / ".config/cabby-shell/config.toml"
        preferences.parent.mkdir(parents=True)
        preferences.write_text("font_size = 13\n")
        original = preferences.read_bytes()

        def invoke(*args, ok=True):
            result = subprocess.run(args, env=env, capture_output=True, text=True, timeout=30)
            if (result.returncode == 0) != ok:
                raise AssertionError(result.stdout + result.stderr)
            return result

        command = ["/usr/bin/python3", str(ROOT / "scripts/install-local.py")]
        for _ in range(2):
            invoke(*command, "install", "--build-dir", str(build))
        manifest = home / "data/cabby-shell/install.json"
        records = json.loads(manifest.read_text())["files"]
        assert len(records) == len(installer.BINARIES) + len(installer.UNITS)
        for name in installer.BINARIES:
            source, target = build / name, home / ".local/bin" / name
            assert target.read_bytes()[:4] == b"\x7fELF", name
            assert installer.digest(source) == installer.digest(target) == records[name], name
            assert target.stat().st_mode & 0o777 == 0o755, name
        for name in installer.UNITS:
            target = home / "data/systemd/user" / name
            assert installer.digest(target) == records[name], name
            if name.endswith(".service"):
                assert b"%h/.local/bin/cabby" in target.read_bytes(), name
        installed = home / ".local/bin/cabby"
        assert "font_size = 13" in invoke(str(installed), "validate").stdout
        invoke(str(installed), "--help")
        assert preferences.read_bytes() == original
        preferences.write_text("font_size = 0\n")
        invoke(*command, "install", "--build-dir", str(build), ok=False)
        assert json.loads(manifest.read_text())["files"] == records
        for name in installer.BINARIES:
            assert installer.digest(home / ".local/bin" / name) == records[name]
        preferences.write_bytes(original)
        invoke(*command, "uninstall")
        assert not manifest.exists()
        assert not any((home / ".local/bin" / name).exists() for name in installer.BINARIES)
        assert not any((home / "data/systemd/user" / name).exists() for name in installer.UNITS)
        assert preferences.read_bytes() == original
        return {
            "status": "passed", "binaries": list(installer.BINARIES),
            "units": list(installer.UNITS),
            "checks": ["real-release-install", "idempotent-upgrade", "installed-CLI",
                       "invalid-config-before-mutation", "uninstall-preserves-preferences"],
            "manager_calls": [json.loads(line) for line in calls.read_text().splitlines()],
            "limits": ["temporary-home-on-existing-Fedora-host", "manager-calls-intercepted",
                       "not-a-clean-OS-or-package-dependency-test"],
        }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--build-dir", type=Path, default=ROOT / "target/release")
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    result = json.dumps(check(args.build_dir.resolve()), indent=2) + "\n"
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(result)
    print(result, end="")


if __name__ == "__main__":
    main()
