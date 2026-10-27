"""Exercise true wheel paging, row geometry, aliases and shell icons in a private compositor."""
import json
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import sys
import time

app=Path(__file__).resolve().parents[1]
bartjik=Path(os.environ.get('RUNTJIK_TEST_BARTJIK',str(app.parent/'bartjik')))
sys.path.insert(0,str(bartjik))
from demo_cycle import private_compositor


def call(argv):return subprocess.check_output(argv,text=True,stderr=subprocess.DEVNULL)
def wait(check):
    for _ in range(200):
        try:
            value=check()
            if value:return value
        except (ValueError,subprocess.CalledProcessError):pass
        time.sleep(.05)
    raise AssertionError('Progressive UI fixture timed out')


with private_compositor():
    temp=Path(os.environ['XDG_RUNTIME_DIR']).parent
    os.environ['XDG_STATE_HOME']=str(temp/'state')
    os.environ['XDG_DATA_DIRS']=os.environ.get('RUNTJIK_TEST_DATA','/home/io/.local/share')+':/home/io/.local/share:/usr/local/share:/usr/share'
    wait(lambda:json.loads(call(['hyprctl','-j','monitors'])))
    time.sleep(2)
    home=temp/'app';home.mkdir()
    shutil.copytree(app/'qml',home/'qml')
    source=(app/'runtjik.py').read_text()
    start=source.index('def demo_entries():')
    end=source.index('def main():',start)
    fixtures='''def demo_entries():
    apps=[Entry('apps:chatgpt','ChatGPT',icon='chatgpt',data={'classes':['chatgpt']})]
    apps += [Entry('apps:'+str(i),'Entry '+str(i).zfill(3),icon='terminal') for i in range(260)]
    apps += [Entry('apps:zed','Zed Preview',icon='zed-preview',data={'classes':['zed-preview']})]
    return apps + [Entry('commands:'+name,name,icon=COMMAND_ICONS.get(name,COMMAND_ICON),provider='commands',data={'argv':['/usr/bin/'+name]}) for name in ('bash','flatpak','dnf','fish')]
def demo_windows():
    return [{'address':'0xdemo1','class':'chatgpt'},{'address':'0xdemo2','class':'zed-preview'}]


'''
    (home/'runtjik.py').write_text(source[:start]+fixtures+source[end:])
    config=temp/'config.conf';config.write_text('[theme]\nicon_theme="rose-punk"\n')
    os.environ.update(RUNTJIK_HOME=str(home),RUNTJIK_DEMO='1',RUNTJIK_CONFIG=str(config))
    pointer=temp/'pointer'
    subprocess.run(['wayland-scanner','client-header',str(bartjik/'demo_pointer.xml'),str(temp/'pointer.h')],check=True)
    subprocess.run(['wayland-scanner','private-code',str(bartjik/'demo_pointer.xml'),str(temp/'protocol.c')],check=True)
    flags=shlex.split(call(['pkg-config','--cflags','--libs','wayland-client']))
    subprocess.run(['cc',str(app/'tests/pointer.c'),str(temp/'protocol.c'),'-I'+str(temp),'-o',str(pointer),*flags],check=True)
    base=['quickshell','ipc','--path',str(home/'qml'),'call','runtjik']
    ipc=lambda *args:call(base+list(args))
    status=lambda:json.loads(ipc('status'))
    with (temp/'launcher.log').open('w') as log:
        process=subprocess.Popen(['quickshell','--path',str(home/'qml'),'--no-duplicate'],stdout=log,stderr=log)
        ptr=None
        try:
            wait(lambda:status()['ready'] and status()['geometry']['width']>0)
            assert status()['results']==100 and status()['total']==262,status()
            monitor=next(m for m in json.loads(call(['hyprctl','-j','monitors'])) if m['name']==status()['monitor'])
            ptr=subprocess.Popen([str(pointer),monitor['name']],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
            def send(line):
                ptr.stdin.write(line+'\n');ptr.stdin.flush();assert ptr.stdout.readline().strip()=='ok'
            g=status()['geometry']
            x,y=g['x']+g['width']/2,g['y']+g['height']-80
            send('m '+str(round((x-monitor['x'])/(monitor['width']/monitor['scale'])*10000))+' '+str(round((y-monitor['y'])/(monitor['height']/monitor['scale'])*10000)))
            for _ in range(20):
                send('s 900');time.sleep(.15)
                if status()['results']==262 and not status()['hasMore']:break
            wait(lambda:status()['results']==262 and not status()['hasMore'])
            send('s 2000');time.sleep(.3)
            assert status()['selected']==0,'Wheel paging changed keyboard selection'
            ipc('capture',str(temp/'paged-tail.png'));time.sleep(.3)
            print('PASS: real wheel scrolling loads 100 → 200 → all 262 entries without changing selection',flush=True)
            tail=json.loads(ipc('testState'))['scrollY']
            time.sleep(5.3)
            wait(lambda:status()['results']==262)
            assert abs(json.loads(ipc('testState'))['scrollY']-tail)<1,'Refresh lost scroll position'
            ipc('test','Entry');wait(lambda:json.loads(ipc('testState'))['resultQuery']=='Entry')
            subprocess.run(['wtype']+['-k','Down']*20,check=True)
            wait(lambda:status()['selected']==100 and status()['results']>=200)
            subprocess.run(['wtype']+['-k','Down']*20,check=True)
            wait(lambda:status()['selected']==200 and status()['results']==260)
            print('PASS: index refresh retains scroll position and keyboard paging reaches item 200',flush=True)
            ipc('test','ChatGPT');wait(lambda:status()['results']==1)
            subprocess.run(['wtype','-M','ctrl','-k','l','-m','ctrl'],check=True);time.sleep(.3)
            ipc('capture',str(temp/'list-row.png'));wait(lambda:(temp/'list-row.png').exists())
            from PIL import Image
            image=Image.open(temp/'list-row.png').convert('RGB')
            accent=(255,83,166)
            assert image.getpixel((40,158))==accent,'Running dot is not in its left gutter'
            assert image.getpixel((28,158))==image.getpixel((691,158)),'Selected row margins are asymmetric'
            print('PASS: list dot is before the icon; selected row has equal outer margins',flush=True)
            ipc('test','/');wait(lambda:status()['results']==9)
            ipc('capture',str(temp/'aliases.png'));time.sleep(.2)
            ipc('test','! fish');wait(lambda:status()['results']==1)
            subprocess.run(['wtype','-M','ctrl','-k','l','-m','ctrl'],check=True);time.sleep(.3)
            ipc('capture',str(temp/'fish.png'));time.sleep(.2)
            assert status()['view']=='grid'
            ipc('test','!dnf');wait(lambda:json.loads(ipc('testState'))['resultQuery']=='!dnf')
            assert json.loads(ipc('testState'))['rows'][0]['id']=='commands:dnf'
            ipc('capture',str(temp/'dnf.png'));time.sleep(.2)
            print('PASS: ! and /run aliases, deduplicated command menu, Fish and DNF render',flush=True)
        finally:
            if ptr:
                ptr.stdin.close();ptr.wait(timeout=5)
            process.terminate();process.wait(timeout=5)
            print('Progressive UI evidence:',temp,flush=True)
