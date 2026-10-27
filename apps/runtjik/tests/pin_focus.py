"""Verify that pinned Runtjik yields and regains keyboard focus on real clicks."""
import json
import os
from pathlib import Path
import shlex
import subprocess
import sys
import time

app = Path(__file__).resolve().parents[1]
sys.path.insert(0, os.environ.get('RUNTJIK_TEST_BARTJIK', str(app.parent/'bartjik')))
from demo_cycle import private_compositor


def call(argv):
    return subprocess.check_output(argv, text=True, stderr=subprocess.DEVNULL)


def wait(check):
    for _ in range(160):
        try:
            result = check()
            if result:
                return result
        except (ValueError, subprocess.CalledProcessError):
            pass
        time.sleep(.05)
    raise AssertionError('Timed out waiting for pinned focus')


with private_compositor():
    tmp = Path(os.environ['XDG_RUNTIME_DIR']).parent
    os.environ['XDG_STATE_HOME'] = str(tmp/'state')
    config = tmp/'runtjik.conf'
    config.write_text('[index]\nrefresh_seconds=1\n')
    os.environ.update(RUNTJIK_HOME=str(app), RUNTJIK_DEMO='1', RUNTJIK_CONFIG=str(config))
    monitors = wait(lambda: json.loads(call(['hyprctl', '-j', 'monitors'])))
    time.sleep(2)
    call(['hyprctl','eval','hl.config({misc={layers_hog_keyboard_focus=true}})'])
    pointer = tmp/'pointer'
    bartjik = Path(os.environ.get('RUNTJIK_TEST_BARTJIK', str(app.parent/'bartjik')))
    subprocess.run(['wayland-scanner','client-header',str(bartjik/'demo_pointer.xml'),str(tmp/'pointer.h')],check=True)
    subprocess.run(['wayland-scanner','private-code',str(bartjik/'demo_pointer.xml'),str(tmp/'protocol.c')],check=True)
    source = Path(os.environ.get('RUNTJIK_TEST_POINTER', str(app/'tests/pointer.c')))
    flags = shlex.split(call(['pkg-config','--cflags','--libs','wayland-client']))
    subprocess.run(['cc',str(source),str(tmp/'protocol.c'),'-I'+str(tmp),'-o',str(pointer),*flags],check=True)
    receiver = tmp/'receiver'
    receiver.mkdir()
    (receiver/'shell.qml').write_text('''import QtQuick
import Quickshell
import Quickshell.Io
ShellRoot {
    IpcHandler {target:"receiver";function text():string{return editor.text;}}
    FloatingWindow {
        visible:true;title:"Pinned focus receiver";color:"#123456"
        implicitWidth:1000;implicitHeight:680
        TextInput {id:editor;anchors.fill:parent;focus:true;color:"white"}
    }
}''')
    processes=[]
    try:
        receiver_process=subprocess.Popen(['quickshell','--path',str(receiver)],stdout=open(tmp/'receiver.log','w'),stderr=subprocess.STDOUT)
        processes.append(receiver_process)
        clients=lambda:json.loads(call(['hyprctl','-j','clients']))
        target=wait(lambda:next((c for c in clients() if c['title']=='Pinned focus receiver'),None))
        monitor=next(m for m in monitors if m['id']==target['monitor'])
        ptr=subprocess.Popen([str(pointer),monitor['name']],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
        processes.append(ptr)
        def send(command):
            ptr.stdin.write(command+'\n');ptr.stdin.flush()
            assert ptr.stdout.readline().strip()=='ok'
        def move(x,y):
            send('m '+str(round((x-monitor['x'])/(monitor['width']/monitor['scale'])*10000))+' '+str(round((y-monitor['y'])/(monitor['height']/monitor['scale'])*10000)))
        def click(x,y):
            move(x,y);time.sleep(.1);send('b 1');send('b 0');time.sleep(.3)
        shell=subprocess.Popen(['quickshell','--path',str(app/'qml'),'--no-duplicate'],stdout=open(tmp/'runtjik.log','w'),stderr=subprocess.STDOUT)
        processes.append(shell)
        ipc=['quickshell','ipc','--path',str(app/'qml'),'call','runtjik']
        status=lambda:json.loads(call(ipc+['status']))
        wait(lambda:status()['ready'])
        call(ipc+['test','/pin']);time.sleep(.2)
        subprocess.run(['wtype','-k','Return'],check=True)
        wait(lambda:status()['pinned']);time.sleep(.3)
        call(ipc+['capture',str(tmp/'pinned.png')]);time.sleep(.2)
        g=status()['geometry']
        candidates=[(target['at'][0]+20,target['at'][1]+20),
                    (target['at'][0]+target['size'][0]-20,target['at'][1]+20)]
        outside=next((x,y) for x,y in candidates if not(g['x']<=x<g['x']+g['width'] and g['y']<=y<g['y']+g['height']))
        click(*outside)
        subprocess.run(['wtype','externalFirst'],check=True)
        receiver_text=lambda:call(['quickshell','ipc','--path',str(receiver),'call','receiver','text'])
        wait(lambda:receiver_text().strip()=='externalFirst')
        assert status()['pinned'] and not status()['keyboardOwned']
        time.sleep(1.5)
        subprocess.run(['wtype','AfterRefresh'],check=True)
        wait(lambda:receiver_text().strip()=='externalFirstAfterRefresh')
        print('PASS: outside app receives typing while pinned, including after index refresh',flush=True)
        click(g['x']+180,g['y']+42)
        subprocess.run(['wtype','Firefox'],check=True)
        wait(lambda:status()['keyboardOwned'] and status()['results']==1)
        assert receiver_text().strip()=='externalFirstAfterRefresh'
        print('PASS: clicking pinned launcher restores search input',flush=True)
        call(['hyprctl','eval','hl.dispatch(hl.dsp.focus({window="address:'+target['address']+'"}))'])
        subprocess.run(['wtype','KeyboardSwitch'],check=True)
        wait(lambda:receiver_text().strip()=='externalFirstAfterRefreshKeyboardSwitch')
        time.sleep(1.5)
        subprocess.run(['wtype','AfterKeyboardRefresh'],check=True)
        wait(lambda:receiver_text().strip()=='externalFirstAfterRefreshKeyboardSwitchAfterKeyboardRefresh')
        assert status()['pinned']
        print('PASS: switching focus to another window also permits typing across refreshes',flush=True)
        click(g['x']+180,g['y']+42)
        call(ipc+['test','/pin']);time.sleep(.2)
        subprocess.run(['wtype','-k','Return'],check=True)
        wait(lambda:not status()['pinned']);time.sleep(.2)
        click(*outside)
        shell.wait(timeout=5)
        print('PASS: unpinned outside click still dismisses normally',flush=True)
    finally:
        for process in reversed(processes):
            if process.poll() is None:process.terminate()
            process.wait(timeout=5)
        print('Pinned focus fixture:',tmp,flush=True)
