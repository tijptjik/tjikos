"""Verify native settings float over their invoking window in private Hyprland."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

APP = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(APP.parent / 'runtjik/tests'))
from private_compositor import private_compositor, wait_for

root = Path(tempfile.mkdtemp(prefix='tasktjik-settings-window-'))
qml = root / 'qml'
shutil.copytree(APP / 'qml', qml)
(qml / 'shell.qml').write_text('''import QtQuick
import Quickshell
import Quickshell.Io
ShellRoot {
    FloatingWindow {id:parent;title:'Tasktjik fixture';implicitWidth:1000;implicitHeight:700;color:'#26233a'
        TaskSettings {id:settings;parentWindow:parent
            theme:({background:'#26233a',surface:'#39354b',text:'#f5f5f7',muted:'#aaa5ba',accent:'#a8c7a0',border:'#555169'})
            stateData:({groups:['All','Work','Socials'],tasks:[]})
        }
    }
    IpcHandler {target:'settings-test'
        function open():void {settings.open();}
        function close():void {settings.close();}
        function tab(value:string):void {settings.tab=value;}
        function capture(path:string):void {settings.panelItem.grabToImage(image=>image.saveToFile(path));}
    }
}
''')
rule_path = Path(sys.argv[1]) if len(sys.argv)>1 else APP.parents[2] / 'dotjik/native/home/files/.config/hypr/windows.lua.j2'
text = rule_path.read_text()
rule = text[text.index('-- Settings are separate native surfaces'):text.index("-- Playwright's headed Chrome")]
os.environ['AQ_BACKEND'] = 'headless'
with private_compositor(isolate_pointer=True):
    def hypr(*args):
        return subprocess.check_output(['hyprctl', *args], text=True)
    hypr('output','create','headless')
    wait_for(lambda: json.loads(hypr('-j','monitors')))
    hypr('eval', 'hl.monitor({output="",mode="1600x1000@60",position="0x0",scale=1}); hl.window_rule({name="fixture",float=true,no_anim=true,match={title="^Tasktjik fixture$"}})')
    hypr('eval','do\n'+rule+'\nend')
    assert not hypr('configerrors').strip(),hypr('configerrors')
    env={**os.environ,'QT_QPA_PLATFORM':'wayland','QT_QUICK_BACKEND':'software'}
    def ipc(method,*args):
        return subprocess.check_output(['quickshell','ipc','--path',str(qml),'call','settings-test',method,*map(str,args)],env=env,text=True)
    def client(title):
        return next((c for c in json.loads(hypr('-j','clients')) if c['title']==title),None)
    with (root/'qml.log').open('w') as log:
        process=subprocess.Popen(['quickshell','--path',str(qml)],env=env,stdout=log,stderr=log)
        try:
            parent=wait_for(lambda:client('Tasktjik fixture'))
            hypr('eval','for _,w in ipairs(hl.get_windows()) do if w.title=="Tasktjik fixture" then hl.dispatch(hl.dsp.window.move({window=w,x=120,y=140,relative=false}));hl.dispatch(hl.dsp.focus({window=w})) end end')
            parent=client('Tasktjik fixture')
            ipc('open')
            child=wait_for(lambda:client('Tasktjik Settings'))
            assert child['floating'],child
            def centered():
                child=client('Tasktjik Settings')
                return all(abs(child['at'][i]+child['size'][i]/2-parent['at'][i]-parent['size'][i]/2)<=2 for i in (0,1))
            try:
                wait_for(centered)
            except ValueError:
                print(json.dumps({'parent':parent,'child':client('Tasktjik Settings'),'errors':hypr('configerrors'),'log':hypr('rollinglog')[-5000:]}))
                raise
            ipc('capture',root/'general.png');wait_for(lambda:(root/'general.png').exists())
            ipc('tab','groups');ipc('capture',root/'groups.png');wait_for(lambda:(root/'groups.png').exists())
            ipc('close');wait_for(lambda:not client('Tasktjik Settings'))
            assert client('Tasktjik fixture')
            assert not hypr('configerrors').strip()
            print('PASS settings native floating and centered on invoking window:',root)
        finally:
            process.terminate();process.wait(timeout=5)
