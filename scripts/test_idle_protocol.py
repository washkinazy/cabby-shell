#!/usr/bin/env python3
"""Private idle-notify server and intercepted power helpers; cannot suspend a host."""
import json, os, queue, shutil, socket, struct, subprocess, tempfile, threading, time
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
U=lambda n:struct.pack('=I',n)
def string(s):
    data=s.encode()+b'\0';return U(len(data))+data+b'\0'*((-len(data))%4)
def wait(test):
    end=time.monotonic()+8
    while not test():
        assert time.monotonic()<end,'idle protocol fixture timed out'
        time.sleep(.02)
class Fixture:
    def __init__(self,path):
        self.listener=socket.socket(socket.AF_UNIX);self.listener.bind(str(path));self.listener.listen();self.listener.settimeout(.03)
        self.stop=False;self.notifications={};self.commands=queue.Queue();self.errors=[]
        self.thread=threading.Thread(target=self.run);self.thread.start()
    def emit(self,timeout,seat,opcode=0):self.commands.put((timeout,seat,opcode))
    def run(self):
        while not self.stop:
            try:client,_=self.listener.accept()
            except socket.timeout:continue
            try:self.handle(client)
            except (BrokenPipeError,ConnectionResetError):pass
            except Exception as e:self.errors.append(repr(e))
            finally:client.close();self.notifications={}
    def handle(self,client):
        client.settimeout(.02);objects={1:('display',0)};buf=b''
        def event(obj,op,data=b''):client.sendall(U(obj)+U(((len(data)+8)<<16)|op)+data)
        while not self.stop:
            while not self.commands.empty():
                timeout,seat,opcode=self.commands.get()
                for obj,info in list(self.notifications.items()):
                    if info==(timeout,seat):event(obj,opcode)
            try:data=client.recv(65536)
            except socket.timeout:continue
            if not data:return
            buf+=data
            while len(buf)>=8:
                obj,header=struct.unpack('=II',buf[:8]);length=header>>16
                if len(buf)<length:break
                args=buf[8:length];buf=buf[length:];op=header&65535;kind,name=objects[obj]
                if kind=='display' and op==0:
                    callback=struct.unpack('=I',args)[0];event(callback,0,U(0));event(1,1,U(callback))
                elif kind=='display' and op==1:
                    registry=struct.unpack('=I',args)[0];objects[registry]=('registry',0)
                    for ident,interface,version in [(1,'ext_idle_notifier_v1',1),(2,'wl_seat',7),(3,'wl_seat',7)]:event(registry,0,U(ident)+string(interface)+U(version))
                elif kind=='registry':
                    ident,n=struct.unpack('=II',args[:8]);offset=8+((n+3)//4*4);version,newid=struct.unpack('=II',args[offset:offset+8])
                    objects[newid]=('notifier' if ident==1 else 'seat',ident)
                    if ident!=1:event(newid,0,U(0));event(newid,1,string('fixture-seat'))
                elif kind=='notifier' and op==1:
                    newid,timeout,seat=struct.unpack('=III',args);objects[newid]=('notification',0);self.notifications[newid]=(timeout,objects[seat][1])
                elif op==0 and kind in ('notification','notifier') or kind=='seat' and op==3:
                    self.notifications.pop(obj,None);objects.pop(obj);event(1,1,U(obj))
                else:raise AssertionError((kind,op))
    def close(self):
        self.stop=True;self.thread.join(timeout=3);assert not self.thread.is_alive();self.listener.close();assert not self.errors,self.errors
def main():
    root=Path(tempfile.mkdtemp(prefix='idle-protocol-',dir=ROOT/'.artifacts'))
    tools=root/'bin';tools.mkdir();config=root/'config.toml';runtime=root/'runtime';runtime.mkdir(mode=0o700)
    helper=tools/'systemctl'
    helper.write_text('''#!/usr/bin/python3
import json,os,sys,time
from pathlib import Path
p=Path(__file__).resolve().parents[1];args=sys.argv[1:]
with (p/'commands').open('a') as f:f.write(json.dumps(['systemctl',*args])+'\\n')
if 'start' in args:
 if (p/'block-lock').exists():
  (p/'pending-pid').write_text(str(os.getpid()))
  time.sleep(60)
 if (p/'deny-lock').exists():sys.exit(1)
 (p/'ready').touch()
elif args[-1]=='suspend':
 assert args==['--no-ask-password','--check-inhibitors=yes','--no-block','suspend']
 assert (p/'ready').exists(),'suspend before lock readiness'
 with (p/'suspends').open('a') as f:f.write('requested\\n')
 sys.exit(int((p/'deny-sleep').exists()))
else:assert 'is-active' in args or 'reset-failed' in args,args
''');helper.chmod(0o700)
    niri=tools/'niri';niri.write_text('''#!/usr/bin/python3
import json,sys
from pathlib import Path
assert sys.argv[1:3]==['msg','action'] and sys.argv[3] in ['power-off-monitors','power-on-monitors']
with (Path(__file__).resolve().parents[1]/'commands').open('a') as f:f.write(json.dumps(['niri',*sys.argv[1:]])+'\\n')
''');niri.chmod(0o700)
    fixture=Fixture(root/'wayland')
    def configure(suspend=30,lock=10,display=20):config.write_text(f'idle_lock_seconds={lock}\nidle_display_off_seconds={display}\nidle_suspend_seconds={suspend}\n[clipboard]\nenabled=false\n')
    configure()
    env=os.environ|{'WAYLAND_DISPLAY':str(root/'wayland'),'XDG_RUNTIME_DIR':str(runtime),'DBUS_SYSTEM_BUS_ADDRESS':'unix:path='+str(root/'no-bus'),'PATH':str(tools)+':'+os.environ['PATH']}
    def cli(cmd):return json.loads(subprocess.check_output([ROOT/'target/debug/cabby',cmd,'--runtime-dir',runtime],env=env,text=True,timeout=5))
    def suspends():return (root/'suspends').read_text().count('requested') if (root/'suspends').exists() else 0
    def commands():return (root/'commands').read_text() if (root/'commands').exists() else ''
    def both(timeout):fixture.emit(timeout,2);fixture.emit(timeout,3)
    def reset():
        before=set(fixture.notifications);fixture.emit(10000,2,1);wait(lambda:len(fixture.notifications)==6 and set(fixture.notifications)!=before)
    with (root/'coordinator.log').open('w') as log:
        process=subprocess.Popen([ROOT/'target/debug/cabby','serve','--config',config,'--runtime-dir',runtime],env=env,stdout=log,stderr=log)
        try:
            wait(lambda:(runtime/'control.sock').exists() and len(fixture.notifications)==6)
            fixture.emit(10000,2);time.sleep(.15);assert not commands(),'one seat triggered lock'
            fixture.emit(10000,3);wait(lambda:(root/'ready').exists())
            both(20000);wait(lambda:'power-off-monitors' in commands())
            fixture.emit(30000,2);time.sleep(.15);assert suspends()==0
            fixture.emit(30000,3);wait(lambda:suspends()==1)
            wait(lambda:cli('idle-status')['idle']['state']=='suspend-requested')
            time.sleep(.2);assert suspends()==1,'repeated suspend in one idle cycle'
            reset();wait(lambda:'power-on-monitors' in commands())
            (root/'block-lock').touch();both(30000);wait(lambda:(root/'pending-pid').exists())
            pid=(root/'pending-pid').read_text();reset()
            wait(lambda:not Path('/proc',pid).exists());assert suspends()==1,'activity did not cancel pending suspend'
            (root/'pending-pid').unlink();both(30000);wait(lambda:(root/'pending-pid').exists())
            pid=(root/'pending-pid').read_text();cli('caffeine-on')
            wait(lambda:not Path('/proc',pid).exists());assert suspends()==1,'pending suspend survived inhibition'
            assert cli('idle-status')['idle']['inhibited']
            (root/'block-lock').unlink();cli('caffeine-off');wait(lambda:len(fixture.notifications)==6)
            time.sleep(.15);assert suspends()==1,'release reused previous idle state'
            (root/'deny-lock').touch();both(30000);wait(lambda:cli('idle-status')['idle']['state']=='suspend-failed');assert suspends()==1
            time.sleep(.2);assert suspends()==1
            (root/'deny-lock').unlink();reset();(root/'deny-sleep').touch();both(30000)
            wait(lambda:suspends()==2 and cli('idle-status')['idle']['state']=='suspend-failed');assert (root/'ready').exists()
            (root/'deny-sleep').unlink();reset();both(30000);wait(lambda:suspends()==3)
            configure(suspend=0);wait(lambda:cli('idle-status')['idle']['suspend_seconds']==0 and len(fixture.notifications)==4)
            configure(suspend=15);time.sleep(.5);assert cli('idle-status')['idle']['suspend_seconds']==0,'invalid config replaced policy'
            configure(suspend=0,lock=0,display=0);wait(lambda:cli('idle-status')['idle']['state']=='disabled')
            assert '"stop"' not in commands() and 'unlock' not in commands()
            (root/'result.json').write_text(json.dumps({'passed':True,'checks':['all-seat-lock-display-suspend-order','lock-readiness-gates-suspend','once-per-cycle','activity-restores-and-rearms','activity-cancels-pending-suspend','caffeine-cancels-pending-helper','helper-reaped','inhibition-release-fresh-cycle','lock-failure-no-suspend','sleep-rejection-keeps-lock','reload-disable-invalid-retention'],'unverified':['physical-suspend-resume','real-logind-authorization']},indent=2))
        finally:
            process.terminate()
            try:process.wait(timeout=5)
            except subprocess.TimeoutExpired:process.kill();process.wait(timeout=5)
            fixture.close()
    print('Artifacts:',root);print((root/'result.json').read_text())
if __name__=='__main__':main()
