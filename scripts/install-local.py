#!/usr/bin/env python3
"""Install Cabby's local binaries and vendor user units; never activate services."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
BINARIES = ('cabby', 'cabby-auth', 'cabby-lock', 'cabby-desktop', 'cabby-launcher',
            'cabby-notifications', 'cabby-platform-spike', 'cabby-wallpaper')
UNITS = ('cabby.target', 'cabby-session.service', 'cabby-ui.service',
         'cabby-notifications.service', 'cabby-launcher.service', 'cabby-lock.service')
HEADER = b'# Installed by Cabby Shell; use systemctl --user edit for personal overrides.\n'

def digest(path):
    with path.open('rb') as source:
        return hashlib.file_digest(source, 'sha256').hexdigest()

def absolute(value, name):
    path = Path(value)
    if not path.is_absolute():
        raise RuntimeError(f'{name} must be absolute')
    return path

def paths():
    home = absolute(os.environ['HOME'], 'HOME')
    data = absolute(os.environ.get('XDG_DATA_HOME') or home / '.local/share', 'XDG_DATA_HOME')
    config = absolute(os.environ.get('XDG_CONFIG_HOME') or home / '.config', 'XDG_CONFIG_HOME')
    return home / '.local/bin', data / 'systemd/user', config / 'systemd/user', data / 'cabby-shell/install.json'

def run(args, check=True):
    result = subprocess.run([str(a) for a in args], check=False, text=True, capture_output=True, timeout=15)
    if check and result.returncode:
        raise RuntimeError(f'{args[0]} failed: {result.stderr.strip()[:8192]}')
    return result

def atomic_copy(source, target, mode):
    target.parent.mkdir(parents=True, exist_ok=True)
    fd, temporary = tempfile.mkstemp(prefix='.cabby-install-', dir=target.parent)
    os.close(fd)
    try:
        shutil.copyfile(source, temporary)
        os.chmod(temporary, mode)
        os.replace(temporary, target)
    finally:
        Path(temporary).unlink(missing_ok=True)

def atomic_bytes(data, target, mode=0o644):
    target.parent.mkdir(parents=True, exist_ok=True)
    fd, temporary = tempfile.mkstemp(prefix='.cabby-install-', dir=target.parent)
    try:
        with os.fdopen(fd, 'wb') as stream:
            stream.write(data)
        os.chmod(temporary, mode)
        os.replace(temporary, target)
    finally:
        Path(temporary).unlink(missing_ok=True)

def load_manifest(path, destinations):
    if not path.exists():
        return {}
    if path.is_symlink():
        raise RuntimeError(f'Installer manifest is a symlink: {path}')
    manifest = json.loads(path.read_text())
    if manifest.get('version') != 1 or not isinstance(manifest.get('files'), dict):
        raise RuntimeError('Unsupported installer manifest')
    files = manifest['files']
    if not files.keys() <= destinations.keys() or not all(isinstance(h, str) and len(h) == 64 for h in files.values()):
        raise RuntimeError('Invalid installer manifest')
    return files

def preflight_existing(destinations, owned, candidates):
    for name, target in destinations.items():
        if not target.exists() and not target.is_symlink():
            continue
        if target.is_symlink() or not target.is_file():
            raise RuntimeError(f'Preserving non-regular destination: {target}')
        if name in owned:
            if digest(target) != owned[name]:
                raise RuntimeError(f'Installed file was modified; preserving it: {target}')
        elif name in UNITS:
            if target.read_bytes() != candidates[name]:
                raise RuntimeError(f'Preserving unmanaged unit: {target}')
        else:
            # Adopt older releases from the original just install recipe.
            with target.open('rb') as stream:
                if stream.read(4) != b'\x7fELF':
                    raise RuntimeError(f'Preserving unmanaged executable: {target}')

def install(build, destinations, unit_dir, config_dir, manifest, owned):
    sources = {name: build / name for name in BINARIES}
    for path in sources.values():
        if not path.is_file() or not os.access(path, os.X_OK):
            raise RuntimeError(f'Missing executable; run just build first: {path}')
    candidates = {name: HEADER + (ROOT / 'systemd/user' / name).read_bytes().replace(b'/usr/bin/cabby', b'%h/.local/bin/cabby') for name in UNITS}
    preflight_existing(destinations, owned, candidates)
    # Reject invalid configuration before changing any installed file.
    run([sources['cabby'], 'validate'])
    migrate = []
    for name in UNITS:
        shadow = config_dir / name
        if shadow.exists() or shadow.is_symlink():
            if shadow.is_file() and shadow.read_bytes() in (candidates[name], candidates[name][len(HEADER):]):
                migrate.append(shadow)
            else:
                print(f'Preserving personal full-unit override: {shadow}')
    for name, source in sources.items():
        atomic_copy(source, destinations[name], 0o755)
    for name, data in candidates.items():
        atomic_bytes(data, destinations[name])
    records = {name: digest(path) for name, path in destinations.items()}
    atomic_bytes(json.dumps({'version': 1, 'files': records}, indent=2).encode() + b'\n', manifest, 0o600)
    # Only exact shipped legacy copies are removed; masks/drop-ins/custom units survive.
    for shadow in migrate:
        shadow.unlink()
    run(['systemctl', '--user', 'daemon-reload'])
    print(f'Installed binaries and Cabby user units in {unit_dir}. No services started or restarted.')

def uninstall(destinations, manifest, owned):
    if not owned:
        raise RuntimeError('No Cabby local install manifest; refusing unmanaged removal')
    for name in UNITS:
        state = run(['systemctl', '--user', 'show', name, '--property=ActiveState', '--value'], check=False)
        if state.stdout.strip() not in ('inactive', 'failed'):
            raise RuntimeError(f'{name} is active or its state is unavailable; stop Cabby and unlock normally before uninstalling')
    for name, expected in owned.items():
        path = destinations[name]
        if path.is_symlink() or (path.exists() and (not path.is_file() or digest(path) != expected)):
            raise RuntimeError(f'Installed file was modified; preserving it: {path}')
    for name in owned:
        destinations[name].unlink(missing_ok=True)
    manifest.unlink()
    run(['systemctl', '--user', 'daemon-reload'])
    print('Removed Cabby local binaries and vendor units. Personal configuration, overrides and PAM policy retained.')

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action', choices=('install', 'uninstall'))
    parser.add_argument('--build-dir', type=Path, default=ROOT / 'target/release')
    args = parser.parse_args()
    binary_dir, unit_dir, config_dir, manifest = paths()
    destinations = {name: (binary_dir if name in BINARIES else unit_dir) / name for name in (*BINARIES, *UNITS)}
    owned = load_manifest(manifest, destinations)
    if args.action == 'install':
        install(args.build_dir.resolve(), destinations, unit_dir, config_dir, manifest, owned)
    else:
        uninstall(destinations, manifest, owned)

if __name__ == '__main__':
    try:
        main()
    except (RuntimeError, OSError, ValueError, subprocess.SubprocessError) as error:
        raise SystemExit(f'Cabby install: {error}')
