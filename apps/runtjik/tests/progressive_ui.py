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
from private_compositor import private_compositor


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
    shell=home/'qml/shell.qml'
    shell.write_text(shell.read_text().replace('    id: root', '    id: root\n    property string testActivatedId: ""')
        .replace('request.prepare=switchWindow', 'root.testActivatedId=request.id;request.prepare=switchWindow')
        .replace('function testState(): string {', 'function testIndex(x: real,y: real): int {'
        'const p=items.contentItem.mapFromItem(frame,x,y);return items.indexAt(p.x,p.y);}\n        function testState(): string {')
        .replace('scrollY:items.contentY,rows:root.results',
        'scrollY:items.contentY,pointer:{x:selectionPointer.point.position.x,y:selectionPointer.point.position.y,owned:root.pointerSelection},activatedId:root.testActivatedId,cells:Array.from({length:Math.min(15,items.count)},(_,i)=>{'
        'const cell=items.itemAtIndex(i);if(!cell)return null;const p=cell.mapToItem(frame,cell.width/2,cell.height/2);'
        'return {x:p.x,y:p.y};}),rows:root.results'))
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
    subprocess.run(['wayland-scanner','client-header',str(Path(__file__).with_name('virtual_pointer.xml')),str(temp/'pointer.h')],check=True)
    subprocess.run(['wayland-scanner','private-code',str(Path(__file__).with_name('virtual_pointer.xml')),str(temp/'protocol.c')],check=True)
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
            def move_to(index,dx=0):
                cell=json.loads(ipc('testState'))['cells'][index]
                x,y=g['x']+cell['x']+dx,g['y']+cell['y']
                send('m '+str(round((x-monitor['x'])/(monitor['width']/monitor['scale'])*10000))+' '+str(round((y-monitor['y'])/(monitor['height']/monitor['scale'])*10000)))
                return cell['x']+dx,cell['y']
            def scroll_tracking():
                px,py=move_to(2,3)
                original=status()['selected']
                under_pointer=lambda:int(ipc('testIndex',str(px),str(py)))
                send('s 180')
                try:
                    wait(lambda:under_pointer()!=original and status()['selected']==under_pointer())
                except AssertionError:
                    state=json.loads(ipc('testState'))
                    raise AssertionError({'selected':status()['selected'],'underPointer':under_pointer(),
                        'original':original,'scrollY':state['scrollY'],'pointer':state['pointer'],'target':(px,py)})
                subprocess.run(['wtype','-k','Up'],check=True)
                keyboard=status()['selected']
                time.sleep(.3)
                assert status()['selected']==keyboard,'Pointer stole keyboard selection'
                send('s 180')
                wait(lambda:status()['selected']==under_pointer())
                # Movement must still track after coordinates have scrolled far
                # beyond the viewport's height.
                px+=2;py+=1
                send('r 2 1')
                wait(lambda:status()['selected']==under_pointer())
                send('s -360')
                wait(lambda:status()['selected']==under_pointer())
            subprocess.run(['wtype','-k','Tab'],check=True)
            wait(lambda:status()['selected']==1)
            move_to(6)
            wait(lambda:status()['selected']==6)
            subprocess.run(['wtype','-k','Down'],check=True)
            wait(lambda:status()['selected']==11)
            time.sleep(.3)
            assert status()['selected']==11,'Stationary pointer overrode keyboard navigation'
            move_to(6,2)
            wait(lambda:status()['selected']==6)
            chosen=json.loads(ipc('testState'))['rows'][6]['id']
            subprocess.run(['wtype','-k','Return'],check=True)
            wait(lambda:json.loads(ipc('testState'))['activatedId']==chosen)
            subprocess.run(['wtype','-M','ctrl','-k','l','-m','ctrl'],check=True)
            wait(lambda:status()['view']=='list')
            move_to(2)
            wait(lambda:status()['selected']==2)
            subprocess.run(['wtype','-k','Down'],check=True)
            wait(lambda:status()['selected']==3)
            move_to(2,2)
            wait(lambda:status()['selected']==2)
            scroll_tracking()
            subprocess.run(['wtype','-M','ctrl','-k','l','-m','ctrl'],check=True)
            wait(lambda:status()['view']=='grid')
            scroll_tracking()
            # Leave the pointer outside the results before resetting keyboard selection.
            send('m 100 100')
            ipc('test','Entry 259');wait(lambda:status()['results']==1)
            ipc('test','');wait(lambda:status()['results']==100)
            print('PASS: grid/list pointer movement and keyboard share selection; same-cell movement takes over; Enter uses pointer selection',flush=True)
            # The final grid row is incomplete; keep the pointer in its first column.
            x,y=g['x']+60,g['y']+g['height']-80
            send('m '+str(round((x-monitor['x'])/(monitor['width']/monitor['scale'])*10000))+' '+str(round((y-monitor['y'])/(monitor['height']/monitor['scale'])*10000)))
            for _ in range(20):
                send('s 900');time.sleep(.15)
                if status()['results']==262 and not status()['hasMore']:break
            wait(lambda:status()['results']==262 and not status()['hasMore'])
            send('s 2000');time.sleep(.3)
            wait(lambda:status()['selected']==int(ipc('testIndex',str(x-g['x']),str(y-g['y']))))
            ipc('capture',str(temp/'paged-tail.png'));time.sleep(.3)
            print('PASS: mouse scrolling tracks the pointer in grid/list views, resumes after keyboard use, and survives paging to 262 entries',flush=True)
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
