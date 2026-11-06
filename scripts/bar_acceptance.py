"""Icon bar and passive hints exercised only inside the disposable compositor."""
import json
import os
from pathlib import Path
import re
import subprocess
import time
from virtual_pointer import Pointer

ROOT = Path(__file__).resolve().parents[1]

def check(directory, renderer, children, selected_adapter):
    def call(*args):
        return subprocess.run(args, check=True, capture_output=True, text=True, timeout=15).stdout
    def wait(test):
        end = time.monotonic()+12
        while not test():
            assert time.monotonic()<end, "bar acceptance timed out"
            time.sleep(.05)
    def layers(): return json.loads(call("niri","msg","--json","layers"))
    def shown(name): return any(v["namespace"]==name for v in layers())
    logical = next(iter(json.loads(call("niri","msg","--json","outputs")).values()))["logical"]
    size = logical["width"],logical["height"]
    config = directory/"bar-icons.toml"
    def configure(edge="top", light=False, widgets='["network"]'):
        config.write_text('osd={enabled=false}\nrenderer="'+('vulkan' if renderer=='gpu' else 'software')+'"\n'
                          'bar_height=40\nbar_left=[]\nbar_middle=[]\nbar_right='+widgets+'\nbar_position="'+edge+'"\n'+
                          ('background="#f2f2f7"\nforeground="#202128"\n' if light else ''))
    configure()
    env=os.environ|{"DBUS_SYSTEM_BUS_ADDRESS":"unix:path="+str(directory/"no-system-bus")}
    log=directory/"bar-icons.log"
    with (directory/"bar-coordinator.log").open("w") as output:
        coordinator=subprocess.Popen([ROOT/"target/debug/cabby","serve","--config",config],env=env,stdout=output,stderr=output)
        children.append(coordinator)
    with log.open("w") as output:
        desktop=subprocess.Popen([ROOT/"target/debug/cabby-desktop","--config",config],env=env,stdout=output,stderr=output)
        children.append(desktop)
        pointer=Pointer()
        try:
            wait(lambda:shown("cabby-bar") and ".attach(wl_buffer" in log.read_text())
            for edge,light in [("top",False),("bottom",False),("top",True)]:
                pointer.move(100,100,size)
                configure(edge,light)
                time.sleep(.8)
                pointer.move(size[0]-20,20 if edge=="top" else size[1]-20,size)
                wait(lambda:shown("cabby-bar-hint"))
                time.sleep(.3)
                trace=log.read_text()
                events=list(re.finditer(r'get_layer_surface\(new id zwlr_layer_surface_v1#(\d+), wl_surface#(\d+),[^\n]*"cabby-bar-hint"\)',trace))
                assert events
                layer,surface=events[-1].groups()
                body=trace[events[-1].end():].split("get_layer_surface(",1)[0]
                width,height=map(int,re.search(rf'zwlr_layer_surface_v1#{layer}\.set_size\((\d+), (\d+)\)',body).groups())
                top,_,bottom,left=map(int,re.search(rf'zwlr_layer_surface_v1#{layer}\.set_margin\((\d+), (\d+), (\d+), (\d+)\)',body).groups())
                y=top if edge=="top" else size[1]-bottom-height
                assert (y>=40 if edge=="top" else y+height<=size[1]-40)
                assert re.search(rf'zwlr_layer_surface_v1#{layer}\.set_keyboard_interactivity\(0\)',body)
                region=re.search(rf'wl_surface#{surface}\.set_input_region\(wl_region#(\d+)\)',body)[1]
                lifetime=re.search(rf'create_region\(new id wl_region#{region}\)(.*?)wl_region#{region}\.destroy\(\)',body,re.S)
                assert lifetime and not re.search(rf'wl_region#{region}\.add\(',lifetime[1])
                assert not re.search(r'wl_keyboard#\d+\.enter\(',trace), "hint took keyboard focus"
                capture=directory/f'bar-hint-{edge}-{light}.png'
                call("grim",str(capture))
                call(str(ROOT/"target/debug/wallpaper-pixel-probe"),str(capture),f'bar-hint-{size[0]}-{size[1]}-{left}-{y}-{width}-{height}-{int(light)}')
                pointer.move(100,100,size)
                wait(lambda:not shown("cabby-bar-hint"))
            configure(widgets='["clipboard"]')
            time.sleep(.8)
            pointer.click(size[0]-20,20,size)
            wait(lambda:shown("cabby-clipboard"))
            pointer.click(100,size[1]-100,size)
            wait(lambda:not shown("cabby-clipboard"))
            configure(widgets='["audio","network","bluetooth","caffeine","nightlight","clipboard","screenshot","apps","idle"]')
            time.sleep(.8)
            capture=directory/"bar-all-icons.png"
            call("grim",str(capture))
            call(str(ROOT/"target/debug/wallpaper-pixel-probe"),str(capture),'bar-all-icons')
            adapters={}
            if renderer=="gpu":
                adapter=selected_adapter(log.read_text())
                assert adapter["backend"]=="Vulkan" and 'RTX 4070' in adapter["name"]
                adapters['bar']=adapter
            (directory/"result.json").write_text(json.dumps({'status':'passed','adapters':adapters,'checks':['all-status-action-icons','opaque-dark-light-hints','top-bottom-placement','hover-exit-dismissal','passive-no-focus','clipboard-icon-opens-picker'],'not_proven':['physical-multi-output-pointer']},indent=2))
        finally:
            pointer.close()
            desktop.terminate();desktop.wait(timeout=5)
            coordinator.terminate();coordinator.wait(timeout=5)
