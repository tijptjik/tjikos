"""Synthetic API integration: nonblocking reads and durable cross-restart cache."""
import http.server
import json
from pathlib import Path
import socketserver
import tempfile
import threading
import time

from rust_parity import Backend, fixture


def main():
    with tempfile.TemporaryDirectory(prefix='runtjik-task-cache-') as temp:
        root = Path(temp)
        env = fixture(root, 1)
        gate = threading.Event()
        entered = threading.Event()
        task = {'id': 'fixture', 'title': 'Synthetic reading', 'kind': 'reading',
                'reference': 'https://example.org', 'group': 'All', 'tags': [],
                'repeat': {}, 'due': '', 'completions': {}, 'revision': 1}

        class Handler(http.server.BaseHTTPRequestHandler):
            def log_message(self, *args):
                pass

            def do_POST(self):
                request = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
                assert request['action'] == 'list'
                entered.set()
                gate.wait(5)
                data = json.dumps({'result': [task]}).encode()
                self.send_response(200)
                self.send_header('Content-Length', str(len(data)))
                self.end_headers()
                self.wfile.write(data)

        class Server(socketserver.UnixStreamServer):
            pass

        path = root / 'api.sock'
        server = Server(str(path), Handler)
        threading.Thread(target=server.serve_forever, daemon=True).start()
        config = Path(env['XDG_CONFIG_HOME']) / 'tasktjik/tasktjik.conf'
        config.parent.mkdir(parents=True)
        config.write_text('[tasktjik]\nsocket=' + json.dumps(str(path)) + '\n')
        Path(env['RUNTJIK_CONFIG']).write_text('[plugins]\nenabled=["Todo"]\n')
        backend = Backend('rust', env)
        try:
            start = time.monotonic()
            state = backend.request(op='query', query='/reading')['app_state']
            assert time.monotonic() - start < 1
            assert state['tasks'] == []
            assert entered.wait(1)
            start = time.monotonic()
            backend.request(op='app-action', provider='Todo', action='refresh')
            assert time.monotonic() - start < 1
            gate.set()
            for _ in range(100):
                state = backend.request(op='app-action', provider='Todo', action='refresh')['app_state']
                if state['tasks']:
                    break
                time.sleep(.02)
            assert state['tasks'][0]['title'] == task['title']
            assert state['mutation_accepted'] is False
        finally:
            gate.set()
            backend.close()
            server.shutdown()
            server.server_close()
        path.unlink()
        backend = Backend('rust', env)
        try:
            state = backend.request(op='query', query='/reading')['app_state']
            assert state['tasks'][0]['title'] == task['title']
            for _ in range(100):
                state = backend.request(op='app-action', provider='Todo', action='refresh')['app_state']
                if state['sync']['error']:
                    break
                time.sleep(.02)
            assert state['sync']['error']
            assert state['tasks'][0]['title'] == task['title']
        finally:
            backend.close()
        print('Task cache: slow API, complete snapshot, restart and offline retention passed')


if __name__ == '__main__':
    main()
