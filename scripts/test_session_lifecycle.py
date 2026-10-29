#!/usr/bin/env python3
"""Exercise shipped unit relationships using unique runtime-only test units."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time
import uuid

ROOT = Path(__file__).resolve().parents[1]
BIN = ROOT / 'target/debug/cabby'


def call(*args, check=True):
    return subprocess.run(args, check=check, text=True, capture_output=True, timeout=20)


def ctl(*args, check=True):
    return call('systemctl', '--user', *args, check=check)


def wait_for(test, label):
    deadline = time.monotonic() + 15
    while time.monotonic() < deadline:
        if test():
            return
        time.sleep(0.1)
    raise AssertionError(f'timed out: {label}')


def main():
    prefix = f'cabby-test-{uuid.uuid4().hex[:10]}'
    names = {key: f'{prefix}-{key}.service' for key in ('compositor', 'session', 'ui', 'lock', 'app')}
    names['target'] = f'{prefix}.target'
    runtime_root = Path(os.environ['XDG_RUNTIME_DIR'])
    runtime = runtime_root / prefix
    links = runtime_root / 'systemd/user'
    artifacts = ROOT / '.artifacts'
    artifacts.mkdir(exist_ok=True)
    work = Path(tempfile.mkdtemp(prefix='lifecycle-', dir=artifacts))
    config = work / 'config.toml'
    config.write_text('renderer = "software"\nbar_height = 40\n')
    mappings = {'niri.service': names['compositor'], 'cabby-session.service': names['session'],
                'cabby-ui.service': names['ui'], 'cabby.target': names['target']}
    for source in (ROOT / 'systemd/user').iterdir():
        content = source.read_text()
        for old, new in mappings.items():
            content = content.replace(old, new)
        content = content.replace('/usr/bin/cabby', str(BIN))
        content = content.replace(f'serve --ui-unit {names["ui"]}',
                                  f'serve --ui-unit {names["ui"]} --config {config} --runtime-dir {runtime}')
        content = content.replace('RuntimeDirectory=cabby-shell', f'RuntimeDirectory={prefix}')
        if source.name == 'cabby-ui.service':
            # Synthetic UI launches a descendant in another process group. The
            # production bar/rendering is tested separately in nested niri.
            content = '\n'.join('ExecStart=/bin/sh -c "setsid sleep 360 & wait"' if line.startswith('ExecStart=') else line for line in content.splitlines())+'\n'
        (work / mappings[source.name]).write_text(content)
    (work / names['compositor']).write_text('[Unit]\nDescription=Disposable compositor stand-in\n[Service]\nExecStart=/usr/bin/sleep 360\nKillMode=control-group\n')
    for key in ('lock', 'app'):
        (work / names[key]).write_text(f'[Unit]\nDescription=Lifecycle stand-in only\nBindsTo={names["compositor"]}\nAfter={names["compositor"]}\n[Service]\nExecStart=/usr/bin/sleep 360\nKillMode=control-group\n')
    units = [str(work / unit) for unit in names.values()]
    def pid(key):
        return int(ctl('show', names[key], '-p', 'MainPID', '--value').stdout.strip() or 0)
    def active(key):
        return ctl('is-active', names[key], check=False).stdout.strip() == 'active'
    def request(command, check=True):
        response = call(str(BIN), command, '--runtime-dir', str(runtime), check=check)
        return json.loads(response.stdout)
    results = []
    try:
        verify = call('systemd-analyze', '--user', 'verify', *units)
        (work / 'verify.txt').write_text(verify.stdout + verify.stderr)
        ctl('link', '--runtime', *units)
        ctl('daemon-reload')
        ctl('start', names['target'])
        wait_for(lambda: active('session') and active('ui'), 'services start')
        initial = pid('session')
        ctl('start', names['target'])
        assert pid('session') == initial
        duplicate = call(str(BIN), 'serve', '--config', str(config), '--runtime-dir', str(runtime), check=False)
        assert duplicate.returncode != 0 and 'another Cabby' in duplicate.stderr
        results.append('duplicate starts preserve one owner')
        assert request('status')['config']['bar_height'] == 40
        before = (runtime / 'active.toml').read_text()
        config.write_text('bar_height = 0\n')
        assert not request('reload', check=False)['ok']
        assert request('status')['config']['bar_height'] == 40
        assert (runtime / 'active.toml').read_text() == before
        config.unlink()
        assert not request('reload', check=False)['ok']
        results.append('invalid/deleted reload preserves snapshot')
        config.write_text('renderer = "software"\nbar_height = 64\n')
        old_ui = pid('ui')
        assert request('reload')['ui_restart_queued']
        wait_for(lambda: active('ui') and pid('ui') != old_ui, 'UI restart after valid reload')
        assert request('status')['config']['bar_height'] == 64
        assert not request('reload')['changed']
        results.append('valid reload publishes and restarts UI; unchanged reload is no-op')
        ctl('reset-failed', names['ui'])
        old_ui = pid('ui')
        ctl('kill', '--kill-whom=main', '--signal=KILL', names['ui'])
        wait_for(lambda: active('ui') and pid('ui') not in (0, old_ui), 'UI crash restart')
        assert pid('session') == initial
        results.append('UI crash restarts independently')
        ctl('reset-failed', names['ui'])
        ctl('kill', '--kill-whom=main', '--signal=KILL', names['session'])
        wait_for(lambda: active('session') and pid('session') not in (0, initial) and active('ui'), 'coordinator restart')
        assert request('status')['config']['bar_height'] == 64
        results.append('coordinator crash rebuilds IPC and restarts UI')
        ctl('start', names['lock'], names['app'])
        group = ctl('show', names['ui'], '-p', 'ControlGroup', '--value').stdout.strip()
        descendants = (Path('/sys/fs/cgroup') / group.lstrip('/') / 'cgroup.procs').read_text().split()
        ctl('stop', names['target'])
        wait_for(lambda: not active('session') and not active('ui'), 'shell stop')
        assert all(not Path('/proc', child).exists() for child in descendants)
        assert active('lock') and active('app')
        assert not runtime.exists()
        results.append('shell stop cleans descendants/runtime; session-owned lock/app survive')
        ctl('reset-failed', names['session'], names['ui'], check=False)
        ctl('start', names['target'])
        wait_for(lambda: active('ui'), 'shell restart')
        ctl('reset-failed', names['ui'])
        for _ in range(4):
            old_ui = pid('ui')
            ctl('kill', '--kill-whom=main', '--signal=KILL', names['ui'])
            wait_for(lambda: (active('ui') and pid('ui') not in (0, old_ui)) or
                     ctl('show', names['ui'], '-p', 'Result', '--value').stdout.strip() == 'start-limit-hit',
                     'bounded UI restart')
            if not active('ui'):
                break
        assert ctl('show', names['ui'], '-p', 'Result', '--value').stdout.strip() == 'start-limit-hit'
        assert active('session')
        results.append('repeated crashes hit restart limit without killing coordinator')
        ctl('reset-failed', names['ui'])
        ctl('start', names['ui'])
        ctl('kill', '--kill-whom=main', '--signal=KILL', names['compositor'])
        wait_for(lambda: all(not active(key) for key in ('target', 'session', 'ui', 'lock', 'app')), 'compositor loss')
        time.sleep(1.5)
        assert not active('session') and not active('ui') and not runtime.exists()
        results.append('compositor crash stops every dependent without restart loop')
        (work / 'result.json').write_text(json.dumps({'passed': results, 'stand_ins': ['compositor', 'ui', 'lock', 'app']}, indent=2))
        print(json.dumps(results, indent=2))
    finally:
        journal = call('journalctl', '--user', '--no-pager', '-n', '150',
                       *[arg for unit in names.values() for arg in ('-u', unit)], check=False)
        (work / 'journal.txt').write_text(journal.stdout + journal.stderr)
        ctl('stop', *names.values(), check=False)
        ctl('reset-failed', *names.values(), check=False)
        for unit in names.values():
            link = links / unit
            if link.is_symlink() and link.resolve() == work / unit:
                link.unlink()
        ctl('daemon-reload')
        assert not runtime.exists(), 'test runtime directory left behind'
        print(f'Artifacts: {work}; runtime-only test units removed')


if __name__ == '__main__':
    main()
