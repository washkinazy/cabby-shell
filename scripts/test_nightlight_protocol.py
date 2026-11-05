#!/usr/bin/env python3
"""Private Wayland gamma fixture: synthetic outputs and FDs, never a real display."""
import array, json, os, socket, struct, subprocess, tempfile, threading, time
from pathlib import Path
ROOT = Path(__file__).resolve().parents[1]
def integer(n): return struct.pack('=I',n)
def string(s):
    b=s.encode()+b'\0';return integer(len(b))+b+b'\0'*((-len(b))%4)
def event(client,obj,opcode,payload=b''): client.sendall(integer(obj)+integer(((len(payload)+8)<<16)|opcode)+payload)
class Fixture:
    def __init__(self,path):
        self.listener=socket.socket(socket.AF_UNIX,socket.SOCK_STREAM);self.listener.bind(str(path));self.listener.listen();self.listener.settimeout(.1)
        self.stop=False;self.trace=[];self.errors=[];self.uploads=[];self.restores=0;self.current=None;self.size=256;self.thread=threading.Thread(target=self.run);self.thread.start()
    def run(self):
        try:
            while not self.stop:
                try: client,_=self.listener.accept()
                except socket.timeout: continue
                client.settimeout(.1);objects={1:'display'};buf=b'';fds=[];registry=None;control=None
                self.current=(client,lambda:registry,lambda:control)
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
                            elif kind=='display' and op==0:
                                callback=struct.unpack('=I',args)[0];event(client,callback,0,integer(0));event(client,1,1,integer(callback))
                            elif kind=='registry' and op==0:
                                _,length=struct.unpack('=II',args[:8]);interface=args[8:8+length-1].decode();position=8+((length+3)//4*4);version,newid=struct.unpack('=II',args[position:position+8]);objects[newid]='manager' if interface=='zwlr_gamma_control_manager_v1' else 'output'
                            elif kind=='manager' and op==0:
                                control,output=struct.unpack('=II',args);assert objects[output]=='output';objects[control]='gamma';event(client,control,0,integer(self.size))
                            elif kind=='gamma' and op==0:
                                assert fds;fd=fds.pop(0)
                                try:
                                    raw=os.pread(fd,500000,0);assert len(raw)==self.size*6
                                    values=struct.unpack('='+str(self.size*3)+'H',raw)
                                    assert values[self.size-1]>values[self.size*2-1]>values[self.size*3-1]
                                    self.uploads.append(values)
                                finally: os.close(fd)
                            elif kind in ('gamma','manager','output'):
                                assert op==(1 if kind!='output' else 0)
                                if kind=='gamma': self.restores+=1;control=None
                                del objects[obj];event(client,1,1,integer(obj))
                            else: raise AssertionError((kind,op))
                except (BrokenPipeError,ConnectionResetError): pass
                finally:
                    for fd in fds:os.close(fd)
                    client.close();self.current=None
        except Exception as error:self.errors.append(repr(error))
    def fail(self):
        client,_,control=self.current;self.trace.append(("fail",control()));event(client,control(),1)
    def hotplug(self):
        client,registry,_=self.current;event(client,registry(),1,integer(12));event(client,registry(),0,integer(13)+string('wl_output')+integer(4))
    def close(self):
        self.stop=True;self.thread.join(3);self.listener.close();assert not self.thread.is_alive();assert not self.errors,self.errors

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

        (directory/'result.json').write_text(json.dumps({'passed':True,'checks':['native-gamma-FD-ramp','synthetic-output-hotplug','per-output-failure','force-to-scheduled-toggle','disable-restores','oversize-gamma-rejection','config-clears-overrides','location-cache-survives-toggle'],'unverified':['physical-color-temperature','driver-resume','multiple-physical-outputs']},indent=2))
        print(directory);print((directory/'result.json').read_text())
    finally:
        child.terminate()
        try:child.wait(timeout=5)
        except subprocess.TimeoutExpired:child.kill();child.wait(timeout=5)
        log.close();(directory/'trace.json').write_text(json.dumps(fixture.trace));fixture.close();(directory/'cleanup.txt').write_text('Owned coordinator reaped; private fixture thread/socket closed.\n')
if __name__=='__main__':main()
