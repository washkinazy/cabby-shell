"""Rendered session menu with a copied desktop and a non-powering sibling CLI."""
import json,os,re,shutil,subprocess,time
from pathlib import Path
from virtual_pointer import Pointer
ROOT=Path(__file__).resolve().parents[1]
def check(directory,renderer,children,selected_adapter):
    def call(*args):return subprocess.run(args,check=True,capture_output=True,text=True,timeout=15).stdout
    def wait(test):
        end=time.monotonic()+20
        while not test():
            assert time.monotonic()<end,'session menu acceptance timed out'
            time.sleep(.05)
    logical=next(iter(json.loads(call('niri','msg','--json','outputs')).values()))['logical'];size=logical['width'],logical['height']
    config=directory/'session.toml';config.write_text('osd={enabled=false}\n[clipboard]\nenabled=false\n')
    # Keep keys at the root, outside the clipboard table.
    config.write_text('renderer="'+('vulkan' if renderer=='gpu' else 'software')+'"\nbar_left=[]\nbar_middle=[]\nbar_right=[]\nwallpaper_color="#19334d"\n'+config.read_text())
    tools=directory/'session-tools';tools.mkdir();desktop_bin=tools/'cabby-desktop';shutil.copy2(ROOT/'target/debug/cabby-desktop',desktop_bin)
    helper=tools/'cabby';helper.write_text('''#!/usr/bin/python3
import json,os,sys,time
from pathlib import Path
p=Path(__file__).parent
assert sys.argv[1:] in [[v] for v in ('lock','suspend','logout','reboot','poweroff')]
with (p/'actions').open('a') as f:f.write(json.dumps(sys.argv[1:])+'\\n')
if (p/'block').exists():
 (p/'pid').write_text(str(os.getpid()));time.sleep(60)
sys.exit(int((p/'deny').exists()))
''');helper.chmod(0o700)
    env=os.environ|{'DBUS_SYSTEM_BUS_ADDRESS':'unix:path='+str(directory/'no-system-bus')}
    log=directory/'session-menu.log'
    with (directory/'session-coordinator.log').open('w') as output:
        coordinator=subprocess.Popen([ROOT/'target/debug/cabby','serve','--config',config],env=env,stdout=output,stderr=output);children.append(coordinator)
    with log.open('w') as output:
        desktop=subprocess.Popen([desktop_bin,'--config',config],env=env,stdout=output,stderr=output);children.append(desktop)
        pointer=Pointer()
        def shown():return any(v['namespace']=='cabby-session-menu' for v in json.loads(call('niri','msg','--json','layers')))
        def toggle():call(str(ROOT/'target/debug/cabby'),'session-menu')
        def painted():
            trace=log.read_text();matches=list(re.finditer(r'get_layer_surface\(new id zwlr_layer_surface_v1#\d+, wl_surface#(\d+),[^\n]*"cabby-session-menu"\)',trace))
            return bool(matches and re.search(rf'wl_surface#{matches[-1][1]}\.attach\(wl_buffer',trace[matches[-1].end():]))
        def open_menu():
            pointer.move(20,size[1]-20,size)
            toggle();wait(shown);wait(painted)
        def actions():return [json.loads(v)[0] for v in (tools/'actions').read_text().splitlines()] if (tools/'actions').exists() else []
        width=min(440,size[0]-16);height=min(400,size[1]-56);x=(size[0]-width)//2;y=(size[1]-height)//2
        def capture(name):
            path=directory/(name+'.png');call('grim',str(path));call(str(ROOT/'target/debug/wallpaper-pixel-probe'),str(path),f'session-menu-{size[0]}-{size[1]}-{x}-{y}-{width}-{height}')
        try:
            wait(lambda:(Path(os.environ['XDG_RUNTIME_DIR'])/'cabby-shell/desktop.sock').exists())
            open_menu();capture('session-menu')
            assert re.search(r'wl_keyboard#\d+\.enter\(',log.read_text()),'menu did not receive keyboard focus'
            # Cancel a destructive choice: no helper may have been invoked.
            pointer.click(x+width//2,y+75+3*52,size);time.sleep(.2);assert not actions()
            capture('session-confirmation');pointer.click(x+width//4,y+height-38,size);time.sleep(.2);assert not actions()
            for index,action in enumerate(('lock','suspend','logout','reboot','poweroff')):
                if not shown():open_menu()
                pointer.click(x+width//2,y+75+index*52,size)
                if index>=2:
                    assert actions()==list(('lock','suspend','logout','reboot','poweroff'))[:index]
                    time.sleep(.2)
                    pointer.click(x+3*width//4,y+height-38,size)
                wait(lambda:len(actions())==index+1 and not shown())
                assert actions()[-1]==action
            # Failure remains visible; toggling closes it without another action.
            (tools/'deny').touch();open_menu();pointer.click(x+width//2,y+75+52,size)
            wait(lambda:len(actions())==6);time.sleep(.3);assert shown();capture('session-failure')
            toggle();wait(lambda:not shown());(tools/'deny').unlink()
            # A dismissed pending menu owns and cancels its helper.
            (tools/'block').touch();open_menu();pointer.click(x+width//2,y+75,size)
            wait(lambda:(tools/'pid').exists());pid=(tools/'pid').read_text();toggle();wait(lambda:not shown() and not Path('/proc',pid).exists())
            assert actions()==['lock','suspend','logout','reboot','poweroff','suspend','lock']
            assert desktop.poll() is None
            adapters={}
            if renderer=='gpu':
                adapter=selected_adapter(log.read_text());assert adapter['backend']=='Vulkan' and 'RTX 4070' in adapter['name'];adapters['session']=adapter
            (directory/'result.json').write_text(json.dumps({'status':'passed','adapters':adapters,'checks':['activation-toggle-focus','opaque-centered-menu-confirmation','all-five-intercepted-actions','destructive-cancel-no-action','failed-request-retained','pending-helper-cancel-reap'],'not_proven':['physical-Ctrl-Alt-Delete','actual-host-power-authorization']},indent=2))
        finally:
            pointer.close();desktop.terminate();desktop.wait(timeout=5);coordinator.terminate();coordinator.wait(timeout=5)
