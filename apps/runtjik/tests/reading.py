"""Reading handoff against the Rust HTTP authority and a synthetic browser bridge."""
import json
import os
from pathlib import Path
import socketserver
import sqlite3
import subprocess
import tempfile
import threading
import time

from rust_parity import APP, Backend, fixture


with tempfile.TemporaryDirectory(prefix='runtjik-reading-') as directory:
    root = Path(directory)
    env = fixture(root)
    with open(env['RUNTJIK_CONFIG'], 'a') as config_file:
        config_file.write('[plugins]\nenabled=["Browser","Todo"]\n')
    runtime = root / 'runtime'
    runtime.mkdir(mode=0o700)
    bridge_root = runtime / 'runtjik-browser'
    bridge_root.mkdir(mode=0o700)
    env['XDG_RUNTIME_DIR'] = str(runtime)
    tab = {'id': 11, 'title': 'Synthetic reading article', 'url': 'https://example.org/article'}
    closes = []

    class Bridge(socketserver.StreamRequestHandler):
        def handle(self):
            request = json.loads(self.rfile.readline())
            if request['op'] == 'list':
                response = {'windows': [{'id': 1, 'title': 'Synthetic browser', 'tabs': [tab]}]}
            elif request['op'] == 'details':
                response = {'tab': dict(tab)}
            elif request['op'] == 'close':
                if request['url'] != tab['url']:
                    response = {'error': 'Tab navigated; it was left open'}
                else:
                    # The close request must follow a durable authority commit.
                    with sqlite3.connect(root / 'database/tasks.sqlite3') as db:
                        assert db.execute('SELECT count(*) FROM tasks').fetchone()[0] > 0
                    closes.append(request)
                    response = {}
            else:
                raise AssertionError(request)
            self.wfile.write(json.dumps(response).encode() + b'\n')

    bridge = socketserver.ThreadingUnixStreamServer(str(bridge_root / 'fixture.sock'), Bridge)
    threading.Thread(target=bridge.serve_forever, daemon=True).start()
    socket = root / 'api.sock'
    server_config = root / 'server.toml'
    server_config.write_text(f'''[server]
socket={json.dumps(str(socket))}
directory={json.dumps(str(root / 'database'))}
origin="https://si.synthetic.ts.net"
allowed_users=["fixture@example.org"]
[server.local_users]
"{os.getuid()}"="fixture@example.org"
''')
    server = subprocess.Popen([str(APP.parent / 'tasktjik/tasktjik-api'), '--config', str(server_config)],
                              stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
    config = root / 'config/tasktjik/tasktjik.conf'
    config.parent.mkdir(parents=True)
    config.write_text('[tasktjik]\nsocket=' + json.dumps(str(socket)) + '\n')
    # Lose exactly one reply after curl has delivered the request and received its ack.
    bindir = root / 'bin'
    bindir.mkdir()
    (bindir / 'curl').write_text('''#!/usr/bin/env python3
import os, pathlib, subprocess, sys
body=sys.stdin.buffer.read()
result=subprocess.run(['/usr/bin/curl', *sys.argv[1:]], input=body, capture_output=True)
flag=pathlib.Path(os.environ['READING_DROP_REPLY'])
if b'"action":"create"' in body and flag.exists() and result.returncode == 0:
    flag.unlink()
    sys.stderr.write('Synthetic reply lost after commit')
    sys.exit(52)
sys.stdout.buffer.write(result.stdout)
sys.stderr.buffer.write(result.stderr)
sys.exit(result.returncode)
''')
    (bindir / 'curl').chmod(0o755)
    env['PATH'] = str(bindir) + ':' + env['PATH']
    env['READING_DROP_REPLY'] = str(root / 'drop-reply')
    backend = None
    try:
        for _ in range(100):
            if socket.exists():
                break
            assert server.poll() is None, server.stderr.read().decode()
            time.sleep(.02)
        assert socket.exists()
        backend = Backend('rust', env)

        def begin():
            rows = backend.request(op='query', query='/tabs')['results']
            opened = backend.request(op='activate', id=rows[0]['id'], mode='switch')
            assert opened['app_mode'] == 'Todo', opened
            assert opened['edit_query'] == '/reading', opened
            state = opened['app_state']
            assert state['draft']['title'] == tab['title'], state
            return state

        def save(draft):
            return backend.request(op='app-action', provider='Todo', action='reading-save',
                                   draft_id=draft['draft_id'], fields=draft['draft'])

        draft = begin()
        cancelled = backend.request(op='app-action', provider='Todo', action='reading-cancel', draft_id=draft['draft_id'])
        assert cancelled['app_state']['mutation_accepted']
        assert not closes
        with sqlite3.connect(root / 'database/tasks.sqlite3') as db:
            assert db.execute('SELECT count(*) FROM tasks').fetchone()[0] == 0
        draft = begin()
        # An unavailable HTTP authority must not close the browser tab.
        config.write_text('[tasktjik]\nsocket=' + json.dumps(str(root / 'missing.sock')) + '\n')
        failed = save(draft)
        assert failed['kind'] == 'error', failed
        assert not closes
        config.write_text('[tasktjik]\nsocket=' + json.dumps(str(socket)) + '\n')
        Path(env['READING_DROP_REPLY']).touch()
        lost = save(draft)
        assert lost['kind'] == 'error' and 'reply lost' in lost['message'], lost
        assert not closes
        with sqlite3.connect(root / 'database/tasks.sqlite3') as db:
            assert db.execute('SELECT count(*) FROM tasks').fetchone()[0] == 1
        saved = save(draft)
        assert saved['app_state']['mutation_accepted'], saved
        assert saved['app_state']['message'] == '', saved
        assert len(closes) == 1
        with sqlite3.connect(root / 'database/tasks.sqlite3') as db:
            records = [json.loads(row[0]) for row in db.execute('SELECT body FROM tasks')]
            assert len(records) == 1, records
            assert records[0]['kind'] == 'reading' and records[0]['reference'] == tab['url']
        assert save(draft)['kind'] == 'error'  # stale duplicate cannot close again
        draft = begin()
        tab['url'] = 'https://example.org/navigated'
        saved = save(draft)
        assert saved['app_state']['mutation_accepted'], saved
        assert 'Saved to Reading; tab could not be closed' in saved['app_state']['message'], saved
        assert len(closes) == 1
        tab['url'] = 'about:config'
        rows = backend.request(op='query', query='/tabs')['results']
        assert backend.request(op='activate', id=rows[0]['id'], mode='switch')['kind'] == 'error'
        print('PASS: HTTP capture, cancel, outage, lost-reply deduplication, stale save, navigation guard and unsupported URL')
    finally:
        if backend:
            backend.close()
        bridge.shutdown()
        bridge.server_close()
        server.terminate()
        server.wait(timeout=5)
