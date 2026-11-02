"""Synthetic resize workload: CPU, delegate churn, and optional per-client GPU time."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time
import sys

APP = Path(__file__).resolve().parents[1]
if '--wayland' in sys.argv:
    os.environ['AQ_BACKEND']='headless'
    sys.path.insert(0,str(APP.parent/'runtjik/tests'))
    from private_compositor import private_compositor, wait_for
    with private_compositor(isolate_pointer=True):
        subprocess.run(['hyprctl','output','create','headless'],check=True,stdout=subprocess.DEVNULL)
        wait_for(lambda:json.loads(subprocess.check_output(['hyprctl','-j','monitors'],text=True)))
        env={**os.environ,'TASKTJIK_BENCH_WAYLAND':'1'}
        env['WAYLAND_DISPLAY']=str(Path(os.environ['XDG_RUNTIME_DIR'])/os.environ['WAYLAND_DISPLAY'])
        result=subprocess.run([sys.executable,'-B',__file__],env=env)
    raise SystemExit(result.returncode)
root = Path(tempfile.mkdtemp(prefix='tasktjik-resize-'))
qml = root / 'qml'
shutil.copytree(APP / 'qml', qml)
(root / 'runtime').mkdir(mode=0o700)
(root / 'config/tasktjik').mkdir(parents=True)
source = Path(os.environ.get('TASKTJIK_BENCH_COMPONENT', qml / 'TodoApp.qml')).read_text()
source = source.replace('    id: app', '    id: app\n    property var metrics: ({created:0,balanced:0,painted:0,lines:0})')
source = source.replace('id:taskCard', 'id:taskCard\n                                            Component.onCompleted:app.metrics.created++')
source = source.replace('function balance() {', 'function balance() { app.metrics.balanced++;')
source = source.replace('onPaint:{', 'onPaint:{app.metrics.painted++;').replace('onPaint: {', 'onPaint: {app.metrics.painted++;')
source = source.replace('onLineLaidOut: line => {', 'onLineLaidOut: line => {app.metrics.lines++;')
(qml / 'TodoApp.qml').write_text(source)
groups = ['All', 'Work', 'Socials', 'Home', 'Travel', 'Books', 'General', 'Finance']
tasks = [dict(id=str(i), title='Synthetic task '+str(i)+' with a moderately long wrapping title to exercise layout',
              kind='todo', group=groups[i % len(groups)], due='', done=False, status='ready',
              tags=['synthetic', 'resize'], subtasks=[], period='once') for i in range(112)]
(qml / 'shell.qml').write_text('''import QtQuick
import Quickshell
import Quickshell.Io
ShellRoot {
    property int frame:0
    property bool finished:false
    FloatingWindow {id:win;implicitWidth:1500;implicitHeight:700
        TodoApp {id:view;width:800;height:parent.height;view:'all'
            theme:({background:'#26233a',surface:'#39354b',text:'#f5f5f7',muted:'#aaa5ba',accent:'#a8c7a0',border:'#555169'})
            stateData:FIXTURE
        }
    }
    Timer {id:resize;interval:16;repeat:true;onTriggered:{frame++;view.width=760+Math.abs(60-frame%120)*12;if(frame===120){stop();settle.start();}}}
    Timer {id:settle;interval:500;onTriggered:finished=true}
    IpcHandler {target:'resize-test'
        function start():void {view.metrics={created:0,balanced:0,painted:0,lines:0};frame=0;finished=false;resize.start();}
        function stats():string {return JSON.stringify({finished:finished,frame:frame,metrics:view.metrics});}
    }
}
'''.replace('FIXTURE', json.dumps(dict(tasks=tasks, groups=groups, today='2026-11-02'))))
env = {**os.environ, 'QT_QPA_PLATFORM':'offscreen', 'QT_QUICK_BACKEND':'software',
       'XDG_RUNTIME_DIR':str(root/'runtime'), 'XDG_CONFIG_HOME':str(root/'config'), 'XDG_CACHE_HOME':str(root/'cache')}
if os.environ.get('TASKTJIK_BENCH_WAYLAND'):
    env['QT_QPA_PLATFORM']='wayland';env.pop('QT_QUICK_BACKEND',None);env['QSG_INFO']='1'

def gpu_ticks(pid):
    clients={}
    for fd in Path(f'/proc/{pid}/fdinfo').iterdir():
        try:
            data=dict(line.split(':',1) for line in fd.read_text().splitlines() if ':' in line)
            if 'drm-client-id' in data:
                clients[data['drm-client-id']]=sum(int(value.strip().split()[0]) for key,value in data.items() if key.startswith('drm-engine-'))/1e9
        except (OSError,ValueError):pass
    return sum(clients.values()) if clients else None

def ipc(method):
    return subprocess.check_output(['quickshell','ipc','--path',str(qml),'call','resize-test',method],env=env,text=True,stderr=subprocess.DEVNULL)
def ticks(pid):
    stat=Path(f'/proc/{pid}/stat').read_text().rsplit(')',1)[1].split()
    return (int(stat[11])+int(stat[12]))/os.sysconf('SC_CLK_TCK')
with (root/'qml.log').open('w') as log:
    p=subprocess.Popen(['quickshell','--path',str(qml)],env=env,stdout=log,stderr=log)
    try:
        for _ in range(100):
            try:
                json.loads(ipc('stats'));break
            except (ValueError,subprocess.CalledProcessError):time.sleep(.1)
        time.sleep(.7)
        before=ticks(p.pid);gpu_before=gpu_ticks(p.pid);start=time.monotonic();ipc('start')
        while time.monotonic()-start<60:
            data=json.loads(ipc('stats'))
            if data['finished']:break
            time.sleep(.15)
        assert data['finished'],data
        if not os.environ.get('TASKTJIK_BENCH_COMPONENT'):
            assert data['metrics']['created']==0, data
        gpu_after=gpu_ticks(p.pid)
        print(json.dumps(dict(data,gpu_engine_seconds=round(gpu_after-gpu_before,3) if gpu_before is not None and gpu_after is not None else None,cpu_seconds=round(ticks(p.pid)-before,3),wall_seconds=round(time.monotonic()-start,3),artifacts=str(root))))
    finally:
        p.terminate();p.wait(timeout=5)
