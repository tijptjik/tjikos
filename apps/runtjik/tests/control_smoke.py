"""Real startup races, idempotent show, shutdown and failed-start recovery."""
import concurrent.futures
import json
import os
from pathlib import Path
import subprocess
import sys
import time
from private_compositor import private_compositor
app=Path(__file__).resolve().parents[1]
with private_compositor():
    # A nested Wayland output can appear late or disappear with its parent window.
    subprocess.run(['hyprctl','output','create','headless'],check=True,stdout=subprocess.DEVNULL)
    for _ in range(100):
        if json.loads(subprocess.check_output(['hyprctl','-j','monitors'])):break
        time.sleep(.05)
    root=Path(os.environ['XDG_CONFIG_HOME']).parent
    os.environ['XDG_STATE_HOME']=str(root/'state')
    tools=root/'test-bin';tools.mkdir()
    notifier=tools/'notify-send';notifier.write_text('#!/bin/sh\nexit 0\n');notifier.chmod(0o755)
    os.environ['PATH']=str(tools)+os.pathsep+os.environ['PATH']
    config=root/'runtjik.conf'
    config.write_text('[index]\ndesktop_apps=false\npath_commands=false\n')
    os.environ['RUNTJIK_CONFIG']=str(config)
    def run(action,check=True):
        return subprocess.run([str(app/'bin/runtjik'),action],capture_output=True,text=True,timeout=16,check=check)
    def status():return json.loads(run('status').stdout)
    def layer_count():
        layers=json.loads(subprocess.check_output(['hyprctl','-j','layers']))
        return sum(l['namespace']=='runtjik' for m in layers.values() for ls in m['levels'].values() for l in ls)
    try:
        baseline_binds=len(json.loads(subprocess.check_output(['hyprctl','-j','binds'])))
        resident_pid=None
        for _ in range(8):
            run('show');s=status();assert s['ready'] and s['layer']=='top',s
            if resident_pid is not None:assert s['pid']==resident_pid,'Warm reopen restarted the process'
            resident_pid=s['pid']
            run('show');assert status()['pid']==s['pid']
            run('close')
            for _ in range(100):
                if layer_count()==0:break
                time.sleep(.005)
            assert layer_count()==0
            assert len(json.loads(subprocess.check_output(['hyprctl','-j','binds'])))==baseline_binds,'Hidden launcher retained mouse bindings'
            hidden=status();assert hidden['pid']==s['pid'] and hidden['ready'] and not hidden['visible'],hidden
        with concurrent.futures.ThreadPoolExecutor(max_workers=3) as pool:
            list(pool.map(run,['show']*3))
        assert layer_count()==1
        run('close')
        with concurrent.futures.ThreadPoolExecutor(max_workers=3) as pool:
            list(pool.map(run,['toggle']*3))
        for _ in range(100):
            if status()['ready'] and status()['visible'] and layer_count()==1:break
            time.sleep(.01)
        assert status()['ready'] and status()['visible'] and layer_count()==1
        run('close')
        subprocess.run(['hyprctl','eval','hl.bind("SUPER + Escape",hl.dsp.exec_cmd('+json.dumps(str(app/'bin/runtjik')+' toggle')+'))'],check=True,stdout=subprocess.DEVNULL)
        for order,hold in [('escape-first',100),('super-first',100),('super-first',800)]:
            tail=['-p','Escape','-s','100','-m','logo'] if order=='escape-first' else ['-m','logo','-s',str(hold),'-p','Escape']
            chord=['wtype','-M','logo','-P','Escape','-s','100',*tail]
            subprocess.run(chord,check=True)
            for _ in range(100):
                if layer_count()==1:break
                time.sleep(.05)
            assert layer_count()==1,('Shortcut failed to open',order,hold)
            time.sleep(.5)
            subprocess.run(chord,check=True);time.sleep(.6)
            assert layer_count()==0,('Shortcut failed to close',order,hold)
        run('quit');assert run('status',check=False).returncode!=0
        config.write_text('[ui]\ncolumns="invalid"\n')
        bad=run('show',check=False)
        assert bad.returncode!=0 and 'startup' in bad.stderr,bad
        assert layer_count()==0
        config.write_text('[index]\ndesktop_apps=false\npath_commands=false\n')
        run('show');assert status()['ready']
        # Reopening reloads external TOML edits and respects the memory opt-out.
        run('close')
        config.write_text('[ui]\nwidth=640\nkeep_alive=false\n[index]\ndesktop_apps=false\npath_commands=false\n')
        run('show')
        for _ in range(100):
            if status()['geometry']['width']==640:break
            time.sleep(.02)
        assert status()['geometry']['width']==640
        run('close')
        for _ in range(100):
            if run('status',check=False).returncode!=0:break
            time.sleep(.02)
        assert run('status',check=False).returncode!=0,'keep_alive=false did not quit'
        print('PASS: 8 warm hide/show cycles retaining PID, idempotent show, concurrent show/toggle, both key-release orders, held-key toggle, failed-start cleanup and recovery',flush=True)
    finally:
        run('quit',check=False)
        print('Private startup logs:',root/'state/runtjik/launcher.log',flush=True)
