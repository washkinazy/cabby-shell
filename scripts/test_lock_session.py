#!/usr/bin/env python3
"""Authenticated Iced lock host, on a private nested niri only."""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import signal
import socket
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('phase0', ROOT / 'scripts/phase0-session.py')
phase0 = importlib.util.module_from_spec(spec)
spec.loader.exec_module(phase0)

def wait_for(test, seconds=20):
    deadline = time.monotonic() + seconds
    while not test():
        if time.monotonic() > deadline:
            raise TimeoutError('lock probe condition timed out')
        time.sleep(.05)

def inside(directory, renderer):
    directory = Path(directory)
    display = Path(os.environ['WAYLAND_DISPLAY'])
    if not display.is_absolute():
        display = directory / 'runtime' / display
    assert display.parent == directory / 'runtime' and display.is_socket()
    config = directory / 'lock.toml'
    config.write_text(f'renderer = "{"vulkan" if renderer == "gpu" else "software"}"\n')
    env = os.environ | {'CABBY_TEST_WAYLAND': os.environ['WAYLAND_DISPLAY'], 'WAYLAND_DEBUG': '1',
        'CABBY_TEST_AUTH_HELPER': str(ROOT / 'scripts/fixtures/auth-helper.py'), 'CABBY_RENDERER_DIAGNOSTICS': '1'}
    children = []
    logs = []
    notify = socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM)
    notify_path = directory / 'notify'
    notify.bind(str(notify_path))
    notify.settimeout(15)
    env['NOTIFY_SOCKET'] = str(notify_path)
    def launch(name, mode):
        path = directory / f'{name}.log'
        log = path.open('w')
        logs.append(log)
        child = subprocess.Popen([ROOT / 'target/debug/lock-ui-probe', '--config', config], env=env | {'CABBY_TEST_LOCK_MODE': mode}, stdout=log, stderr=log)
        children.append(child)
        return child, path
    def confirmed(child, path):
        assert notify.recv(1024) == b'READY=1'
        wait_for(lambda: '.locked()' in path.read_text() and '.attach(wl_buffer' in path.read_text())
        assert child.poll() is None
    try:
        first, path = launch('hold', 'hold')
        confirmed(first, path)
        contender, denied = launch('contender', 'hold')
        assert contender.wait(timeout=15) == 73
        assert '.unlock_and_destroy()' not in denied.read_text()
        outputs = json.loads(phase0.run('niri', 'msg', '--json', 'outputs'))
        name = next(iter(outputs))
        phase0.run('niri', 'msg', 'output', name, 'scale', '1.5')
        time.sleep(.5)
        assert first.poll() is None
        first.kill()
        first.wait(timeout=5)
        assert '.unlock_and_destroy()' not in path.read_text()
        # niri must accept a replacement after orphaned-locker death.
        recovered, recovery = launch('recovery', 'authenticate')
        confirmed(recovered, recovery)
        assert recovered.wait(timeout=15) == 0
        trace = recovery.read_text()
        assert 'rejection preserved lock' in trace
        assert trace.count('.unlock_and_destroy()') == 1
        assert trace.index('rejection preserved lock') < trace.index('.unlock_and_destroy()')
        # Fatal host paths must not use Iced's normal unlocking shutdown.
        crashed, crash_path = launch('crash', 'crash')
        confirmed(crashed, crash_path)
        assert crashed.wait(timeout=10) == 1
        assert '.unlock_and_destroy()' not in crash_path.read_text()
        terminated, term_path = launch('terminate', 'hold')
        confirmed(terminated, term_path)
        terminated.terminate()
        assert terminated.wait(timeout=5) == -signal.SIGTERM
        assert '.unlock_and_destroy()' not in term_path.read_text()
        final, final_path = launch('final', 'authenticate')
        confirmed(final, final_path)
        assert final.wait(timeout=15) == 0
        adapters = {}
        if renderer == 'gpu':
            for name in ('hold', 'recovery', 'crash', 'terminate', 'final'):
                adapter = phase0.selected_adapter((directory / f'{name}.log').read_text())
                assert adapter['backend'] == 'Vulkan' and adapter['device_type'] in ('DiscreteGpu', 'IntegratedGpu')
                adapters[name] = adapter
        (directory / 'result.json').write_text(json.dumps({'passed': True, 'renderer': renderer, 'adapters': adapters,
            'checks': ['locked-before-READY', 'buffer-presented', 'competing-lock-denied', 'scale-change', 'kill-without-unlock', 'niri-orphan-replacement', 'denied-auth-stays-locked', 'success-unlocks-once', 'fatal-without-unlock', 'SIGTERM-without-unlock'],
            'unverified': ['real-user-PAM', 'physical-input/multiple-outputs', 'DRM-lock-security', 'suspend']}, indent=2))
    except Exception as error:
        (directory / 'error.txt').write_text(repr(error))
        raise
    finally:
        for child in children:
            if child.poll() is None:
                child.kill()
            child.wait(timeout=5)
        for log in logs:
            log.close()
        notify.close()
        (directory / 'finished').touch()

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--renderer', choices=('gpu', 'software'), default='gpu')
    parser.add_argument('--inside', type=Path)
    args = parser.parse_args()
    if args.inside:
        return inside(args.inside, args.renderer)
    parent = Path(os.environ['WAYLAND_DISPLAY'])
    if not parent.is_absolute():
        parent = Path(os.environ['XDG_RUNTIME_DIR']) / parent
    directory = Path(tempfile.mkdtemp(prefix='lock-', dir=ROOT / '.artifacts'))
    env = os.environ.copy()
    keys = ('XDG_RUNTIME_DIR', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'XDG_STATE_HOME', 'XDG_DATA_HOME')
    for key, name in zip(keys, ('runtime', 'config', 'cache', 'state', 'data')):
        path = directory / name
        path.mkdir(mode=0o700)
        env[key] = str(path)
    env['WAYLAND_DISPLAY'] = str(parent)
    removed = ('NIRI_SOCKET', 'NIRI_CONFIG', 'DISPLAY', 'WAYLAND_DEBUG', 'CABBY_TEST_WAYLAND', 'NOTIFY_SOCKET')
    config = directory / 'niri.kdl'
    config.write_text('prefer-no-csd\nhotkey-overlay { skip-at-startup; }\n')
    unit = f'cabby-{directory.name}.scope'
    command = ['systemd-run', '--user', '--scope', '--quiet', '--collect', f'--unit={unit}', '--property=RuntimeMaxSec=120', '--', 'env', *[f'--unset={key}' for key in removed], *[f'{key}={env[key]}' for key in (*keys, 'WAYLAND_DISPLAY')], 'dbus-run-session', '--', 'niri', '-c', str(config), '--', sys.executable, str(Path(__file__).resolve()), '--inside', str(directory), '--renderer', args.renderer]
    print(f'Artifacts: {directory}', flush=True)
    with (directory / 'session.log').open('w') as log:
        session = subprocess.Popen(command, stdout=log, stderr=log, start_new_session=True)
        try:
            wait_for(lambda: (directory / 'finished').exists() or session.poll() is not None, 100)
            if not (directory / 'result.json').exists():
                raise RuntimeError((directory / 'error.txt').read_text() if (directory / 'error.txt').exists() else 'nested session failed')
            print((directory / 'result.json').read_text())
        finally:
            subprocess.run(['systemctl', '--user', 'stop', unit], capture_output=True, timeout=15)
            try:
                os.killpg(session.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
            session.wait(timeout=5)
            state = subprocess.run(['systemctl', '--user', 'show', unit, '--property=ActiveState', '--value'], capture_output=True, text=True, timeout=5)
            (directory / 'cleanup.txt').write_text(state.stdout)
            assert state.stdout.strip() in ('inactive', 'failed')

if __name__ == '__main__':
    main()
