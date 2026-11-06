#!/usr/bin/env python3
"""Private Wayland gamma fixture: synthetic outputs and FDs, never a real display."""
import array, json, os, shutil, socket, struct, subprocess, tempfile, threading, time
from pathlib import Path
ROOT = Path(__file__).resolve().parents[1]
def integer(n): return struct.pack('=I',n)
def string(s):
    b=s.encode()+b'\0';return integer(len(b))+b+b'\0'*((-len(b))%4)
def event(client,obj,opcode,payload=b''): client.sendall(integer(obj)+integer(((len(payload)+8)<<16)|opcode)+payload)
class Fixture:
    def __init__(self,path):
        self.listener=socket.socket(socket.AF_UNIX,socket.SOCK_STREAM);self.listener.bind(str(path));self.listener.listen();self.listener.settimeout(.1)
        self.stop=False;self.handlers=[];self.full_clients=[];self.fullscreen=False;self.trace=[];self.errors=[];self.uploads=[];self.restores=0;self.current=None;self.size=256;self.thread=threading.Thread(target=self.run);self.thread.start()
    def run(self):
        while not self.stop:
            try:client,_=self.listener.accept()
            except socket.timeout:continue
            thread=threading.Thread(target=self.handle,args=(client,));self.handlers.append(thread);thread.start()
    def handle(self,client):
        try:self.handle_connection(client)
        except Exception as error:self.errors.append(repr(error))
    def handle_connection(self,client):
        client.settimeout(.1);objects={1:'display'};buf=b'';fds=[];registry=None;control=None
        entry=None
        try:
            while not self.stop:
                try: data,anc,_,_=client.recvmsg(65536,socket.CMSG_SPACE(64))
                except socket.timeout: continue
                if not data: break
                for level,kind,body in anc:
                    if level==socket.SOL_SOCKET and kind==socket.SCM_RIGHTS:
                        descriptors=array.array('i');descriptors.frombytes(body[:len(body)//4*4]);fds.extend(descriptors)
                buf+=data
                while len(buf)>=8:
                    obj,header=struct.unpack('=II',buf[:8]);size=header>>16;op=header&65535
                    assert 8<=size<=65536
                    if len(buf)<size: break
                    args=buf[8:size];buf=buf[size:];kind=objects[obj];self.trace.append((obj,kind,op))
                    if kind=='display' and op==1:
                        registry=struct.unpack('=I',args)[0];objects[registry]='registry'
                        event(client,registry,0,integer(11)+string('zwlr_gamma_control_manager_v1')+integer(1))
                        event(client,registry,0,integer(12)+string('wl_output')+integer(4))
                        event(client,registry,0,integer(14)+string('zwlr_foreign_toplevel_manager_v1')+integer(3))
                    elif kind=='display' and op==0:
                        callback=struct.unpack('=I',args)[0];event(client,callback,0,integer(0));event(client,1,1,integer(callback))
                    elif kind=='registry' and op==0:
                        _,length=struct.unpack('=II',args[:8]);interface=args[8:8+length-1].decode();position=8+((length+3)//4*4);version,newid=struct.unpack('=II',args[position:position+8]);objects[newid]={'zwlr_gamma_control_manager_v1':'manager','zwlr_foreign_toplevel_manager_v1':'fullmanager','wl_output':'output'}[interface]
                        if objects[newid]=='output' and version>=4:event(client,newid,4,string('TEST-1'))
                        if objects[newid]=='fullmanager':
                            top=0xff000000;objects[top]='top';event(client,newid,0,integer(top));output=next((i for i,k in objects.items() if k=='output'),None);entry={'client':client,'top':top,'output':output};self.full_clients.append(entry)
                            if output:event(client,top,2,integer(output))
                            event(client,top,4,integer(8)+integer(3)+integer(2) if self.fullscreen else integer(0));event(client,top,5)
                        elif objects[newid]=='output' and entry:
                            entry['output']=newid;event(client,entry['top'],2,integer(newid));event(client,entry['top'],5)
                    elif kind=='manager' and op==0:
                        self.current=(client,lambda:registry,lambda:control)
                        control,output=struct.unpack('=II',args);assert objects[output]=='output';objects[control]='gamma';event(client,control,0,integer(self.size))
                    elif kind=='gamma' and op==0:
                        assert fds;fd=fds.pop(0)
                        try:
                            # Match niri's sequential FD read. pread(..., 0) hid
                            # uploads accidentally sent with their cursor at EOF.
                            assert os.lseek(fd,0,os.SEEK_CUR)==0, 'gamma FD must start at byte zero'
                            raw=os.read(fd,500000);assert len(raw)==self.size*6
                            values=struct.unpack('='+str(self.size*3)+'H',raw)
                            assert values[self.size-1]>values[self.size*2-1]>values[self.size*3-1]
                            self.uploads.append(values)
                        finally: os.close(fd)
                    elif kind=='top':
                        assert op==7;objects.pop(obj,None)
                    elif kind in ('gamma','manager','output'):
                        assert op==(1 if kind!='output' else 0)
                        if kind=='gamma': self.restores+=1;control=None
                        del objects[obj];event(client,1,1,integer(obj))
                    else: raise AssertionError((kind,op))
        except (BrokenPipeError,ConnectionResetError): pass
        finally:
            for fd in fds:os.close(fd)
            client.close()
            if self.current and self.current[0] is client:self.current=None
            if entry in self.full_clients:self.full_clients.remove(entry)
    def fail(self):
        client,_,control=self.current;self.trace.append(("fail",control()));event(client,control(),1)
    def hotplug(self):
        client,registry,_=self.current;event(client,registry(),1,integer(12));event(client,registry(),0,integer(13)+string('wl_output')+integer(4))
    def focus(self, focused):
        for entry in list(self.full_clients):
            flags=[3,2] if focused else [3]
            event(entry['client'],entry['top'],4,integer(4*len(flags))+b''.join(integer(n) for n in flags));event(entry['client'],entry['top'],5)
    def visibility(self, visible):
        for entry in list(self.full_clients):
            event(entry['client'],entry['top'],2 if visible else 3,integer(entry['output']));event(entry['client'],entry['top'],5)
    def minimize(self, value):
        for entry in list(self.full_clients):
            flags=[3,2,1] if value else [3,2]
            event(entry['client'],entry['top'],4,integer(4*len(flags))+b''.join(integer(n) for n in flags));event(entry['client'],entry['top'],5)
    def set_fullscreen(self,value):
        self.fullscreen=value
        for entry in list(self.full_clients):
            event(entry['client'],entry['top'],4,integer(8)+integer(3)+integer(2) if value else integer(0));event(entry['client'],entry['top'],5)
    def close(self):
        self.stop=True;self.thread.join(3);[t.join(3) for t in self.handlers];self.listener.close();assert not self.thread.is_alive();assert not self.errors,self.errors

def wait(test):
    deadline=time.monotonic()+8
    while not test():
        assert time.monotonic()<deadline,'gamma fixture timed out'
        time.sleep(.02)
def main():
    (ROOT/'.artifacts').mkdir(exist_ok=True)
    directory=Path(tempfile.mkdtemp(prefix='night-protocol-',dir=ROOT/'.artifacts'))
    display=directory/'wayland';fixture=Fixture(display)
    config=directory/'config.toml';config.write_text('[nightlight]\nenabled=true\nforce=true\ntemperature_night=4500\n')
    runtime=directory/'runtime';log=(directory/'coordinator.log').open('w')
    env=os.environ|{'WAYLAND_DISPLAY':str(display),'WAYLAND_DEBUG':'1','DBUS_SYSTEM_BUS_ADDRESS':'unix:path='+str(directory/'no-system-bus'),'PATH':str(directory/'bin')+':'+os.environ['PATH']}
    child=subprocess.Popen(['setpriv','--pdeathsig','KILL',ROOT/'target/debug/cabby','serve','--config',config,'--runtime-dir',runtime],env=env,stdout=log,stderr=log)
    def cli(command):return json.loads(subprocess.run([ROOT/'target/debug/cabby',command,'--runtime-dir',runtime],check=True,capture_output=True,text=True,timeout=6).stdout)['night_light']
    def state():return cli('nightlight-status')['state']
    try:
        wait(lambda:(runtime/'control.sock').exists());wait(lambda:len(fixture.uploads)>0 and state()=='active')
        fixture.hotplug();wait(lambda:len(fixture.uploads)>1)
        fixture.fail();wait(lambda:state()=='output-failed');assert cli('nightlight-status')['applied']==0
        cli('nightlight-toggle');wait(lambda:state()=='no-schedule');assert cli('nightlight-status')['enabled']
        cli('nightlight-toggle');wait(lambda:state()=='disabled')
        fixture.size=65537;cli('nightlight-force-toggle');wait(lambda:state()=='output-failed')
        assert fixture.restores>=3
        config.write_text('[nightlight]\nenabled=false\nforce=false\n');wait(lambda:state()=='disabled')
        fixture.size=256
        bindir=directory/'bin';bindir.mkdir()
        curl=bindir/'curl';curl.write_text("#!/usr/bin/python3\nimport json\nfrom pathlib import Path\np=Path(__file__).resolve().parents[1]/'lookups'\nwith p.open('a') as f:f.write('lookup\\n')\nprint(json.dumps({'lat':52.52,'lng':13.405}))\n");curl.chmod(0o700)
        # The coordinator must already have this fixture-only PATH; no public lookup.
        config.write_text('[nightlight]\nenabled=true\ntemperature_night=4500\n[location]\nauto_locate=true\n')
        wait(lambda:(directory/'lookups').exists() and state()=='active')
        lookups=(directory/'lookups').read_text()
        cli('nightlight-disable');wait(lambda:state()=='disabled')
        cli('nightlight-enable');wait(lambda:state()=='active')
        assert (directory/'lookups').read_text()==lookups,'toggle discarded the in-memory location cache'

        config.write_text('[caffeine]\nfullscreen=true\nmanual_pause_nightlight=true\n[nightlight]\nenabled=true\nforce=true\ntemperature_night=4500\n')
        wait(lambda:len(fixture.full_clients)>0 and state()=='active')
        def caffeine(command='caffeine-status'):
            return json.loads(subprocess.run([ROOT/'target/debug/cabby',command,'--runtime-dir',runtime],check=True,capture_output=True,text=True,timeout=6).stdout)['caffeine']
        caffeine('caffeine-on');wait(lambda:cli('nightlight-status')['inhibited_outputs']==1)
        fixture.set_fullscreen(True);wait(lambda:'fullscreen' in caffeine()['reasons'])
        caffeine('caffeine-off');assert 'fullscreen' in caffeine()['reasons']
        assert cli('nightlight-status')['inhibited_outputs']==1
        fixture.focus(False);wait(lambda:not caffeine()['active'])
        fixture.focus(True);wait(lambda:caffeine()['active'])
        fixture.visibility(False);wait(lambda:not caffeine()['active'] and state()=='active')
        fixture.visibility(True);wait(lambda:'fullscreen' in caffeine()['reasons'])
        fixture.minimize(True);wait(lambda:not caffeine()['active'])
        fixture.minimize(False);wait(lambda:'fullscreen' in caffeine()['reasons'])
        effects='[caffeine]\nfullscreen=true\ninhibit_idle=false\ninhibit_sleep=false\nmanual_pause_nightlight=true\nfullscreen_pause_nightlight=false\n[nightlight]\nenabled=true\nforce=true\ntemperature_night=4500\n'
        config.write_text(effects)
        wait(lambda:caffeine()['active'] and not caffeine()['idle_inhibited'] and not caffeine()['nightlight_paused'] and caffeine()['sleep_state']=='inactive' and state()=='active')
        config.write_text(effects.replace('inhibit_idle=false','inhibit_idle="invalid"'));time.sleep(.5)
        assert not caffeine()['idle_inhibited'], 'invalid config replaced valid policy'
        caffeine('caffeine-on');wait(lambda:cli('nightlight-status')['inhibited_outputs']==1)
        caffeine('caffeine-off');wait(lambda:state()=='active')
        fixture.focus(False);wait(lambda:not caffeine()['active'])
        config.write_text(effects.replace('fullscreen=true','fullscreen=true\nfullscreen_requires_focus=false'))
        wait(lambda:caffeine()['active'])
        fixture.set_fullscreen(False);wait(lambda:not caffeine()['active'] and state()=='active')
        assert cli('nightlight-status')['force'], 'inhibition rewrote night-light preference'
        shutil.copy2(ROOT/'scripts/fixtures/audio-helper.py',bindir/'pactl');(bindir/'pactl').chmod(0o700)
        audio_data=json.loads((ROOT/'scripts/fixtures/audio-state.json').read_text());audio_path=bindir/'audio-state.json';audio_path.write_text(json.dumps(audio_data))
        config.write_text('[caffeine]\naudio=true\nfullscreen=true\nmanual_pause_nightlight=true\n[nightlight]\nenabled=true\nforce=true\ntemperature_night=4500\n')
        wait(lambda:caffeine()['audio_state']=='watching')
        audio_data['sinks'][0]['state']='RUNNING';audio_data['version']+=1
        temporary=bindir/'state.tmp';temporary.write_text(json.dumps(audio_data));os.replace(temporary,audio_path)
        wait(lambda:'audio' in caffeine()['reasons']);assert cli('nightlight-status')['inhibited_outputs']==0
        audio_data['sinks'][0]['state']='IDLE';audio_data['sources'][0]['state']='RUNNING';audio_data['version']+=1
        temporary.write_text(json.dumps(audio_data));os.replace(temporary,audio_path);time.sleep(.4)
        assert 'audio' in caffeine()['reasons'], 'recording did not inhibit'
        audio_data['sources'][0]['state']='SUSPENDED';audio_data['version']+=1;temporary.write_text(json.dumps(audio_data));os.replace(temporary,audio_path)
        wait(lambda:not caffeine()['active'])
        caffeine('caffeine-on')
        (bindir/'audio-offline').touch()
        wait(lambda:caffeine()['audio_state']=='unavailable')
        assert caffeine()['reasons']==['manual']
        (bindir/'audio-offline').unlink()
        wait(lambda:caffeine()['audio_state']=='watching')
        caffeine('caffeine-off')
        config.write_text('[caffeine]\naudio=false\nfullscreen=false\n')
        wait(lambda:caffeine()['audio_state']=='disabled' and caffeine()['fullscreen_state']=='disabled')
        pids=set((bindir/'audio-pids').read_text().splitlines())
        wait(lambda:all(not Path('/proc',pid).exists() for pid in pids))

        (directory/'result.json').write_text(json.dumps({'passed':True,'checks':['native-gamma-FD-ramp','synthetic-output-hotplug','per-output-failure','force-to-scheduled-toggle','disable-restores','oversize-gamma-rejection','config-clears-overrides','location-cache-survives-toggle','fullscreen-and-manual-reasons-compose','per-output-nightlight-pause-and-restore','playback-and-recording-inhibition','audio-does-not-pause-nightlight','focus-output-and-minimized-gating','background-fullscreen-opt-in','all-effects-live-configurable','invalid-config-retains-policy','audio-disconnect-recovers-with-manual-preserved','disable-reaps-audio-helpers'],'unverified':['physical-color-temperature','driver-resume','multiple-physical-outputs']},indent=2))
        print(directory);print((directory/'result.json').read_text())
    finally:
        child.terminate()
        try:child.wait(timeout=5)
        except subprocess.TimeoutExpired:child.kill();child.wait(timeout=5)
        log.close();(directory/'trace.json').write_text(json.dumps(fixture.trace));fixture.close();(directory/'cleanup.txt').write_text('Owned coordinator reaped; private fixture thread/socket closed.\n')
if __name__=='__main__':main()
