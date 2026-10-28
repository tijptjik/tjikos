import json,os,subprocess,time,sys
from pathlib import Path
from private_compositor import private_compositor
app=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(app))
import runtjik as backend
sys.path.insert(0,str(app))
from runtjik import SHORTCUTS
def call(args):return subprocess.check_output(args,text=True,stderr=subprocess.DEVNULL)
def wait(fn):
    for _ in range(120):
        try:
            v=fn()
            if v:return v
        except (ValueError,StopIteration,subprocess.CalledProcessError):pass
        time.sleep(.05)
    raise AssertionError('Timed out')
with private_compositor():
    wait(lambda:len(json.loads(call(['hyprctl','-j','monitors'])))>0)
    call(['hyprctl','output','create','headless'])
    initial=wait(lambda:next((m for m in json.loads(call(['hyprctl','-j','monitors'])) if m['name'].startswith('HEADLESS')),None))
    call(['hyprctl','eval','hl.monitor({output='+json.dumps(initial['name'])+',mode="1000x680@60",position="-3000x0",scale=1})'])
    call(['hyprctl','eval','hl.dispatch(hl.dsp.focus({monitor='+json.dumps(initial['name'])+'}))'])
    time.sleep(.4)
    tmp=Path(os.environ['XDG_RUNTIME_DIR']).parent
    with (tmp/'hyprland.lua').open('a') as config:
        config.write('hl.monitor({output='+json.dumps(initial['name'])+',mode="1000x680@60",position="-3000x0",scale=1})\n')
    os.environ['XDG_STATE_HOME']=str(tmp/'state')
    pointer=tmp/'pointer'
    protocol=Path(__file__).with_name('virtual_pointer.xml')
    subprocess.run(['wayland-scanner','client-header',str(protocol),str(tmp/'pointer.h')],check=True)
    subprocess.run(['wayland-scanner','private-code',str(protocol),str(tmp/'protocol.c')],check=True)
    flags=call(['pkg-config','--cflags','--libs','wayland-client']).split()
    subprocess.run(['cc',str(app/'tests/pointer.c'),str(tmp/'protocol.c'),'-I'+str(tmp),'-o',str(pointer),*flags],check=True)
    receiver=tmp/'receiver';receiver.mkdir()
    (receiver/'shell.qml').write_text('''import QtQuick
import Quickshell
import Quickshell.Io
ShellRoot {
    property int clicks: 0
    IpcHandler {target: "receiver";function count(): string {return String(clicks);}}
    FloatingWindow {visible:true;title:"Click receiver";color:"#123456";implicitWidth:1000;implicitHeight:680
        MouseArea {anchors.fill:parent;onClicked:clicks++;}
    }
}''')
    q=subprocess.Popen(['quickshell','--path',str(receiver)],stdout=open(tmp/'receiver.log','w'),stderr=subprocess.STDOUT)
    def clients():return json.loads(call(['hyprctl','-j','clients']))
    wait(lambda:any(c['title']=='Click receiver' for c in clients()))
    original=next(c for c in clients() if c['title']=='Click receiver')
    before=len(json.loads(call(['hyprctl','-j','binds'])))
    mon=next(m for m in json.loads(call(['hyprctl','-j','monitors'])) if m['id']==original['monitor'])
    early=subprocess.Popen([str(pointer),mon['name']],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
    early.stdin.write('m 200 200\n');early.stdin.flush();assert early.stdout.readline().strip()=='ok'
    time.sleep(.2)
    p=subprocess.Popen([str(app/'bin/runtjik'),'--demo'],stdout=open(tmp/'runtjik.log','w'),stderr=subprocess.STDOUT)
    ipc=['quickshell','ipc','--path',str(app/'qml'),'call','runtjik']
    def status():return json.loads(call(ipc+['status']))
    try:
        wait(lambda:status()['ready']);time.sleep(.8)
        assert status()['clickWatchError']=='',status()
        subprocess.run(['wtype','/shortcuts'],check=True);time.sleep(.3)
        assert status()['results']==len(SHORTCUTS),('No keyboard focus with pointer outside',status())
        call(ipc+['test','']);time.sleep(.2)
        early.stdin.close();early.wait()
        print('Keyboard focus with pointer outside: PASS',flush=True)
        c=next(c for c in clients() if c['title']=='Click receiver');g=status()['geometry'];r={'at':[g['x'],g['y']],'size':[g['width'],g['height']]}
        assert not any(c['pid']==p.pid for c in clients()),'Launcher entered normal window layout'
        layers=json.loads(call(['hyprctl','-j','layers']))
        assert any(l['namespace']=='runtjik' and l['pid']==p.pid for mon in layers.values() for l in mon['levels'].get('2',[])), 'Not in top layer'
        assert (c['at'],c['size'])==(original['at'],original['size']),'Launcher altered normal window layout'
        m=next(m for m in json.loads(call(['hyprctl','-j','monitors'])) if m['id']==c['monitor'])
        ptr=subprocess.Popen([str(pointer),m['name']],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
        def send(line):ptr.stdin.write(line+'\n');ptr.stdin.flush();assert ptr.stdout.readline().strip()=='ok'
        def move(x,y):send('m '+str(round((x-m['x'])/(m['width']/m['scale'])*10000))+' '+str(round((y-m['y'])/(m['height']/m['scale'])*10000)))
        # Inside header: click and drag must keep the launcher open.
        x,y=r['at'][0]+r['size'][0]/2,r['at'][1]+10
        move(x,y);time.sleep(.1);send('b 1');time.sleep(.1);move(x+80,y+30);time.sleep(.2);send('b 0');time.sleep(.3)
        assert p.poll() is None
        g=status()['geometry'];moved=[g['x'],g['y']]
        print('DRAG',r['at'],moved,flush=True)
        assert abs(moved[0]-r['at'][0]-80)<4 and abs(moved[1]-r['at'][1]-30)<4
        # Real pointer grab across outputs; the layer moves when released.
        call(['hyprctl','output','create','headless'])
        second=wait(lambda:next((v for v in json.loads(call(['hyprctl','-j','monitors'])) if v['name'].startswith('HEADLESS') and v['name']!=m['name']),None))
        second_x=m['x']-1000
        with (tmp/'hyprland.lua').open('a') as config:
            config.write('hl.monitor({output='+json.dumps(second['name'])+',mode="1000x680@60",position="'+str(second_x)+'x'+str(m['y'])+'",scale=1})\n')
        call(['hyprctl','eval','hl.monitor({output='+json.dumps(second['name'])+',mode="1000x680@60",position="'+str(second_x)+'x'+str(m['y'])+'",scale=1})'])
        time.sleep(.4)
        def drag_to(gx,gy):
            g=status()['geometry']
            hx,hy=g['x']+g['width']/2,g['y']+10
            send('r 0 100');time.sleep(.1)
            cursor=json.loads(call(['hyprctl','-j','cursorpos']))
            send('r '+str(hx-cursor['x'])+' '+str(hy-cursor['y']))
            time.sleep(.15);send('b 1');time.sleep(.1)
            send('r '+str(gx-hx)+' '+str(gy-hy));time.sleep(.2)
            send('b 0');time.sleep(.4)
        drag_to(second_x+500,m['y']+70)
        wait(lambda:status()['monitor']==second['name'] and status()['ready'])
        assert status()['clickWatchError']=='',status()
        subprocess.run(['grim','-o',second['name'],str(tmp/'second.png')],check=True)
        call(ipc+['test','/pin']);time.sleep(.2)
        subprocess.run(['wtype','-k','Return'],check=True)
        wait(lambda:status()['pinned'])
        subprocess.run(['wtype','/shortcuts'],check=True);time.sleep(.15)
        assert status()['results']==len(SHORTCUTS)
        call(ipc+['test','']);time.sleep(.15)
        drag_to(r['at'][0]+r['size'][0]/2+80,r['at'][1]+40)
        wait(lambda:status()['monitor']==m['name'] and status()['ready'])
        assert status()['pinned']
        g=status()['geometry'];moved=[g['x'],g['y']]
        print('Cross-monitor drag both directions and keyboard focus: PASS',flush=True)
        subprocess.run(['grim' ,'-g',f"{round(g['x'])-10},{round(g['y'])-10} {g['width']+20}x{g['height']+20}",str(tmp/'composited.png')],check=True)
        # Reloading config clears dynamic bindings; the launcher must re-arm them.
        call(['hyprctl','reload','config-only']);time.sleep(.5)
        assert len(json.loads(call(['hyprctl','-j','binds'])))==before+8
        c=next(c for c in clients() if c['title']=='Click receiver')
        m=next(v for v in json.loads(call(['hyprctl','-j','monitors'])) if v['name']==m['name'])
        x,y=c['at'][0]+30,c['at'][1]+30
        assert not (moved[0]<=x<moved[0]+720 and moved[1]<=y<moved[1]+600),(moved,x,y)
        move(x,y);time.sleep(.4);assert p.poll() is None,'Hover must not dismiss'
        send('b 1');time.sleep(.2);send('b 0');time.sleep(.3)
        assert p.poll() is None,'Pinned outside click dismissed the launcher'
        wait(lambda:call(['quickshell','ipc','--path',str(receiver),'call','receiver','count']).strip()=='1')
        # A pinned launcher remains draggable after giving another app focus.
        wait(lambda:not status()['keyboardOwned'])
        g=status()['geometry']
        drag_to(g['x']+g['width']/2-30,g['y']+10-20)
        moved_again=status()['geometry']
        assert abs(moved_again['x']-g['x']+30)<4 and abs(moved_again['y']-g['y']+20)<4, (g,moved_again)
        assert status()['pinned'] and status()['visible']
        # Refocus the search box, then toggle pin off through the command UI.
        g=status()['geometry'];move(g['x']+200,g['y']+42)
        send('b 1');send('b 0');time.sleep(.15)
        call(ipc+['test','/pin']);time.sleep(.15)
        subprocess.run(['wtype','-k','Return'],check=True)
        wait(lambda:not status()['pinned']);time.sleep(.2)
        move(x,y);send('b 1');time.sleep(.2);send('b 0')
        p.wait(timeout=4)
        wait(lambda:call(['quickshell','ipc','--path',str(receiver),'call','receiver','count']).strip()=='2')
        assert len(json.loads(call(['hyprctl','-j','binds'])))==before,'Mouse observers leaked'
        ptr.stdin.close();ptr.wait()
        print('PASS: dragging; hover does not dismiss; outside click closes and reaches underlying window; reload re-arms; bindings cleaned up',flush=True)
    finally:
        for proc in [p,q]:
            if proc.poll() is None:proc.terminate();proc.wait(timeout=5)
        print('Logs:',tmp,flush=True)
