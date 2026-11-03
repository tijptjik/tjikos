"""Saved-site list/grid rendering and real metadata fetch, using synthetic local pages only."""
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import hashlib
import os
from pathlib import Path
import shutil
import subprocess
import sys
import threading
import time
sys.path.insert(0, str(Path(__file__).resolve().parents[2] / 'runtjik/tests'))
from private_compositor import private_compositor, wait_for

app = Path(__file__).resolve().parents[1]


class Pages(BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def do_GET(self):
        self.server.requests += 1
        if self.path in ('/cover.png', '/favicon.ico'):
            data = self.server.image
            mime = 'image/png'
        else:
            data = b'''<html><head><title>Designing for the everyday web</title>
<meta property="og:description" content="A field guide to thoughtful interfaces, small details, and the places we make online.">
<meta property="og:image" content="/cover.png">
<style>body{margin:0;background:#433e68;color:white;font-family:sans-serif;padding:70px}
h1{font-size:64px}p{font-size:28px}img{width:400px;float:right}</style>
</head><body><img src="/cover.png"><h1>European Literature Night</h1>
<p id="description">Loading...</p><script>setTimeout(()=>document.getElementById('description').textContent='Read the city. Discover a story.',100)</script></body></html>'''
            if self.path == '/1':
                data = data.replace(b'<meta property="og:image" content="/cover.png">', b'')
            if self.path == '/blocked':
                data = b'<html><head><title>403 Access denied</title></head><body>Blocked</body></html>'
            mime = 'text/html'
        self.send_response(200)
        self.send_header('Content-Type', mime)
        self.end_headers()
        self.wfile.write(data)


with private_compositor(isolate_pointer=True):
    subprocess.run(['hyprctl', 'output', 'create', 'headless', 'reading-test'], check=True, stdout=subprocess.DEVNULL)
    wait_for(lambda: any(m.get('width', 0) > 0 for m in json.loads(subprocess.check_output(['hyprctl', '-j', 'monitors'], text=True))))
    temp = Path(os.environ['XDG_RUNTIME_DIR']).parent
    # Artwork belongs to the synthetic fixture, never to real reading content.
    from PIL import Image, ImageDraw
    cover = Image.new('RGB', (640, 320), '#433e68')
    draw = ImageDraw.Draw(cover)
    draw.ellipse((350, -100, 740, 290), fill='#e8ae9c')
    draw.rectangle((50, 70, 330, 245), fill='#d6d3e6')
    draw.rectangle((70, 95, 225, 110), fill='#433e68')
    for y in (140, 160, 180):
        draw.rectangle((70, y, 300, y + 5), fill='#9189ac')
    cover.save(temp / 'cover.png')
    server = ThreadingHTTPServer(('127.0.0.1', 0), Pages)
    server.image = (temp / 'cover.png').read_bytes()
    server.requests = 0
    threading.Thread(target=server.serve_forever, daemon=True).start()
    url = f'http://127.0.0.1:{server.server_port}'
    cache = Path(os.environ['XDG_CACHE_HOME']) / 'tasktjik/page-previews'
    cache.mkdir(parents=True)
    legacy = cache / (hashlib.sha256((url + '/1').encode()).hexdigest() + '.json')
    legacy.write_text(json.dumps({'description': 'Old metadata only'}))
    home = temp / 'app'
    shutil.copytree(app / 'qml', home)
    sites = [{'id': str(i), 'title': title, 'kind': 'reading', 'reference': url + '/' + str(i), 'group': group,
              'tags': tags, 'done': False, 'status': 'ready', 'createdAt': f'2026-11-0{i+1}', 'revision': 1}
             for i, (title, group, tags) in enumerate([
                 ('Designing for the everyday web', 'Tech', ['design', 'web']),
                 ('European Literature Night', 'All', ['culture']),
                 ('A weekend in Hong Kong', 'Travel', []),
             ])]
    sites.append({**sites[0], 'id': 'unavailable', 'title': 'A page without a preview', 'reference': 'https://example.invalid/no-preview'})
    sites.append({**sites[0], 'id': 'blocked', 'title': 'A blocked site with a favicon', 'reference': url + '/blocked'})
    (home / 'shell.qml').write_text('''import QtQuick
import Quickshell
import Quickshell.Io
ShellRoot {
    id:root
    property string opened: ''
    property string lastCapture: ''
    FloatingWindow {
        title:'Reading test';implicitWidth:720;implicitHeight:612;color:'#26233a'
        TodoApp {
            id:view;width:688;height:580;anchors.centerIn:parent;focus:true;collection:'reading';view:'all'
            background:Rectangle {color:'#26233a'}
            theme:({background:'#26233a',surface:'#302c43',text:'#e0def4',muted:'#a29bb7',accent:'#ff53a6',border:'#484159'})
            stateData:''' + json.dumps({'tasks': sites, 'groups': ['All', 'Finances', 'Tech', 'Travel', 'Skills']}) + '''
            onRequest:value=>{if(value.action==='open-reference')root.opened=value.id;}
        }
    }
    IpcHandler {
        target:'reading-test'
        function state():string {return JSON.stringify({count:view.visibleCount(),grid:view.readingGrid,categories:view.readingCategories,selected:view.selectedId,opened:root.opened,capture:root.lastCapture,previews:view.testPreviews});}
        function navigate(direction:int,horizontal:bool):void {if(horizontal)view.focusColumn(direction);else view.focusVertical(direction);}
        function openSelected():void {view.action('open-reference');}
        function mode(grid:bool,categories:bool):void {view.readingGrid=grid;view.readingCategories=categories;}
        function filter(text:string):void {view.filter=text;}
        function capture(path:string):void {view.grabToImage(image=>{if(image.saveToFile(path))root.lastCapture=path;});}
    }
}''')
    component = home / 'TodoApp.qml'
    component.write_text(component.read_text().replace('    id: app', '    id: app\n    property var testPreviews: readingPane.previews'))

    def ipc(method, *args):
        return subprocess.check_output(['quickshell', 'ipc', '--path', str(home), 'call', 'reading-test', method, *args], text=True, stderr=subprocess.DEVNULL)

    def state():
        try:
            return json.loads(ipc('state'))
        except subprocess.CalledProcessError:
            return {}

    with (temp / 'reading.log').open('w') as log:
        process = subprocess.Popen(['quickshell', '--path', str(home)], stdout=log, stderr=log)
        try:
            wait_for(lambda: state().get('count') == 5, timeout=20)
            wait_for(lambda: len(state().get('previews', {})) == 5, timeout=100)
            current = state()
            assert not current['grid'] and not current['categories'], current
            assert current['previews'][url + '/0']['image'].startswith('file:'), current
            assert current['previews'][url + '/1']['imageKind'] == 'page', current
            assert json.loads(legacy.read_text())['previewVersion'] == 4
            assert 'thoughtful interfaces' in current['previews'][url + '/0']['description']
            assert current['previews']['https://example.invalid/no-preview']['unavailable']
            assert current['previews'][url + '/blocked']['imageKind'] == 'favicon', current
            assert current['previews'][url + '/blocked']['renderError'] == 'blocked', current
            ipc('capture', str(temp / 'reading-list.png'))
            wait_for(lambda: state().get('capture') == str(temp / 'reading-list.png'))
            ipc('mode', 'true', 'false')
            time.sleep(.3)
            ipc('capture', str(temp / 'reading-grid.png'))
            wait_for(lambda: state().get('capture') == str(temp / 'reading-grid.png'))
            before = state()['selected']
            ipc('navigate', '1', 'true')
            wait_for(lambda: state()['selected'] != before)
            before = state()['selected']
            ipc('navigate', '1', 'false')
            wait_for(lambda: state()['selected'] != before)
            ipc('navigate', '-1', 'false')
            wait_for(lambda: state()['selected'] == before)
            ipc('openSelected')
            wait_for(lambda: state()['opened'] == state()['selected'])
            ipc('mode', 'true', 'true')
            time.sleep(.3)
            ipc('capture', str(temp / 'reading-categories.png'))
            wait_for(lambda: state().get('capture') == str(temp / 'reading-categories.png'))
            ipc('filter', 'literature')
            wait_for(lambda: state()['count'] == 1)
            ipc('filter', 'no matching page')
            wait_for(lambda: state()['count'] == 0)
            time.sleep(.3)
            ipc('capture', str(temp / 'reading-empty.png'))
            wait_for(lambda: state().get('capture') == str(temp / 'reading-empty.png'))
            errors = (temp / 'reading.log').read_text()
            assert not any(word in errors for word in ['ERROR', 'TypeError', 'ReferenceError', 'Binding loop']), errors
            # A fresh UI process must restore old artwork without network work.
            requests = server.requests
            for receipt in cache.glob('*.json'):
                if json.loads(receipt.read_text()).get('image'):
                    old = time.time() - 30 * 86400
                    os.utime(receipt, (old, old))
            process.terminate()
            process.wait(timeout=5)
            process = subprocess.Popen(['quickshell', '--path', str(home)], stdout=log, stderr=log)
            wait_for(lambda: len(state().get('previews', {})) == 5, timeout=5)
            assert state()['previews'] == current['previews'], state()
            assert server.requests == requests, (server.requests, requests)
            errors = (temp / 'reading.log').read_text()
            assert not any(word in errors for word in ['ERROR', 'TypeError', 'ReferenceError', 'Binding loop']), errors
            print('PASS reading list/grid, metadata/image fetch, fallback, categories, navigation/open, search, persistent cache on restart:', temp)
        except Exception:
            print(state(), (temp / 'reading.log').read_text(), flush=True)
            raise
        finally:
            process.terminate()
            process.wait(timeout=5)
            server.shutdown()
