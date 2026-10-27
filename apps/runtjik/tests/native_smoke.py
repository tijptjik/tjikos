# Real Wayland regression test; launches only synthetic Kitty windows in a private compositor.
import json,os,subprocess,time,sys
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[2]/'bartjik'))
from demo_cycle import private_compositor
app=Path(__file__).resolve().parents[1]
base=['quickshell','ipc','--path',str(app/'qml'),'call','runtjik']
def wait(fn):
    for _ in range(120):
        try:
            v=fn()
            if v:return v
        except (ValueError,StopIteration,subprocess.CalledProcessError):pass
        time.sleep(.05)
    raise AssertionError('Timed out')
def ipc(*args):return subprocess.check_output(base+list(args),text=True,stderr=subprocess.DEVNULL)
def status():return json.loads(ipc('status'))
def exited(pid):
    try:return Path(f'/proc/{pid}/stat').read_text().rsplit(')',1)[1].split()[0]=='Z'
    except OSError:return True
with private_compositor():
    wait(lambda:bool(json.loads(subprocess.check_output(['hyprctl','-j','monitors'],text=True))))
    time.sleep(2)
    base_dir=Path(os.environ['XDG_CONFIG_HOME']).parent
    os.environ['XDG_STATE_HOME']=str(base_dir/'state')
    config=Path(os.environ['XDG_CONFIG_HOME'])/'runtjik/runtjik.conf';config.parent.mkdir();config.write_text('[index]\npath_commands=false\n[launch]\nisolate=false\n')
    os.environ['RUNTJIK_CONFIG']=str(config)
    apps=Path(os.environ['XDG_DATA_HOME'])/'applications';apps.mkdir(parents=True)
    (apps/'runtjik-qa.desktop').write_text('[Desktop Entry]\nType=Application\nName=Runtjik QA Window\nExec=kitty --class runtjik-qa --title SyntheticQA sleep 90\nStartupWMClass=runtjik-qa\nIcon=kitty\n')
    def clients():return json.loads(subprocess.check_output(['hyprctl','-j','clients']))
    def windows():return [c for c in clients() if c['class']=='runtjik-qa']
    def launch():
        subprocess.run([str(app/'bin/runtjik'),'show'],check=True)
        p=status()['pid']
        wait(lambda:status()['ready']);time.sleep(.5)
        subprocess.run(['wtype','Runtjik QA'],check=True);time.sleep(.2)
        return p
    for count in (1,2):
        p=launch();keys=['-M','shift','-k','Return','-m','shift'] if count==1 else ['-k','Return'];subprocess.run(['wtype',*keys],check=True);wait(lambda:not status()['visible'] and status()['ready']);assert not exited(p),'Action killed the warm launcher';wait(lambda:len(windows())==count)
    p=launch();subprocess.run(['wtype','-M','shift','-k','Return','-m','shift'],check=True)
    wait(lambda:status()['windowPicker']);time.sleep(.7)
    current=status()
    assert current['layer']=='top' and current['geometry']['width']==720
    assert not any(c['pid']==p for c in clients()),'Picker entered client layout'
    print('Top-layer instance picker remained mapped',flush=True)
    subprocess.run(['wtype','-k','Return'],check=True);wait(lambda:not status()['visible'] and status()['ready']);assert not exited(p),'Action killed the warm launcher'
    wait(lambda:json.loads(subprocess.check_output(['hyprctl','-j','activewindow']))['class']=='runtjik-qa')
    counts=json.loads(subprocess.check_output([str(app/'bin/runtjik'),'stats']))
    print('Two real launches and focus passed; counts',counts,flush=True)
    target=windows()[0]['address']
    subprocess.run(['hyprctl','eval','hl.dispatch(hl.dsp.window.close({window="address:'+target+'"}))'],check=True,stdout=subprocess.DEVNULL)
    wait(lambda:len(windows())==1)
    p=launch();subprocess.run(['wtype','-M','shift','-k','Return','-m','shift'],check=True);wait(lambda:not status()['visible'] and status()['ready']);assert not exited(p),'Action killed the warm launcher'
    wait(lambda:json.loads(subprocess.check_output(['hyprctl','-j','activewindow']))['class']=='runtjik-qa')
    counts=json.loads(subprocess.check_output([str(app/'bin/runtjik'),'stats']))
    assert counts['launch:apps:runtjik-qa.desktop']['count']==2
    assert counts['switch:apps:runtjik-qa.desktop']['count']==2
    print('Single-instance direct switch also passed; launcher remains resident',flush=True)
    subprocess.run([str(app/'bin/runtjik'),'show'],check=True)
    subprocess.run(['wtype','/pin'],check=True);time.sleep(.2)
    subprocess.run(['wtype','-k','Return'],check=True);wait(lambda:status()['pinned'])
    subprocess.run(['wtype','Runtjik QA'],check=True);time.sleep(.2)
    subprocess.run(['wtype','-k','Return'],check=True)
    wait(lambda:len(windows())==2)
    wait(lambda:json.loads(subprocess.check_output([str(app/'bin/runtjik'),'stats']))['launch:apps:runtjik-qa.desktop']['count']==3)
    assert status()['visible'] and status()['pinned'] and not status()['keyboardOwned'],status()
    ipc('focusSearch');time.sleep(.2)
    subprocess.run(['wtype','-M','shift','-k','Return','-m','shift'],check=True)
    wait(lambda:status()['windowPicker'])
    subprocess.run(['wtype','-k','Return'],check=True)
    wait(lambda:json.loads(subprocess.check_output([str(app/'bin/runtjik'),'stats']))['switch:apps:runtjik-qa.desktop']['count']==3)
    assert status()['visible'] and status()['pinned'] and not status()['keyboardOwned'],status()
    wait(lambda:json.loads(subprocess.check_output(['hyprctl','-j','activewindow']))['class']=='runtjik-qa')
    print('Pinned launch and switch stay visible and give keyboard focus to the app',flush=True)
    subprocess.run([str(app/'bin/runtjik'),'quit'],check=True)
