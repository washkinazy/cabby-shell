"""Native clipboard clients and picker in the caller's disposable niri session."""
import json, os, re, socket, subprocess, time
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
def check(directory, config, runtime, renderer, children):
    assert os.environ.get('CABBY_TEST_WAYLAND')==os.environ['WAYLAND_DISPLAY']
    def wait(test):
        end=time.monotonic()+10
        while not test():
            assert time.monotonic()<end,'clipboard fixture timed out'
            time.sleep(.04)
    def rpc(request):
        with socket.socket(socket.AF_UNIX) as s:
            s.settimeout(4);s.connect(str(runtime/'control.sock'));s.sendall(json.dumps(request).encode()+b'\n')
            data=b''
            while not data.endswith(b'\n'):data+=s.recv(65536)
        return json.loads(data)
    def entries(query=''):return rpc({'clipboard':'list','query':query})['clipboard']['entries']
    def cli(command):return subprocess.run([ROOT/'target/debug/cabby',command],capture_output=True,text=True,timeout=6)
    def copy(data,mime='text/plain;charset=utf-8',supported=True):
        child=subprocess.Popen(['wl-copy','--foreground','--type',mime],stdin=subprocess.PIPE,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
        children.append(child);child.stdin.write(data);child.stdin.close()
        if supported:
            assert child.wait(timeout=8)==0,'source was not replaced by manager'
        return child
    def paste(mime):return subprocess.run(['wl-paste','--no-newline','--type',mime],capture_output=True,timeout=6)
    base=f'osd={{enabled=false}}\nrenderer = "{"vulkan" if renderer=="gpu" else "software"}"\nbar_position = "{"top" if renderer=="gpu" else "bottom"}"\nbar_left=["clipboard"]\nbar_middle=[]\nbar_right=[]\n'
    policy='[clipboard]\nenabled=true\nmax_items=3\nmax_item_bytes=65536\nmemory_bytes=65536\n'
    config.write_text(base+policy);assert cli('reload').returncode==0
    wait(lambda:rpc({'clipboard':'list','query':''})['clipboard']['state']=='watching')
    assert rpc({'clipboard':'clear'})['ok']
    first=b'Synthetic clipboard alpha: source exit persistence and keyboard picker preview.'
    second=b'Synthetic clipboard beta: deduplication and selection are isolated from your clipboard.'
    copy(first);wait(lambda:len(entries())==1);assert paste('text/plain').stdout==first
    for mime in ['text/plain;charset=UTF-8','text/plain;charset=utf-8','UTF8_STRING']:assert paste(mime).stdout==first
    first_id=entries()[0]['id'];copy(second);wait(lambda:len(entries())==2)
    assert rpc({'clipboard':'select','id':first_id})['ok'];wait(lambda:paste('text/plain').stdout==first)
    copy(first);time.sleep(.2);assert len(entries())==2 and entries()[0]['id']==first_id
    assert len(entries('ALPHA'))==1
    png=(ROOT/'examples/config/assets/sample.png').read_bytes();copy(png,'image/png');wait(lambda:len(entries())==3)
    assert paste('image/png').stdout==png
    with (directory/'clipboard.log').open('w') as log:
        bar=subprocess.Popen([ROOT/'target/debug/cabby','ui','--config',config],stdout=log,stderr=log);children.append(bar)
        wait(lambda:cli('clipboard').returncode==0)
        wait(lambda:'cabby-clipboard' in subprocess.run(['niri','msg','--json','layers'],capture_output=True,text=True,timeout=5).stdout)
        wait(lambda:re.search(r'wl_keyboard#\d+\.enter\(', (directory/'clipboard.log').read_text()))
        time.sleep(.8);capture=directory/'clipboard-picker.png';subprocess.run(['grim',capture],check=True,timeout=15)
        subprocess.run([ROOT/'target/debug/wallpaper-pixel-probe',capture,'clipboard-'+('top' if renderer=='gpu' else 'bottom')],check=True,timeout=10)
        bar.terminate();bar.wait(timeout=5)
    assert paste('image/png').stdout==png,'desktop stop lost clipboard ownership'
    for log in [directory/'clipboard.log',directory/'idle-coordinator.log']:
        assert first not in log.read_bytes() and second not in log.read_bytes(),'clipboard payload logged'
    for payload,mime in [(b'ignored','application/x-cabby-fixture-unsupported'),(b'x'*65537,'text/plain;charset=utf-8')]:
        source=copy(payload,mime,False);time.sleep(.4);assert len(entries())==3;source.terminate();source.wait(timeout=5)
    assert rpc({'clipboard':'delete','id':first_id})['ok'];assert len(entries())==2
    assert not rpc({'clipboard':'select','id':first_id})['ok']
    remaining=entries()[0]['id'];assert rpc({'clipboard':'select','id':remaining})['ok']
    assert rpc({'clipboard':'clear'})['ok'];wait(lambda:not entries());assert paste('image/png').returncode!=0
    config.write_text(base+policy.replace('max_items=3','max_items=0'));assert cli('reload').returncode!=0
    assert rpc({'clipboard':'list','query':''})['clipboard']['state']=='watching'
    config.write_text(base+'[clipboard]\nenabled=false\n');assert cli('reload').returncode==0
    wait(lambda:rpc({'clipboard':'list','query':''})['clipboard']['state']=='disabled')
    config.write_text(base+policy);assert cli('reload').returncode==0
    wait(lambda:rpc({'clipboard':'list','query':''})['clipboard']['state']=='watching');copy(first)
    wait(lambda:len(entries())==1);assert rpc({'clipboard':'clear'})['ok']
    for file in runtime.iterdir():
        if file.is_file():assert first not in file.read_bytes() and second not in file.read_bytes(),'clipboard payload persisted'
    (directory/'clipboard-result.json').write_text(json.dumps({'passed':True,'checks':['source-exit-and-paste-real-clients','dedup-search-reselect','png-transfer','desktop-stop-preserves-ownership','oversize-unsupported-rejection','delete-clear','invalid-retention-disable-reenable','picker-focus-render','no-payload-files']},indent=2))
