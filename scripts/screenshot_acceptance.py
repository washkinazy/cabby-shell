"""Screenshot GUI exercised with native input only inside a private compositor."""
import array,fcntl,json,math,os,socket,struct,subprocess,time
from pathlib import Path
from virtual_pointer import Pointer
ROOT=Path(__file__).resolve().parents[1]
def check(directory,config,runtime,renderer,children):
    assert os.environ.get('CABBY_TEST_WAYLAND')==os.environ['WAYLAND_DISPLAY']
    saves=directory/'saved-images';saves.mkdir()
    base=f'osd={{enabled=false}}\nrenderer="{"vulkan" if renderer=="gpu" else "software"}"\nbar_left=["screenshot"]\nbar_middle=[]\nbar_right=[]\nwallpaper_color="#19334d"\n[screenshot]\ndirectory="{saves}"\n'
    config.write_text(base)
    def cli(action):return subprocess.run([ROOT/'target/debug/cabby',action],capture_output=True,text=True,timeout=7)
    assert cli('reload').returncode==0
    def wait(test):
        end=time.monotonic()+12
        while not test():
            assert time.monotonic()<end,'screenshot workflow timed out'
            time.sleep(.05)
    def state():
        try:return json.loads((runtime/'screenshot.json').read_text())
        except (FileNotFoundError,json.JSONDecodeError):return {}
    def phase(value):return state().get('state')==value
    def layers():return json.loads(subprocess.run(['niri','msg','--json','layers'],capture_output=True,text=True,check=True,timeout=5).stdout)
    with (directory/'screenshot.log').open('w') as log:
        desktop=subprocess.Popen([ROOT/'target/debug/cabby','desktop','--config',config],stdout=log,stderr=log);children.append(desktop)
        wait(lambda:any(l.get('namespace')=='cabby-wallpaper' for l in layers()))
        baseline=directory/'screenshot-baseline.png'
        def background_ready():
            subprocess.run(['grim',baseline],check=True,timeout=10)
            return subprocess.run([ROOT/'target/debug/wallpaper-pixel-probe',baseline,'capture-background'],capture_output=True,timeout=8).returncode==0
        wait(background_ready)
        pointer=Pointer()
        def selector_ready(reposition=True):
            logical=tuple(state()['logical_size'])
            if reposition:pointer.move(5,5,logical)
            def painted():
                candidate=directory/'screenshot-selector.png';subprocess.run(['grim',candidate],check=True,timeout=10)
                return subprocess.run([ROOT/'target/debug/wallpaper-pixel-probe',candidate,f'shot-select-ready-{logical[0]}-{logical[1]}'],capture_output=True,timeout=8).returncode==0
            wait(painted)
        try:
            initial=next(iter(json.loads((directory/'outputs.json').read_text()).values()))['logical']
            pointer.move(100,100,(initial['width'],initial['height']))
            wait(lambda:cli('screenshot').returncode==0);wait(lambda:phase('selecting'));selector_ready(False)
            logical=tuple(state()['logical_size']);source=state()['source_size']
            pointer.button(True);pointer.move(400,300,logical);pointer.button(False);wait(lambda:phase('preview') and not state()['busy'])
            expected=[math.ceil(400*source[0]/logical[0])-math.floor(100*source[0]/logical[0]),math.ceil(300*source[1]/logical[1])-math.floor(100*source[1]/logical[1])]
            assert all(abs(a-b)<=2 for a,b in zip(state()['selected_size'],expected)),(state()['selected_size'],expected)
            time.sleep(.4);capture=directory/'screenshot-preview.png';subprocess.run(['grim',capture],check=True,timeout=15)
            def preview_button(index):
                # Model status can precede configure/paint on software. Wait for
                # enabled controls in the actual compositor frame before input.
                pointer.move(5,5,logical)
                def ready():
                    candidate=directory/'screenshot-controls.png';subprocess.run(['grim',candidate],check=True,timeout=10)
                    return subprocess.run([ROOT/'target/debug/wallpaper-pixel-probe',candidate,f'shot-ready-{logical[0]}-{logical[1]}'],capture_output=True,timeout=8).returncode==0
                wait(ready)
                width=min(440,logical[0]-16);height=min(500,logical[1]-56);x=(logical[0]-width)//2;y=(logical[1]-height)//2
                pointer.click(x+width*(2*index+1)//6,y+height-32,logical)
            preview_button(0);wait(lambda:state().get('action')=='copied' and not state()['busy'])
            copied=subprocess.run(['wl-paste','--no-newline','--type','image/png'],capture_output=True,check=True,timeout=6).stdout
            preview_button(1);wait(lambda:state().get('action')=='saved' and not state()['busy'])
            files=list(saves.glob('*.png'));assert len(files)==1 and files[0].read_bytes()==copied
            subprocess.run([ROOT/'target/debug/wallpaper-pixel-probe',files[0],'capture-solid'],check=True,timeout=8)
            # Reject unsealed / malformed descriptor imports without changing clipboard.
            def rejected(fd):
                with socket.socket(socket.AF_UNIX) as client:
                    client.settimeout(4);client.connect(str(runtime/'control.sock'))
                    client.sendmsg([b'{"clipboard":"import-png"}\n'],[(socket.SOL_SOCKET,socket.SCM_RIGHTS,array.array('i',[fd]))])
                    reply=b''
                    while not reply.endswith(b'\n'):reply+=client.recv(4096)
                    assert not json.loads(reply)['ok']
            with files[0].open('rb') as image:rejected(image.fileno())
            fd=os.memfd_create('invalid-fixture',os.MFD_ALLOW_SEALING|os.MFD_CLOEXEC)
            try:
                os.write(fd,b'invalid PNG');fcntl.fcntl(fd,fcntl.F_ADD_SEALS,fcntl.F_SEAL_WRITE|fcntl.F_SEAL_SHRINK|fcntl.F_SEAL_GROW);rejected(fd)
            finally:os.close(fd)
            assert subprocess.run(['wl-paste','--no-newline','--type','image/png'],capture_output=True,check=True,timeout=6).stdout==copied

            preview_button(1);wait(lambda:state().get('action')=='error');assert files[0].read_bytes()==copied
            preview_button(2);wait(lambda:phase('idle'));wait(lambda:not any(l.get('namespace')=='cabby-screenshot' for l in layers()))
            before=len(list(saves.iterdir()))
            assert cli('screenshot').returncode==0;wait(lambda:phase('selecting'));selector_ready();logical=tuple(state()['logical_size'])
            pointer.click(logical[0]-61,logical[1]-32,logical);wait(lambda:phase('idle'));assert len(list(saves.iterdir()))==before
            # Fractional scale / transformed output: crop against actual oriented PNG sizes.
            outputs=json.loads(subprocess.run(['niri','msg','--json','outputs'],capture_output=True,text=True,check=True).stdout)
            output=next(iter(outputs));subprocess.run(['niri','msg','output',output,'scale','1.5'],check=True,capture_output=True,timeout=8);time.sleep(.5)
            assert cli('screenshot-monitor').returncode==0;wait(lambda:phase('preview') and not state()['busy']);logical=tuple(state()['logical_size']);assert state()['selected_size']==state()['source_size']
            preview_button(0);wait(lambda:state().get('action')=='copied' and not state()['busy']);png=subprocess.run(['wl-paste','--no-newline','--type','image/png'],capture_output=True,check=True,timeout=6).stdout
            assert list(struct.unpack('>II',png[16:24]))==state()['source_size']
            preview_button(2);wait(lambda:phase('idle'))
            # Rotate the private output and use the actual oriented PNG dimensions.
            subprocess.run(['niri','msg','output',output,'transform','90'],check=True,capture_output=True,timeout=8);time.sleep(.5)
            assert cli('screenshot').returncode==0;wait(lambda:phase('selecting'));selector_ready();logical=tuple(state()['logical_size']);source=state()['source_size']
            pointer.drag((100,100),(400,300),logical);wait(lambda:phase('preview') and not state()['busy'])
            expected=[math.ceil(400*source[0]/logical[0])-math.floor(100*source[0]/logical[0]),math.ceil(300*source[1]/logical[1])-math.floor(100*source[1]/logical[1])]
            assert all(abs(a-b)<=2 for a,b in zip(state()['selected_size'],expected))
            preview_button(1);wait(lambda:state().get('action')=='saved' and not state()['busy']);files=list(saves.glob('*.png'));assert len(files)==before+1
            latest=max(files,key=lambda p:p.stat().st_mtime_ns);subprocess.run([ROOT/'target/debug/wallpaper-pixel-probe',latest,'capture-solid'],check=True,timeout=8)
            preview_button(2);wait(lambda:phase('idle'));before+=1
            # A failed destination must leave both preview and pre-existing data intact.
            blocked=directory/'blocked-destination';blocked.write_bytes(b'fixture')
            config.write_text(base.replace(str(saves),str(blocked)));assert cli('reload').returncode==0
            assert cli('screenshot-monitor').returncode==0;wait(lambda:phase('preview') and not state()['busy']);logical=tuple(state()['logical_size'])
            preview_button(1);wait(lambda:state().get('action')=='error');assert phase('preview') and blocked.read_bytes()==b'fixture'
            assert cli('screenshot-cancel').returncode==0;wait(lambda:phase('idle'))
            config.write_text(base);assert cli('reload').returncode==0
            # A changed output cancels the frozen selection instead of using stale geometry.
            assert cli('screenshot').returncode==0;wait(lambda:phase('selecting'))
            subprocess.run(['niri','msg','output',output,'scale','1'],check=True,capture_output=True,timeout=8);wait(lambda:phase('idle'))
            assert len(list(saves.iterdir()))==before
            for item in runtime.iterdir():
                if item.is_file():assert not item.read_bytes().startswith(b'\x89PNG'),'capture left a runtime file'
            (directory/'screenshot-result.json').write_text(json.dumps({'passed':True,'checks':['native-pointer-region-selection','preview-copy-save-exact-png','overlay-excluded-solid-pixels','no-overwrite','destination-failure-retains-preview','explicit-cancel-command','cancel-no-file','fractional-monitor-capture','rotated-fractional-region','sealed-descriptor-boundary','output-change-cancels','no-runtime-capture-files']},indent=2))
        finally:pointer.close();desktop.terminate();desktop.wait(timeout=5)
