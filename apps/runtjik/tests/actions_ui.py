"""Exercise shortcut/history keys using only the synthetic launcher."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import time
from private_compositor import private_compositor, wait_for

app = Path(__file__).resolve().parents[1]
with private_compositor():
    temp = Path(os.environ['XDG_RUNTIME_DIR']).parent
    home = temp/'app';home.mkdir()
    shutil.copytree(app/'qml',home/'qml')
    for name in ('runtjik.py','words.py','desktop_plugins.py','capture_worker.py','capture_countdown.py','capture_trash.py'):
        shutil.copy2(app/name,home/name)
    config=temp/'runtjik.conf';config.write_text('')
    env={**os.environ,'RUNTJIK_HOME':str(home),'RUNTJIK_DEMO':'1','RUNTJIK_CONFIG':str(config)}
    def ipc(*args):
        return subprocess.check_output(['quickshell','ipc','--path',str(home/'qml'),'call','runtjik',*args],text=True,stderr=subprocess.DEVNULL)
    def state():
        try:return json.loads(ipc('testState'))
        except subprocess.CalledProcessError:return {}
    def query(text):
        ipc('test',text);wait_for(lambda:state().get('resultQuery')==text)
    def key(*args):subprocess.run(['wtype',*args],check=True)
    with (temp/'launcher.log').open('w') as log:
        process=subprocess.Popen(['quickshell','--path',str(home/'qml'),'--no-duplicate'],env=env,stdout=log,stderr=log)
        try:
            wait_for(lambda:state().get('rows'),timeout=20)
            query('!ss');key('-k','Return')
            wait_for(lambda:state().get('resultQuery')=='/grab screen')
            query('!snap history');key('-k','Return');time.sleep(.2)
            query('!rec')
            for arrow,expected in [('Up','!snap history'),('Up','!ss'),('Down','!snap history'),('Down','!rec')]:
                key('-M','ctrl','-k',arrow,'-m','ctrl')
                wait_for(lambda:state().get('resultQuery')==expected)
            query('!ss history')
            key('-M','ctrl','-k','z','-m','ctrl')
            wait_for(lambda:'Trash unchanged' in json.loads(ipc('status'))['message'])
            print('PASS: !ss / !snap, Ctrl+Up/Down history/draft restoration, gallery Ctrl+Z',temp,flush=True)
        finally:
            process.terminate();process.wait(timeout=5)
