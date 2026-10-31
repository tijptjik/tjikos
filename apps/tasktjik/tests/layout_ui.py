"""Measured column balancing and clickable tag filtering in an offscreen window."""
import json
import os
import re
from pathlib import Path
import shutil
import subprocess
import time
import tempfile

APP = Path(__file__).resolve().parents[1]
root = Path(tempfile.mkdtemp(prefix='tasktjik-layout-'))
(root / 'runtime').mkdir(mode=0o700)
(root / 'config/tasktjik').mkdir(parents=True)
(root / 'config/tasktjik/tasktjik.conf').write_text(
    '[tasktjik]\nauthority=""\ndirectory="' + str(root / 'tasks') + '"\n')
home = root / 'app'
shutil.copytree(APP / 'qml', home / 'qml')
shutil.copy2(APP / 'target/release/tasktjik-api', home / 'tasktjik-api')
env = {**os.environ, 'TASKTJIK_HOME': str(home), 'QT_QPA_PLATFORM': 'offscreen',
       'QT_QUICK_BACKEND': 'software', 'XDG_RUNTIME_DIR': str(root / 'runtime'),
       'XDG_CONFIG_HOME': str(root / 'config'), 'XDG_DATA_HOME': str(root / 'data'),
       'XDG_STATE_HOME': str(root / 'state'), 'XDG_CACHE_HOME': str(root / 'cache')}

def request(value):
    return json.loads(subprocess.check_output([str(home / 'tasktjik-api'), 'request'],
        input=json.dumps(value), text=True, env=env))

moving = request({'action': 'create', 'fields': {'title': 'Move this synthetic task', 'group': 'Work', 'tags':['work','deep work']}})
anchor = request({'action': 'create', 'fields': {'title': 'Target synthetic task', 'group': 'Tech', 'tags':['workshop']}})
request({'action': 'create', 'fields': {'title': 'Keep this task', 'group': 'Work'}})
for group,count in [('Work',10),('Tech',7),('Socials',7),('General',6),('Domestic',5)]:
    for i in range(count):request({'action':'create','fields':{'title':group+' fixture '+str(i),'group':group}})
component = home / 'qml/TodoApp.qml'
component.write_text(component.read_text().replace("text:'#'+modelData;textFormat:Text.PlainText","objectName:'tag-'+modelData; text:'#'+modelData;textFormat:Text.PlainText"))
shell = home / 'qml/shell.qml'
s = shell.read_text().replace('implicitWidth: 860','implicitWidth: 1400').replace('import QtQuick\n', 'import QtQuick\nimport QtTest\n')
s = re.sub(r'view.stateData = JSON.parse\((text|output)\);', lambda m: "const state=JSON.parse("+m[1]+");state.groups=['Work','Tech','Socials','General','Domestic'];view.stateData=state;", s)
s = s.replace('    Component.onCompleted:', '''    TestCase {id:pointer;name:"LayoutInput";when:false}
    IpcHandler {
        target:"layout-test"
        function state():string {return JSON.stringify({filter:view.filter,count:view.visibleCount(),columns:view.dropGroups.map(g=>({group:g.modelData,x:g.mapToItem(view,0,0).x,height:g.height})),tasks:view.stateData.tasks,drag:!!view.dragTask,pending:view.pendingMove,group:view.dropGroup,before:view.dropBefore,error:view.errorMessage});}
        function tagPoint(tag:string):string {const label=pointer.findChild(view,'tag-'+tag);const p=label.mapToItem(view,3,3);return JSON.stringify({x:p.x,y:p.y});}
        function filter(text:string):void {view.filter=text;}
        function press(x:real,y:real):void {pointer.mousePress(view,x,y,Qt.LeftButton);}
        function release(x:real,y:real):void {pointer.mouseRelease(view,x,y,Qt.LeftButton);}
        function capture(path:string):void {view.grabToImage(image=>image.saveToFile(path));}
    }
    Component.onCompleted:''')
shell.write_text(s)

def ipc(method, *args):
    return subprocess.check_output(['quickshell', 'ipc', '--path', str(home / 'qml'),
        'call', 'layout-test', method, *map(str,args)], text=True, env=env, stderr=subprocess.DEVNULL)

def wait(fn, timeout=12):
    end=time.monotonic()+timeout
    while time.monotonic()<end:
        try:
            value=fn()
            if value:return value
        except (subprocess.CalledProcessError, ValueError):
            pass
        time.sleep(.05)
    raise AssertionError('Timed out; see '+str(root / 'qml.log'))

with (root / 'qml.log').open('w') as log:
    process=subprocess.Popen(['quickshell','--path',str(home / 'qml')],env=env,stdout=log,stderr=log)
    try:
        wait(lambda:len(json.loads(ipc('state'))['tasks'])==38)
        wait(lambda:len(json.loads(ipc('state'))['columns'])==5)
        time.sleep(.5)
        state=json.loads(ipc('state'))
        columns=state['columns'];loads={}
        for g in columns:loads[g['x']]=loads.get(g['x'],0)+g['height']+28
        assert len(loads)==3, columns
        old=[0,0,0]
        for i,name in enumerate(['Work','Tech','Socials','General','Domestic']):
            old[i%3]+=next(g['height'] for g in columns if g['group']==name)+28
        assert max(loads.values())<max(old)-50,(loads,old)
        ipc('capture',root/'balanced.png');wait(lambda:(root/'balanced.png').is_file())
        tag=json.loads(ipc('tagPoint','work'))
        ipc('press',tag['x'],tag['y']);ipc('release',tag['x'],tag['y'])
        wait(lambda:json.loads(ipc('state'))['filter']=='tag:work')
        assert json.loads(ipc('state'))['count']==1
        tag=json.loads(ipc('tagPoint','deep work'))
        ipc('press',tag['x'],tag['y']);ipc('release',tag['x'],tag['y'])
        wait(lambda:json.loads(ipc('state'))['filter']=='tag:"deep work"')
        assert json.loads(ipc('state'))['count']==1
        ipc('filter','tag:workshop');wait(lambda:json.loads(ipc('state'))['count']==1)
        ipc('filter','tag:work tag:workshop');wait(lambda:json.loads(ipc('state'))['count']==0)
        assert 'TypeError' not in (root/'qml.log').read_text()
        print('PASS measured column balancing and actual tag clicks with exact tag matching:',root)
    finally:
        process.terminate();process.wait(timeout=5)
