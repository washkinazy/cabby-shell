#!/usr/bin/env python3
"""Shipped locker unit with a READY-aware stand-in, never a real lock."""
import importlib.util
import json
import os
import subprocess
from pathlib import Path
import tempfile
import time
import uuid

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('lifecycle', ROOT / 'scripts/test_session_lifecycle.py')
lifecycle = importlib.util.module_from_spec(spec)
spec.loader.exec_module(lifecycle)
call, ctl, wait_for = lifecycle.call, lifecycle.ctl, lifecycle.wait_for

def main():
    prefix = 'cabby-lock-test-' + uuid.uuid4().hex[:10]
    work = Path(tempfile.mkdtemp(prefix='lock-lifecycle-', dir=ROOT / '.artifacts'))
    links = Path(os.environ['XDG_RUNTIME_DIR']) / 'systemd/user'
    names = {key: f'{prefix}-{key}.service' for key in ('session', 'lock', 'ui')}
    target = f'{prefix}.target'
    fixture = work / 'fixture.py'
    fixture.write_text('''import os,socket,time
from pathlib import Path
root=Path(__file__).parent
(root/'pid').write_text(str(os.getpid()))
if not (root/'no-ready').exists():
    time.sleep(.6)
    sock=socket.socket(socket.AF_UNIX,socket.SOCK_DGRAM)
    addr=os.environ['NOTIFY_SOCKET']
    if addr.startswith('@'): addr='\\0'+addr[1:]
    (root/'ready').touch()
    sock.sendto(b'READY=1',addr)
while not (root/'exit').exists(): time.sleep(.05)
''')
    content = (ROOT / 'systemd/user/cabby-lock.service').read_text().replace('niri.service', names['session']).replace('/usr/bin/cabby lock-ui', f'/usr/bin/python3 {fixture}')
    # Short fixture deadlines preserve the shipped lifecycle semantics.
    content = content.replace('TimeoutStartSec=15', 'TimeoutStartSec=2').replace('TimeoutStopSec=5', 'TimeoutStopSec=1')
    (work / names['lock']).write_text(content)
    (work / names['session']).write_text('[Service]\nExecStart=/usr/bin/sleep 360\n')
    (work / names['ui']).write_text(f'[Unit]\nPartOf={target}\nBindsTo={names["session"]}\nAfter={names["session"]}\n[Service]\nExecStart=/usr/bin/sleep 360\n')
    (work / target).write_text(f'[Unit]\nWants={names["ui"]}\n')
    files = [work / name for name in (*names.values(), target)]
    def active(key):
        return ctl('is-active', names[key], check=False).stdout.strip() == 'active'
    def pid():
        return int(ctl('show', names['lock'], '-p', 'MainPID', '--value').stdout.strip() or 0)
    # Only power requests are intercepted. All user-manager calls reach real
    # isolated fixture units; no route from this wrapper can suspend the host.
    wrapper_dir = work / 'bin'
    wrapper_dir.mkdir()
    wrapper = wrapper_dir / 'systemctl'
    wrapper.write_text('''#!/usr/bin/python3
import os,sys
from pathlib import Path
root=Path(__file__).resolve().parents[1]
args=sys.argv[1:]
if '--user' in args:
    os.execv('/usr/bin/systemctl', ['systemctl', *args])
assert args == ['--no-ask-password', '--check-inhibitors=yes', '--no-block', 'suspend'], args
assert (root/'ready').exists(), 'suspend before READY'
(root/'suspend').touch()
sys.exit(1 if (root/'deny-suspend').exists() else 0)
''')
    wrapper.chmod(0o700)
    def request(command='lock'):
        env = dict(os.environ, PATH=str(wrapper_dir) + ':' + os.environ['PATH'])
        return subprocess.run([str(ROOT / 'target/debug/cabby'), command, '--lock-unit', names['lock'], '--session-unit', names['session']], check=False, text=True, capture_output=True, timeout=45, env=env)
    try:
        verify = call('systemd-analyze', '--user', 'verify', *map(str, files))
        (work / 'verify.txt').write_text(verify.stdout + verify.stderr)
        ctl('link', '--runtime', *map(str, files))
        ctl('daemon-reload')
        assert request('suspend').returncode != 0  # Do not start an absent compositor.
        assert not (work/'suspend').exists()
        ctl('start', names['session'], target)
        started = time.monotonic()
        assert request('suspend').returncode == 0 and active('lock')
        assert (work/'suspend').exists()
        assert time.monotonic() - started >= .5  # start waits for READY.
        original = pid()
        (work/'deny-suspend').touch()
        assert request('suspend').returncode != 0 and active('lock') and pid() == original
        (work/'deny-suspend').unlink()
        (work/'suspend').unlink()
        assert request().returncode == 0 and pid() == original
        for _ in range(4):
            (work / 'exit').touch()
            wait_for(lambda: not active('lock'), 'successful unlock exits')
            (work / 'exit').unlink()
            assert request().returncode == 0 and active('lock')
        original = pid()
        ctl('stop', target)
        assert active('lock') and pid() == original and not active('ui')
        assert ctl('stop', names['lock'], check=False).returncode != 0
        assert active('lock')
        ctl('kill', '--signal=KILL', names['lock'])
        wait_for(lambda: active('lock') and pid() != original, 'lock restart')
        original = pid()
        ctl('kill', '--signal=TERM', names['lock'])
        wait_for(lambda: active('lock') and pid() != original, 'termination signal restart')
        ctl('stop', names['session'])
        wait_for(lambda: not active('lock'), 'session loss stops lock')
        (work / 'no-ready').touch()
        ctl('reset-failed', names['lock'], check=False)
        ctl('start', names['session'])
        (work/'ready').unlink(missing_ok=True)
        assert request('suspend').returncode != 0
        assert not (work/'suspend').exists()
        wait_for(lambda: ctl('show', names['lock'], '-p', 'SubState', '--value').stdout.strip() == 'failed' and int(ctl('show', names['lock'], '-p', 'NRestarts', '--value').stdout.strip()) >= 3, 'bounded readiness failure')
        time.sleep(1.5)
        assert pid() == 0
        assert not active('lock')
        (work / 'result.json').write_text(json.dumps({'passed': True, 'checks': ['absent-session-rejected', 'READY-gates-start', 'duplicate-single-owner', 'rapid-successful-lock-cycles', 'shell-stop-independence', 'manual-stop-refused', 'crash-restart', 'termination-signal-restart', 'session-loss-cleanup', 'missing-READY-fails-bounded', 'suspend-after-READY', 'rejected-suspend-keeps-lock', 'missing-READY-prevents-suspend']}, indent=2))
        print(work)
        print((work / 'result.json').read_text())
    finally:
        ctl('stop', target, names['session'], check=False)
        ctl('reset-failed', *names.values(), target, check=False)
        for path in files:
            (links / path.name).unlink(missing_ok=True)
        ctl('daemon-reload')
        (work / 'cleanup.txt').write_text('Temporary units unlinked; compositor dependency stopped.\n')

if __name__ == '__main__':
    main()
