#!/usr/bin/env python3
"""Native idle observation in disposable niri; every lock helper is intercepted."""
import argparse, json, os, signal, shutil, subprocess, sys, tempfile, time
from pathlib import Path
ROOT = Path(__file__).resolve().parents[1]
def wait_for(test, seconds=10):
    end=time.monotonic()+seconds
    while not test():
        if time.monotonic()>end: raise TimeoutError('idle condition timed out')
        time.sleep(.05)
def inside(directory, renderer):
    display=Path(os.environ['WAYLAND_DISPLAY'])
    if not display.is_absolute(): display=directory/'runtime'/display
    assert display.parent==directory/'runtime' and display.is_socket()
    config=directory/'idle.toml'; config.write_text('idle_lock_seconds=1\n')
    runtime=directory/'coordinator'
    bindir=directory/'bin'; bindir.mkdir()
    helper=bindir/'systemctl'
    helper.write_text("#!/usr/bin/python3\nimport sys\nfrom pathlib import Path\np=Path(__file__).resolve().parents[1]/'requests'\nwith p.open('a') as f: f.write(repr(sys.argv[1:])+'\\n')\nsys.exit(0)\n")
    helper.chmod(0o700)
    log=(directory/'idle.log').open('w')
    env=os.environ | {'PATH':str(bindir)+':'+os.environ['PATH'], 'WAYLAND_DEBUG':'1','DBUS_SYSTEM_BUS_ADDRESS':'unix:path='+str(directory/'no-system-bus')}
    executable=directory/'cabby'
    shutil.copy2(ROOT/'target/debug/cabby',executable)
    child=subprocess.Popen([executable,'serve','--config',config,'--runtime-dir',runtime],env=env,stdout=log,stderr=log)
    def cli(command):
        result=subprocess.run([ROOT/'target/debug/cabby',command,'--runtime-dir',runtime],capture_output=True,text=True,timeout=6,check=True)
        return json.loads(result.stdout)
    def requests():
        p=directory/'requests'
        return p.read_text().count("'start', 'cabby-lock.service'") if p.exists() else 0
    try:
        wait_for(lambda:(runtime/'control.sock').exists())
        wait_for(lambda:requests()==1)
        time.sleep(1.3); assert requests()==1, 'repeated idle action'
        cli('idle-inhibit-on')
        wait_for(lambda:cli('idle-status')['idle']['state']=='inhibited')
        time.sleep(1.2); assert requests()==1
        cli('idle-inhibit-off')
        wait_for(lambda:requests()==2)
        # Re-registering starts from creation even when no physical input arrives.
        replacement=directory/'replacement'
        shutil.copy2(ROOT/'target/debug/cabby',replacement)
        os.replace(replacement,executable)
        config.write_text('idle_lock_seconds=2\n')
        wait_for(lambda:cli('idle-status')['idle']['lock_seconds']==2)
        before=requests(); time.sleep(.5); assert requests()==before
        wait_for(lambda:requests()==before+1)
        config.write_text('idle_lock_seconds=-1\n'); time.sleep(.5)
        assert cli('idle-status')['idle']['lock_seconds']==2
        config.write_text('idle_lock_seconds=0\n')
        wait_for(lambda:cli('idle-status')['idle']['state']=='disabled')
        before=requests(); time.sleep(1.2); assert requests()==before
        niri_helper=bindir/'niri'
        niri_helper.write_text("#!/usr/bin/python3\nimport sys\nfrom pathlib import Path\nassert sys.argv[1:3]==['msg','action']\nassert sys.argv[3] in ['power-off-monitors','power-on-monitors']\nwith (Path(__file__).resolve().parents[1]/'power').open('a') as f: f.write(sys.argv[3]+'\\n')\n")
        niri_helper.chmod(0o700)
        config.write_text('idle_lock_seconds=1\nidle_display_off_seconds=2\n')
        wait_for(lambda:(directory/'power').exists())
        assert 'power-off-monitors' in (directory/'power').read_text()
        cli('idle-inhibit-on')
        wait_for(lambda:'power-on-monitors' in (directory/'power').read_text())
        trace=(directory/'idle.log').read_text()
        assert 'get_idle_notification' in trace and 'get_input_idle_notification' not in trace
        (directory/'result.json').write_text(json.dumps({'passed':True,'checks':['native-idle-lock-once','manual-inhibit','release-fresh-countdown','live-timeout-reset','binary-replacement','invalid-retention','disable','inhibitor-aware-protocol','display-off-after-lock','inhibition-restores-displays'],'unverified':['physical-application-inhibitors','multiple-seats','real-lock-authentication','physical-display-power/hotplug/wake']},indent=2))
    except Exception as error:
        (directory/'error.txt').write_text(repr(error)); raise
    finally:
        child.terminate()
        try: child.wait(timeout=5)
        except subprocess.TimeoutExpired: child.kill(); child.wait(timeout=5)
        log.close(); (directory/'finished').touch()

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
    directory = Path(tempfile.mkdtemp(prefix='idle-', dir=ROOT / '.artifacts'))
    env = os.environ.copy()
    keys = ('XDG_RUNTIME_DIR', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'XDG_STATE_HOME', 'XDG_DATA_HOME')
    for key, name in zip(keys, ('runtime', 'config', 'cache', 'state', 'data')):
        path = directory / name
        path.mkdir(mode=0o700)
        env[key] = str(path)
    env['WAYLAND_DISPLAY'] = str(parent)
    removed = ('NIRI_SOCKET', 'NIRI_CONFIG', 'DISPLAY', 'WAYLAND_DEBUG', 'CABBY_TEST_WAYLAND', 'NOTIFY_SOCKET')
    config = directory / 'niri.kdl'
    config.write_text('prefer-no-csd\nhotkey-overlay { skip-at-startup; }\ninput { keyboard { xkb { layout "us,de"; }; }; }\n')
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
