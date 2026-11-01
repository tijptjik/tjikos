"""Real Wayland image pastes and rendered details against a synthetic SQLite authority."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time
from PIL import Image, ImageDraw

APP = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(APP.parent / 'runtjik/tests'))
from private_compositor import private_compositor, wait_for


with private_compositor(isolate_pointer=True):
    subprocess.run(['hyprctl', 'output', 'create', 'headless', 'task-images-test'], check=True, stdout=subprocess.DEVNULL)
    root = Path(os.environ['XDG_RUNTIME_DIR']).parent
    home = root / 'app'
    shutil.copytree(APP / 'qml', home / 'qml')
    shutil.copy2(APP / 'tasktjik-api', home / 'tasktjik-api')
    env = {**os.environ, 'TASKTJIK_HOME': str(home), 'XDG_STATE_HOME': str(root / 'state'),
           'QT_QUICK_CONTROLS_STYLE': 'Basic'}
    config = Path(env['XDG_CONFIG_HOME']) / 'tasktjik/tasktjik.conf'
    config.parent.mkdir(parents=True)
    config.write_text('[tasktjik]\nsocket=' + json.dumps(str(root / 'authority.sock')) + '\n')
    (root / 'server.toml').write_text(
        '[server]\nsocket=' + json.dumps(str(root / 'authority.sock'))
        + '\ndirectory=' + json.dumps(str(root / 'tasks'))
        + '\norigin="https://synthetic.ts.net"\nallowed_users=["synthetic@example.test"]\n'
        + '[server.local_users]\n' + json.dumps(str(os.getuid())) + '="synthetic@example.test"\n')
    component = home / 'qml/TodoApp.qml'
    gallery = home / 'qml/TaskImages.qml'
    gallery.write_text(gallery.read_text().replace('    id: panel', '    id: panel\n    readonly property real testScroll:scroll.contentItem.contentY'))
    component.write_text(component.read_text().replace('    id: app', '''    id: app
    function imageTestState() {
        const rows=visualRows();
        const rendered=[];
        function visit(item) {if(item.objectName && item.objectName.indexOf('task-image-')===0)rendered.push({name:item.objectName,status:item.status});for(const child of item.children || [])visit(child);}
        visit(imagePane);
        return {visible:imageDetails,left:imagesOnLeft,pane:{x:imagePane.x,y:imagePane.y,width:imagePane.width,height:imagePane.height},offset:imagePane.testScroll,rows:rows,rendered:rendered};
    }
'''))
    shell = home / 'qml/shell.qml'
    shell.write_text(shell.read_text().replace('import QtQuick\n', 'import QtQuick\nimport QtTest\n')
        .replace('implicitWidth: 860', 'implicitWidth: 1000')
        .replace('anchors.fill: parent; anchors.margins: 20', "width:960;height:640;anchors.centerIn:parent;background:Rectangle {color:'#26233a'}")
        .replace('    Component.onCompleted:', '''    TestCase {id:input;name:"ImageInput";when:false}
    IpcHandler {
        target:'images-test'
        function state():string {return JSON.stringify({busy:view.requestBusy,tasks:view.stateData.tasks,selected:view.selectedId,error:view.errorMessage,images:view.imageTestState()});}
        function select(id:string):void {view.selectedId=id;view.forceActiveFocus();}
        function paste():void {view.forceActiveFocus();input.keyClick(Qt.Key_V,Qt.ControlModifier);}
        function key(name:string):void {view.forceActiveFocus();input.keyClick(name==='right'?Qt.Key_Right:Qt.Key_Left);}
        function resize(width:int):void {view.width=width;}
        function refresh():void {root.request({action:'list'});}
        function scrollImages():void {const pane=view.imageTestState().pane;input.mouseWheel(view,pane.x+pane.width/2,pane.y+pane.height/2,0,-360,Qt.NoButton,Qt.NoModifier,0);}
        function clickImages():void {const pane=view.imageTestState().pane;input.mouseClick(view,pane.x+20,pane.y+38);}
        function capture(path:string):void {view.grabToImage(image=>image.saveToFile(path));}
    }
    Component.onCompleted:'''))

    def request(value):
        return json.loads(subprocess.check_output([str(home / 'tasktjik-api'), 'request'], input=json.dumps(value), text=True, env=env))

    def ipc(method, *args):
        return subprocess.check_output(['quickshell', 'ipc', '--path', str(home / 'qml'), 'call', 'images-test', method, *map(str, args)], text=True, env=env, stderr=subprocess.DEVNULL)

    def state():
        try:
            return json.loads(ipc('state'))
        except subprocess.CalledProcessError:
            return {}

    def paste(path, mime='image/png'):
        subprocess.run(['wl-copy', '--type', mime], input=path.read_bytes(), env=env, check=True)
        ipc('paste')

    def count(task_id):
        return len(next((t for t in state().get('tasks', []) if t['id'] == task_id), {}).get('images', []))

    def rendered(number):
        images = state().get('images', {}).get('rendered', [])
        return len(images) == number and all(i['status'] == 1 for i in images)

    cover = Image.new('RGB', (640, 360), '#39354b')
    draw = ImageDraw.Draw(cover)
    draw.rounded_rectangle((40, 40, 600, 320), radius=22, fill='#a8c7a0')
    draw.ellipse((220, 80, 420, 280), fill='#f6c177')
    cover.save(root / 'fixture.png')
    cover.transpose(Image.Transpose.ROTATE_90).save(root / 'portrait.jpg')
    with (root / 'authority.log').open('w') as authority_log, (root / 'qml.log').open('w') as log:
        authority = subprocess.Popen([str(home / 'tasktjik-api'), '--config', str(root / 'server.toml')], env=env, stdout=authority_log, stderr=authority_log)
        process = None
        try:
            wait_for(lambda: (root / 'authority.sock').exists())
            left = request({'action': 'create', 'fields': {'title': 'Sketch the task image experience', 'group': 'All'}})
            right = request({'action': 'create', 'fields': {'title': 'Review the opposite details pane', 'group': 'Work'}})
            plain = request({'action': 'create', 'fields': {'title': 'A task without attachments', 'group': 'Work'}})
            process = subprocess.Popen(['quickshell', '--path', str(home / 'qml')], env=env, stdout=log, stderr=log)
            wait_for(lambda: len(state().get('tasks', [])) == 3, timeout=20)
            time.sleep(.5)
            ipc('select', left['id'])
            paste(root / 'fixture.png')
            wait_for(lambda: count(left['id']) == 1)
            wait_for(lambda: rendered(1))
            assert state()['images']['visible'] and not state()['images']['left'], state()
            paste(root / 'portrait.jpg', 'image/jpeg')
            wait_for(lambda: count(left['id']) == 2)
            wait_for(lambda: rendered(2))
            ipc('capture', root / 'images-right.png')
            wait_for(lambda: (root / 'images-right.png').exists())
            ipc('scrollImages')
            wait_for(lambda: state()['images']['offset'] > 0)
            ipc('clickImages')
            assert state()['selected'] == left['id']
            # Identical pastes do not duplicate the image.
            revision = next(t for t in state()['tasks'] if t['id'] == left['id'])['revision']
            paste(root / 'fixture.png')
            wait_for(lambda: next(t for t in state()['tasks'] if t['id'] == left['id'])['revision'] > revision)
            assert count(left['id']) == 2
            ipc('select', right['id'])
            paste(root / 'fixture.png')
            wait_for(lambda: count(right['id']) == 1)
            wait_for(lambda: rendered(1))
            assert state()['images']['left'], state()
            assert state()['images']['offset'] == 0
            ipc('capture', root / 'images-left.png')
            wait_for(lambda: (root / 'images-left.png').exists())
            # Arrow navigation crosses the covered half and moves the pane.
            ipc('key', 'left')
            wait_for(lambda: state().get('selected') == left['id'])
            assert not state()['images']['left']
            ipc('select', plain['id'])
            assert not state()['images']['visible']
            subprocess.run(['wl-copy', '--type', 'text/plain'], input=b'synthetic text', env=env, check=True)
            ipc('paste')
            wait_for(lambda: 'Copy an image' in state().get('error', ''))
            assert count(plain['id']) == 0
            # Reload from authority: attachments and actual pixel sources survive.
            process.terminate()
            process.wait(timeout=5)
            process = subprocess.Popen(['quickshell', '--path', str(home / 'qml')], env=env, stdout=log, stderr=log)
            wait_for(lambda: len(state().get('tasks', [])) == 3)
            ipc('select', left['id'])
            wait_for(lambda: rendered(2))
            for width in (760, 1480):
                ipc('resize', width)
                time.sleep(.3)
                assert state()['images']['visible']
                ipc('capture', root / f'images-{width}.png')
                wait_for(lambda: (root / f'images-{width}.png').exists())
            errors = (root / 'qml.log').read_text()
            assert not any(word in errors for word in ['ERROR', 'TypeError', 'ReferenceError', 'Binding loop']), errors
            print('PASS clipboard PNG/JPEG, multiple images, deduplication, both pane sides, navigation, text rejection, restart, narrow/wide rendering:', root)
        except Exception:
            print(state(), (root / 'qml.log').read_text(), (root / 'authority.log').read_text(), flush=True)
            raise
        finally:
            for child in (process, authority):
                if child and child.poll() is None:
                    child.terminate()
                    child.wait(timeout=5)
