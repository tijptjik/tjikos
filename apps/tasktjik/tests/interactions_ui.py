"""Synthetic task navigation, deletion, progress and project drop regressions."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time

APP = Path(__file__).resolve().parents[1]
root = Path(tempfile.mkdtemp(prefix='tasktjik-project-sort-'))
qml = root / 'qml'
shutil.copytree(APP / 'qml', qml)
(root / 'runtime').mkdir(mode=0o700)
(root / 'config/tasktjik').mkdir(parents=True)
env = {**os.environ, 'QT_QPA_PLATFORM': 'offscreen', 'QT_QUICK_BACKEND': 'software',
       'QT_QUICK_CONTROLS_STYLE': 'Basic', 'XDG_RUNTIME_DIR': str(root / 'runtime'),
       'XDG_CONFIG_HOME': str(root / 'config'), 'XDG_DATA_HOME': str(root / 'data'),
       'XDG_STATE_HOME': str(root / 'state'), 'XDG_CACHE_HOME': str(root / 'cache')}

def task(id, **values):
    return dict(id=id, title=id, kind='todo', group='All', due='', period='once',
                done=False, status='ready', tags=[], subtasks=[], **values)

tasks = [task('middle'), task('project'), task('empty'), task('member'), task('done'), task('cancelled')]
tasks.append(task('zulu'))
tasks[0]['due'] = '2099-02-20'
for index in (1, 2):
    tasks[index].update(kind='project', onlyInProject=True)
tasks[3].update(project='project', due='2099-02-10', group='Hidden')
tasks[4].update(project='project', due='2099-01-01', done=True, status='done')
tasks[5].update(project='project', due='2099-01-02', status='cancelled')
state = dict(tasks=tasks, groups=['All', 'Hidden'], today='2099-02-01')
(qml / 'shell.qml').write_text('''import QtQuick
import QtQuick.Controls
import QtTest
import Quickshell
import Quickshell.Io
ShellRoot {
    TestCase {id:input;when:false}
    FloatingWindow {
        implicitWidth:900;implicitHeight:700;color:'#26233a'
        TodoApp {id:view;property var lastRequest:({});onRequest:value=>lastRequest=value;anchors.fill:parent;view:'all';stateData:''' + json.dumps(state) + '''
            theme:({background:'#26233a',surface:'#39354b',text:'#f5f5f7',muted:'#aaa5ba',accent:'#a8c7a0',border:'#555169'})
        }
    }
    IpcHandler {
        target:'sort-test'
        function exercise():string {
            view.view='today';view.enterProject(view.stateData.tasks[1]);view.focusList();
            input.keyClick(Qt.Key_Left,Qt.ControlModifier);
            if(view.projectId || view.view!=='today')throw Error('Ctrl+Left from project');
            view.view='week';view.view='projects';view.focusList();
            input.keyClick(Qt.Key_Left,Qt.ControlModifier);
            if(view.view!=='week')throw Error('Ctrl+Left from Projects overview');
            view.view='all';view.sortOrder=4;view.selectedId='middle';view.focusList();
            view.reorderTask(1);
            if(!view.pendingMove || view.moveOperation.before_id!=='')throw Error('manual reorder '+JSON.stringify(view.moveOperation));
            view.clearMove();
            input.wait(200);
            const group=view.dropGroups.find(g=>g.modelData==='All');
            let card=null;for(let i=0;i<group.rows.count;i++){const row=group.rows.itemAt(i);if(row.modelData.id==='project')card=row;}
            const point=card.mapToItem(view,card.width/2,card.height/2);
            view.startDrag(view.stateData.tasks[0],null,100,40,point);
            if(view.dropProject!=='project')throw Error('project target');
            view.finishDrag();
            if(view.lastRequest.action!=='edit' || view.lastRequest.fields.project!=='project')throw Error('project edit request');
            if(view.moveOperation.project!=='project' || view.listState.tasks.find(t=>t.id==='middle').project!=='project')throw Error('optimistic membership');
            view.clearMove();view.selectedId='middle';view.focusList();
            input.mouseClick(view,view.width-2,view.height-2);input.wait(30);
            if(view.taskSelectionActive)throw Error('outside click did not deselect');
            view.focusVertical(1,0);
            if(view.selectedId!=='middle' || !view.taskSelectionActive)throw Error('restore selection');
            view.focusVertical(1,Qt.ControlModifier);
            if(view.selectedId==='middle' || view.selectedSubtaskId)throw Error('ctrl navigation '+view.selectedId);
            view.selectedId='middle';view.focusList();view.doubleDInterval=300;view.pressD();view.pressD();
            if(!view.pendingPurge || view.pendingPurge.id!=='middle' || view.completionQueue.length)throw Error('double d target');
            view.pendingPurge=null;view.queuedPurges=[];view.selectedId='member';view.focusList();view.pressD();input.wait(400);
            if(!view.completionQueue.some(op=>op.id==='member'))throw Error('single d completion');
            view.completionQueue=[];
            view.view='today';
            const next=JSON.parse(JSON.stringify(view.stateData));next.today='2099-02-10';next.tasks[4].completedAt='2099-02-10T12:00:00Z';view.stateData=next;
            const progress=view.projectProgress(next.tasks[1]);
            if(progress.total!==2 || progress.done!==1)throw Error('progress '+JSON.stringify(progress));
            view.view='all';view.projectSort='last';
            if(view.effectiveDue(next.tasks[1])!=='2099-02-10')throw Error('project date');
            return 'PASS';
        }
        function queuedMovesAndDrawer():string {
            const make=(id,key)=>({id:id,title:'A long task title with room for focus and drag padding '+id,kind:'todo',group:'All',manualOrder:key,revision:1,period:'once',status:'ready',done:false,subtasks:[]});
            view.stateData={today:'2099-02-01',groups:['All','Empty A','Empty B'],tasks:[make('a','100'),make('b','200'),make('c','300')]};
            view.view='all';view.sortOrder=4;view.selectedId='a';view.focusList();view.requestBusy=true;input.wait(100);
            view.reorderTask(1);view.reorderTask(1);
            if(view.tasks('All').map(t=>t.id).join(',')!=='b,c,a' || view.queuedMoves.length!==1)throw Error('rapid optimistic moves');
            view.requestBusy=false;view.sendMove();
            let next=JSON.parse(JSON.stringify(view.stateData));next.tasks[0].revision=2;next.tasks[0].manualOrder='250';next.tasks[0].events=[{action:'move'}];view.stateData=next;input.wait(60);
            if(view.lastRequest.revision!==2 || view.tasks('All').map(t=>t.id).join(',')!=='b,c,a')throw Error('queue acknowledgment/revision');
            next=JSON.parse(JSON.stringify(view.stateData));next.tasks[0].revision=3;next.tasks[0].manualOrder='400';view.stateData=next;input.wait(30);
            if(view.pendingMove)throw Error('move queue not drained');
            view.requestBusy=true;view.reorderTask(-1);view.reorderTask(-1);view.errorMessage='Synthetic move failure';input.wait(20);
            // Unsent operations are explicitly rejected as a transport failure.
            if(view.pendingMove)view.rejectMove();
            if(view.tasks('All').map(t=>t.id).join(',')!=='b,c,a' || view.queuedMoves.length)throw Error('move rollback');
            view.errorMessage='';view.requestBusy=false;input.wait(100);
            const card=input.findChild(view,'task-a'),title=input.findChild(view,'task-title-a');
            view.taskSelectionActive=false;input.wait(30);const size=[title.width,title.height,card.height];
            view.taskSelectionActive=true;input.wait(30);
            if(size.join(',')!==[title.width,title.height,card.height].join(','))throw Error('focus changed title geometry');
            const before=view.visualRows().map(r=>[r.id,r.x,r.y]);
            view.startDrag(view.stateData.tasks[0],null,100,40,card.mapToItem(view,60,10));input.wait(200);
            if(JSON.stringify(before)!==JSON.stringify(view.visualRows().map(r=>[r.id,r.x,r.y])))throw Error('empty groups shifted layout');
            if(view.dropGroups.some(g=>g.visible && g.rows.count===0))throw Error('empty group in task layout');
            const target=input.findChild(view,'drag-group-Empty B');
            view.updateDrag(target.mapToItem(view,target.width/2,target.height/2));
            if(view.dropGroup!=='Empty B')throw Error('drawer hit target');
            view.finishDrag();
            if(view.listState.tasks.find(t=>t.id==='a').group!=='Empty B')throw Error('drawer optimistic move');
            view.clearMove();return 'PASS';
        }
        function domains():string {
            view.clearMove();view.view='all';view.projectId='';view.manualDomainOrderJson='[]';
            const make=(id,group,due)=>({id:id,title:id,group:group,due:due,kind:'todo',period:'once',status:'ready',done:false,subtasks:[]});
            view.stateData={today:'2099-02-01',groups:['Charlie','Alpha','Bravo'],tasks:[make('c','Charlie','2099-02-03'),make('a','Alpha','2099-02-02'),make('b','Bravo','2099-02-01')]};
            view.domainSort='alphabetical';input.wait(100);
            const order=()=>view.dropGroups.filter(g=>g.visible).sort((a,b)=>a.index-b.index).map(g=>g.modelData).join(',');
            const columns=()=>view.dropGroups.filter(g=>g.visible).sort((a,b)=>a.index-b.index).map(g=>[g.modelData,g.column]);
            if(order()!=='Alpha,Bravo,Charlie')throw Error('alphabetical '+order());
            view.domainSort='due';input.wait(100);
            if(order()!=='Bravo,Alpha,Charlie')throw Error('due '+order());
            const before=JSON.stringify(columns());
            let next=JSON.parse(JSON.stringify(view.stateData));next.tasks[0].due='2099-01-01';next.tasks.push(make('new','Alpha','2099-01-01'));view.stateData=next;input.wait(100);
            if(order()!=='Bravo,Alpha,Charlie' || JSON.stringify(columns())!==before)throw Error('mutation moved domains');
            next=JSON.parse(JSON.stringify(view.stateData));next.tasks=next.tasks.filter(t=>t.id!=='new');view.stateData=next;input.wait(50);
            if(JSON.stringify(columns())!==before)throw Error('delete moved domains');
            input.mouseClick(input.findChild(view,'domain-collapse-Alpha'));input.wait(50);
            if(view.visualRows().some(r=>r.id==='a') || !view.collapsedDomains.Alpha)throw Error('collapse navigation');
            if(JSON.stringify(columns())!==before)throw Error('collapse moved columns');
            input.mouseClick(input.findChild(view,'domain-collapse-Alpha'));input.wait(50);
            if(!view.visualRows().some(r=>r.id==='a'))throw Error('expand navigation');
            view.view='today';input.wait(30);view.view='all';input.wait(100);
            if(order()!=='Charlie,Bravo,Alpha')throw Error('view did not refresh due order '+order());
            view.domainSort='manual';input.wait(50);view.moveDomain('Charlie',1);input.wait(50);
            if(order()!=='Alpha,Charlie,Bravo')throw Error('manual order '+order());
            return 'PASS';
        }
        function preferences():string {return JSON.stringify({sort:view.domainSort,manual:view.manualDomainOrder});}
        function captureDomains(path:string):void {view.grabToImage(image=>image.saveToFile(path));}
        function state():string {return JSON.stringify({mode:view.projectSort,rows:view.tasks('All').map(t=>t.id),due:view.effectiveDue(view.stateData.tasks[1])});}
        function choose(index:int):void {const settings=input.findChild(view,'task-settings');settings.open();const choice=input.findChild(settings,'project-sort-choice');choice.currentIndex=index;choice.activated(index);settings.close();}
        function sort(index:int):void {view.sortOrder=index;}
        function due(value:string):void {const next=JSON.parse(JSON.stringify(view.stateData));next.tasks[3].due=value;view.stateData=next;}
        function capture(path:string):void {const settings=input.findChild(view,'task-settings');settings.open();input.wait(150);settings.panelItem.grabToImage(image=>image.saveToFile(path));}
        function quit():void {Qt.quit();}
    }
}
''')

def ipc(method, *args):
    return subprocess.check_output(['quickshell', 'ipc', '--path', str(qml), 'call',
                                    'sort-test', method, *map(str, args)], env=env,
                                   text=True, stderr=subprocess.DEVNULL)

def state_now():
    return json.loads(ipc('state'))

def wait(fn):
    deadline = time.monotonic() + 12
    while time.monotonic() < deadline:
        try:
            result = fn()
            if result:
                return result
        except (subprocess.CalledProcessError, ValueError):
            pass
        time.sleep(.05)
    raise AssertionError('Timed out: ' + str(root))

with (root / 'qml.log').open('w') as log:
    process = subprocess.Popen(['quickshell', '--path', str(qml)], env=env, stdout=log, stderr=log)
    try:
        wait(state_now)
        assert ipc('exercise').strip() == 'PASS'
        assert ipc('queuedMovesAndDrawer').strip() == 'PASS'
        assert ipc('domains').strip() == 'PASS'
        ipc('captureDomains', root / 'domains.png')
        wait(lambda: (root / 'domains.png').exists())
        ipc('quit');process.wait(timeout=5)
        process = subprocess.Popen(['quickshell', '--path', str(qml)], env=env, stdout=log, stderr=log)
        wait(state_now)
        assert json.loads(ipc('preferences')) == {'sort': 'manual', 'manual': ['Alpha','Charlie','Bravo']}
        errors = (root / 'qml.log').read_text()
        assert not any(word in errors for word in ['TypeError', 'ReferenceError', 'Binding loop', 'Cannot assign']), errors
        print('PASS navigation restore, Ctrl navigation, pinned double-D delete, single-D completion, temporal progress and project dates:', root)
    finally:
        if process.poll() is None:
            process.terminate();process.wait(timeout=5)
