"""Private-bus synthetic HTTP covers: caching, delayed loads, failures and races."""
import collections
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import shutil
import struct
import subprocess
import sys
import tempfile
import threading
import time
import zlib

from render import wait_for

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from bartjik import ipc


def png(rgb):
    def chunk(kind, data):
        return struct.pack('!I', len(data)) + kind + data + struct.pack('!I', zlib.crc32(kind + data))
    return (b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('!2I5B', 16, 16, 8, 2, 0, 0, 0))
            + chunk(b'IDAT', zlib.compress((b'\0' + bytes(rgb) * 16) * 16)) + chunk(b'IEND', b''))


def main():
    assert os.environ.get('DBUS_SESSION_BUS_ADDRESS', '').startswith('unix:path=/tmp/')
    destination = Path(sys.argv[1]).resolve()
    destination.mkdir(parents=True, exist_ok=True)
    hits = collections.Counter()
    release = threading.Event()

    class Handler(BaseHTTPRequestHandler):
        def do_GET(self):
            hits[self.path] += 1
            if self.path == '/slow':
                release.wait(10)
            if self.path == '/fail':
                self.send_error(503)
                return
            data = png({'/red': (210, 40, 40), '/slow': (40, 40, 210), '/green': (40, 180, 70)}[self.path])
            self.send_response(200)
            self.send_header('Content-Type', 'image/png')
            self.send_header('Content-Length', str(len(data)))
            self.end_headers()
            self.wfile.write(data)

        def log_message(self, *args):
            pass

    server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    base = f'http://127.0.0.1:{server.server_port}'
    try:
        with tempfile.TemporaryDirectory(prefix='bartjik-artwork-test-') as td:
            temp = Path(td)
            shutil.copytree(ROOT / 'qml', temp / 'qml')
            p = temp / 'qml/shell.qml'
            text = p.read_text().replace('    id: root', "    id: root\n    property var artworkOverride:null")
            text = text.replace('return (state.providers[bar.sources[id]] || {}).items || [];', '''
                const rows=(state.providers[bar.sources[id]] || {}).items || [];
                return m.provider==='playerctl' && artworkOverride!==null
                    ? rows.map(r=>Object.assign({},r,{media:Object.assign({},r.media,{artwork:artworkOverride})})) : rows;''')
            text = text.replace("case 'status':break;", "case 'artworkTest':artworkOverride=req.source;return {ok:true};\n        case 'status':break;")
            p.write_text(text)
            p = temp / 'qml/MediaDetails.qml'
            p.write_text(p.read_text().replace('    id:details', '''    id:details
    function artworkState(){return {source:cover.displayedSource,ready:cover.ready,transitioning:cover.transitioning};}'''))
            p = temp / 'qml/DetailPanel.qml'
            p.write_text(p.read_text().replace('return {opened:visible,title:', 'return {image:mediaDetails.artworkState(),opened:visible,title:'))
            cfg = json.loads((ROOT / 'examples/comparison.json').read_text())
            for bar in cfg['bars']:
                bar['autoHide'] = False
            path = temp / 'config.json'
            path.write_text(json.dumps(cfg))
            os.environ.update(BARTJIK_CONFIG=str(path), BARTJIK_QML=str(temp / 'qml'), XDG_CACHE_HOME=str(temp / 'cache'))

            def state():
                return ipc({'op': 'detailState'})['panel']['image']

            def source(name):
                return (temp / 'cache/bartjik/artwork' / (hashlib.sha256((base + name).encode()).hexdigest() + '.img')).as_uri()

            def request(name):
                ipc({'op': 'artworkTest', 'source': base + name})

            def settled(name):
                value = state()
                return value['ready'] and not value['transitioning'] and value['source'] == source(name)

            def capture(name):
                path = destination / (name + '.png')
                ipc({'op': 'captureDetail', 'path': str(path)})
                wait_for(lambda: path.exists() and path.read_bytes().endswith(b'IEND\xaeB`\x82'))

            with (destination / 'artwork.log').open('w') as log:
                process = subprocess.Popen([str(ROOT / 'bin/bartjik'), '--demo'], stdout=log, stderr=log)
                try:
                    wait_for(lambda: ipc({'op': 'status'}).get('ready'))
                    local = ROOT / 'qml/icons/demo-album.png'
                    ipc({'op': 'artworkTest', 'source': str(local)})
                    ipc({'op': 'detailTest', 'module': 'music'})
                    wait_for(lambda: state()['source'] == local.as_uri() and state()['ready'])
                    request('/red')
                    wait_for(lambda: settled('/red'))
                    request('/slow')
                    wait_for(lambda: hits['/slow'] == 1)
                    time.sleep(.3)
                    assert settled('/red'), state()
                    capture('held-while-loading')
                    request('/green')
                    wait_for(lambda: settled('/green'))
                    release.set()
                    wait_for(lambda: Path(source('/slow').removeprefix('file://')).exists())
                    time.sleep(.3)
                    assert settled('/green'), state()
                    capture('latest-cover')
                    request('/fail')
                    wait_for(lambda: hits['/fail'] == 1)
                    time.sleep(.3)
                    assert settled('/green'), state()
                    request('/slow')
                    wait_for(lambda: settled('/slow'))
                    assert hits['/slow'] == 1, hits
                    request('/red')
                    wait_for(lambda: settled('/red'))
                    assert hits['/red'] == 1, hits
                    ipc({'op': 'detailTest', 'module': 'music', 'leave': True})
                    time.sleep(.4)
                    ipc({'op': 'detailTest', 'module': 'music'})
                    wait_for(lambda: settled('/red'))
                    assert hits['/red'] == 1, hits
                finally:
                    release.set()
                    process.terminate()
                    process.wait(timeout=10)
            errors = [line for line in (destination / 'artwork.log').read_text().splitlines()
                      if any(s in line for s in ('WARN scene', 'ERROR', 'ReferenceError', 'TypeError', 'Binding loop'))]
            assert not errors, errors
    finally:
        release.set()
        server.shutdown()
        server.server_close()
    print('PASS: one fetch per cover; old image retained during slow/error responses; stale result ignored; popup reuse cached.')


if __name__ == '__main__':
    main()
