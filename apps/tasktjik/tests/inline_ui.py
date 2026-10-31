"""Inline snoozing, date help, and spatial keyboard navigation in an offscreen window."""
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

request({'action':'create','fields':{'title':'Inbox fixture','group':'All'}})
moving = request({'action': 'create', 'fields': {'title': 'Move this synthetic task', 'group': 'Work', 'tags':['work','deep work']}})
anchor = request({'action': 'create', 'fields': {'title': 'Target synthetic task', 'group': 'Tech', 'tags':['workshop']}})
request({'action': 'create', 'fields': {'title': 'Keep this task', 'group': 'Work'}})
for group,count in [('Work',10),('Tech',7),('Socials',7),('General',6),('Domestic',5)]:
    for i in range(count):request({'action':'create','fields':{'title':group+' fixture '+str(i),'group':group}})
component = home / 'qml/TodoApp.qml'
picker=home/'qml/SnoozePicker.qml'
picker.write_text(picker.read_text().replace('id: picker', "id: picker;objectName:'snooze-picker'"))
component.write_text(component.read_text().replace('color:"transparent"', 'color:theme.background', 1).replace('id:dateHelp', "id:dateHelp;objectName:'dateHelp'").replace("text:'#'+modelData;textFormat:Text.PlainText","objectName:'tag-'+modelData; text:'#'+modelData;textFormat:Text.PlainText"))
shell = home / 'qml/shell.qml'
s = shell.read_text().replace('implicitWidth: 860','implicitWidth: 1400').replace('implicitHeight: 680','implicitHeight: 1000').replace('import QtQuick\n', 'import QtQuick\nimport QtQuick.Layouts\nimport QtTest\n')
s = re.sub(r'view.stateData = JSON.parse\((text|output)\);', lambda m: "const state=JSON.parse("+m[1]+");state.groups=['Work','Tech','Socials','General','Domestic','All'];view.stateData=state;", s)
s = s.replace('root.applyState(JSON.parse(output));', "const state=JSON.parse(output);state.groups=['Work','Tech','Socials','General','Domestic','All'];root.applyState(state);")
s = s.replace('    Component.onCompleted:' , '''    TestCase {id:pointer;name:"LayoutInput";when:false}
    IpcHandler {
        target:"layout-test"
        function state():string {return JSON.stringify({filter:view.filter,count:view.visibleCount(),columns:view.dropGroups.map(g=>({group:g.modelData,x:g.mapToItem(view,0,0).x,height:g.height})),tasks:view.stateData.tasks,drag:!!view.dragTask,pending:view.pendingMove,group:view.dropGroup,before:view.dropBefore,error:view.errorMessage});}
        function tagPoint(tag:string):string {const label=pointer.findChild(view,'tag-'+tag);const p=label.mapToItem(view,3,3);return JSON.stringify({x:p.x,y:p.y});}
        function inspect():string {
            const rows=[];
            for(const g of view.dropGroups)for(let i=0;i<g.rows.count;i++) {const r=g.rows.itemAt(i),p=r.mapToItem(view,0,0);rows.push({id:r.modelData.id,group:g.modelData,x:p.x,y:p.y,height:r.height});}
            return JSON.stringify({rows:rows,selected:view.selectedId,editor:view.editor,snooze:view.snoozeTaskId});
        }
        function later():void {for(const g of view.dropGroups)for(let i=0;i<g.rows.count;i++){const row=g.rows.itemAt(i);if(row.modelData.id===view.selectedId){const b=pointer.findChild(row,'snooze-Later');pointer.mouseClick(b,b.width/2,b.height/2);}}}
        function focused():string {for(const g of view.dropGroups)for(let i=0;i<g.rows.count;i++){const row=g.rows.itemAt(i);if(row.modelData.id===view.selectedId){const picker=pointer.findChild(row,'snooze-picker');const focused=picker.focusButtons().find(b=>b.activeFocus);return focused?focused.objectName || focused.Accessible.name:'';}}return '';}
        function pickerWidth(width:int):void {for(const g of view.dropGroups)for(let i=0;i<g.rows.count;i++){const row=g.rows.itemAt(i);if(row.modelData.id===view.selectedId)pointer.findChild(row,'snooze-picker').Layout.maximumWidth=width;}}
        function middleDate():void {for(const g of view.dropGroups)for(let i=0;i<g.rows.count;i++){const row=g.rows.itemAt(i);if(row.modelData.id===view.selectedId){const picker=pointer.findChild(row,'snooze-picker');picker.focusButtons().find(b=>b.dateValue && b.dateValue.getDay()===2 && b.dateValue.getDate()>=8 && b.dateValue.getDate()<=14).forceActiveFocus();}}}
        function calendarDay():int {for(const g of view.dropGroups)for(let i=0;i<g.rows.count;i++){const row=g.rows.itemAt(i);if(row.modelData.id===view.selectedId){const b=pointer.findChild(row,'snooze-picker').focusButtons().find(b=>b.activeFocus);return b && b.dateValue?b.dateValue.getDate():-1;}}return -1;}
        function backtab():void {pointer.keyClick(Qt.Key_Tab,Qt.ShiftModifier);}
        function help():void {pointer.findChild(view,'dateHelp').open();}
        function captureHelp(path:string):void {pointer.findChild(view,'dateHelp').contentItem.grabToImage(image=>image.saveToFile(path));}
        function helpState():bool {return pointer.findChild(view,'dateHelp').visible;}
        function select(id:string):void {view.selectedId=id;view.forceActiveFocus();}
        function key(code:int):void {pointer.keyClick(code);}
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
        wait(lambda:len(json.loads(ipc('state'))['tasks'])==39)
        time.sleep(.8)
        state=json.loads(ipc('inspect'));rows=state['rows']
        inbox=next(r for r in rows if r['group']=='All')
        assert inbox['x']==min(r['x'] for r in rows)
        assert inbox['y']==min(r['y'] for r in rows)
        origin=next(r for r in rows if r['x']==inbox['x'] and r['group']!='All')
        ipc('select',origin['id']);ipc('key',0x01000014)
        candidates=[r for r in rows if r['x']>origin['x']+1]
        x=min(r['x'] for r in candidates)
        expected=min((r for r in candidates if r['x']==x),key=lambda r:abs(r['y']-origin['y']))
        assert json.loads(ipc('inspect'))['selected']==expected['id']
        ipc('key',0x01000012)
        assert json.loads(ipc('inspect'))['selected']==origin['id']
        ipc('key',ord('S'));time.sleep(.35)
        opened=json.loads(ipc('inspect'))
        assert opened['editor']=='' and opened['snooze']==origin['id']
        row=next(r for r in opened['rows'] if r['id']==origin['id'])
        assert row['height']>origin['height']+60
        assert len(opened['rows'])==len(rows)
        assert ipc('focused').strip()=='snooze-Afternoon'
        for key, label in [(0x01000014,'Evening'),(0x01000015,'Weekend'),(0x01000015,'Later'),
                           (0x01000013,'Weekend'),(0x01000014,'Next Week'),(0x01000013,'Night'),
                           (0x01000014,'Night'),(0x01000012,'Evening'),(0x01000012,'Afternoon')]:
            ipc('key',key)
            assert ipc('focused').strip()=='snooze-'+label, (key, label, ipc('focused'))
            assert json.loads(ipc('inspect'))['selected']==origin['id']
        ipc('pickerWidth',260)
        time.sleep(.15)
        for key, label in [(0x01000014,'Evening'),(0x01000015,'Tomorrow'),(0x01000012,'Night'),
                           (0x01000015,'Weekend'),(0x01000014,'Next Week'),(0x01000015,'Later'),
                           (0x01000013,'Next Week')]:
            ipc('key',key)
            assert ipc('focused').strip()=='snooze-'+label, (key, label, ipc('focused'))
            assert json.loads(ipc('inspect'))['selected']==origin['id']
        for _ in range(5):ipc('backtab')
        assert ipc('focused').strip()=='snooze-Afternoon'
        ipc('pickerWidth',10000)
        time.sleep(.15)
        for label in ['Evening','Night','Tomorrow','Weekend','Next Week','Later','close','Afternoon']:
            ipc('key',0x01000001)
            assert ipc('focused').strip()=='snooze-'+label
            assert json.loads(ipc('inspect'))['selected']==origin['id']
        ipc('backtab')
        assert ipc('focused').strip()=='snooze-close'
        ipc('key',0x01000004)  # Return on × closes without changing the task.
        assert json.loads(ipc('inspect'))['snooze']==''
        assert not next(t for t in json.loads(ipc('state'))['tasks'] if t['id']==origin['id']).get('snoozedUntil')
        ipc('key',ord('S'))
        wait(lambda:ipc('focused').strip()=='snooze-Afternoon')
        time.sleep(.35)
        ipc('capture',root/'inline-snooze.png');wait(lambda:(root/'inline-snooze.png').is_file())
        for _ in range(6):ipc('key',0x01000001)
        assert ipc('focused').strip()=='snooze-Later'
        ipc('key',0x01000005)  # Keypad Enter opens the calendar.
        time.sleep(.3)
        assert ipc('focused').strip()=='Next month'
        ipc('key',0x01000012)
        assert ipc('focused').strip()=='Previous month'
        ipc('key',0x01000014)
        assert ipc('focused').strip()=='Next month'
        ipc('key',0x01000004)  # Use a future month so every test date is enabled.
        ipc('middleDate')
        day=int(ipc('calendarDay'))
        for key, expected_day in [(0x01000014,day+1),(0x01000012,day),(0x01000015,day+7),(0x01000013,day)]:
            ipc('key',key)
            assert int(ipc('calendarDay'))==expected_day
        ipc('key',0x01000001)
        assert '202' in ipc('focused'), 'Tab should reach an enabled calendar date'
        expanded=json.loads(ipc('inspect'))
        assert next(r for r in expanded['rows'] if r['id']==origin['id'])['height']>row['height']+120
        ipc('capture',root/'inline-calendar.png');wait(lambda:(root/'inline-calendar.png').is_file())
        ipc('select',origin['id']);ipc('key',0x01000000);time.sleep(.3)
        assert json.loads(ipc('inspect'))['snooze']==''
        ipc('key',ord('S'))
        wait(lambda:ipc('focused').strip()=='snooze-Afternoon')
        for _ in range(3):ipc('key',0x01000001)
        ipc('key',0x01000004)
        wait(lambda:any(t['id']==origin['id'] and t.get('snoozedUntil') for t in json.loads(ipc('state'))['tasks']))
        assert json.loads(ipc('inspect'))['snooze']==''
        ipc('help');wait(lambda:ipc('helpState').strip()=='true')
        time.sleep(.3);ipc('captureHelp',root/'date-help.png');wait(lambda:(root/'date-help.png').is_file())
        ipc('key',0x01000000);wait(lambda:ipc('helpState').strip()=='false')
        assert 'TypeError' not in (root/'qml.log').read_text()
        print('PASS spatial arrows in 2/3-column snooze grids and calendar, Tab/Shift+Tab, cancel, Return save and Escape:',root)
    finally:
        process.terminate();process.wait(timeout=5)
