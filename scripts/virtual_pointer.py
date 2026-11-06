"""Test-only virtual pointer. Refuses any display outside the disposable runtime."""
import os,socket,struct,time
from pathlib import Path
U=lambda n:struct.pack('=I',n)
def string(value):
    data=value.encode()+b'\0';return U(len(data))+data+b'\0'*((-len(data))%4)
class Pointer:
    def __init__(self):
        display=os.environ['WAYLAND_DISPLAY'];assert os.environ.get('CABBY_TEST_WAYLAND')==display
        display=Path(display);display=display if display.is_absolute() else Path(os.environ['XDG_RUNTIME_DIR'])/display
        assert display.parent==Path(os.environ['XDG_RUNTIME_DIR']) and '.artifacts' in str(display)
        self.socket=socket.socket(socket.AF_UNIX);self.socket.settimeout(4);self.socket.connect(str(display));self.buffer=b'';self.next=6;self.manager=None
        self.send(1,1,U(2));self.send(1,0,U(3))
        while True:
            obj,op,args=self.event()
            if obj==2 and op==0:
                name,length=struct.unpack('=II',args[:8]);interface=args[8:8+length-1].decode();position=8+((length+3)//4*4);version=struct.unpack('=I',args[position:position+4])[0]
                if interface=='zwlr_virtual_pointer_manager_v1':self.manager=(name,min(version,2))
            if obj==3 and op==0:break
        assert self.manager,'private compositor has no virtual pointer'
        name,version=self.manager;self.send(2,0,U(name)+string('zwlr_virtual_pointer_manager_v1')+U(version)+U(4));self.send(4,0,U(0)+U(5));self.sync()
    def send(self,obj,op,args=b''):self.socket.sendall(U(obj)+U(((len(args)+8)<<16)|op)+args)
    def event(self):
        while len(self.buffer)<8:self.buffer+=self.socket.recv(65536)
        obj,header=struct.unpack('=II',self.buffer[:8]);length=header>>16
        while len(self.buffer)<length:self.buffer+=self.socket.recv(65536)
        args=self.buffer[8:length];self.buffer=self.buffer[length:]
        if obj==1 and (header&65535)==0:raise RuntimeError('private Wayland pointer error')
        return obj,header&65535,args
    def sync(self):
        callback=self.next;self.next+=1;self.send(1,0,U(callback))
        while True:
            obj,op,_=self.event()
            if obj==callback and op==0:break
    def move(self,x,y,size):
        self.send(5,1,b''.join(U(int(v)) for v in [int(time.monotonic()*1000)&0xffffffff,x,y,*size]));self.send(5,4);self.sync();time.sleep(.08)
    def button(self,pressed):
        self.send(5,2,U(int(time.monotonic()*1000)&0xffffffff)+U(272)+U(int(pressed)));self.send(5,4);self.sync();time.sleep(.08)
    def click(self,x,y,size):self.move(x,y,size);self.button(True);self.button(False)
    def drag(self,a,b,size):self.move(*a,size);self.button(True);self.move(*b,size);self.button(False)
    def close(self):self.socket.close()
