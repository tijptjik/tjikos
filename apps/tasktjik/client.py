"""HTTPS client: Tailscale supplies identity; no application password is stored."""
import fcntl
import http.client
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import urllib.error
import urllib.request
from urllib.parse import urlsplit
import uuid


def state_root():
    path = Path(os.environ.get('XDG_STATE_HOME', Path.home() / '.local/state')) / 'tasktjik'
    path.mkdir(parents=True, exist_ok=True, mode=0o700)
    return path


def device_identity():
    root = state_root()
    with (root / '.device.lock').open('a') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        path = root / 'device.json'
        if path.exists():
            return json.loads(path.read_text())
        value = {'id': str(uuid.uuid4()), 'name': socket.gethostname()}
        fd, tmp = tempfile.mkstemp(dir=root)
        with os.fdopen(fd, 'w') as handle:
            json.dump(value, handle)
        os.replace(tmp, path)
        return value


def base_url(value):
    parsed = urlsplit(value)
    if parsed.scheme != 'https' or not parsed.hostname or not parsed.hostname.endswith('.ts.net') or parsed.username or parsed.password or parsed.query or parsed.fragment or parsed.port not in (None, 443):
        raise ValueError('Use the HTTPS Tailscale hostname, for example https://si.tailnet.ts.net/tasktjik')
    if parsed.path.rstrip('/') != '/tasktjik':
        raise ValueError('Tasktjik URL must end in /tasktjik')
    return value.rstrip('/')


class TailnetHTTPSConnection(http.client.HTTPSConnection):
    def connect(self):
        try:
            return super().connect()
        except socket.gaierror:
            # Preserve certificate validation and SNI; resolve only a peer whose
            # exact DNS identity is supplied by the local authenticated daemon.
            status = json.loads(subprocess.check_output(['tailscale', 'status', '--json'], timeout=5))
            peers = [status.get('Self', {}), *status.get('Peer', {}).values()]
            peer = next((p for p in peers if p.get('DNSName', '').rstrip('.') == self.host), None)
            if not peer or not peer.get('TailscaleIPs'):
                raise ValueError('Tasktjik host is not present in the local tailnet')
            raw = socket.create_connection((peer['TailscaleIPs'][0], self.port), timeout=self.timeout)
            self.sock = self._context.wrap_socket(raw, server_hostname=self.host)


class TailnetHTTPSHandler(urllib.request.HTTPSHandler):
    def https_open(self, request):
        return self.do_open(TailnetHTTPSConnection, request, context=self._context)


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, *args, **kwargs):
        raise ValueError('Unexpected redirect from Tasktjik API')


def http_request(url, endpoint, body=None):
    data = json.dumps(body).encode() if body is not None else None
    request = urllib.request.Request(base_url(url) + '/api/v1/' + endpoint, data=data,
        headers={'Content-Type': 'application/json', 'X-Tasktjik-Request': '1', 'X-Tasktjik-Device': device_identity()['id']})
    try:
        opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), TailnetHTTPSHandler(), NoRedirect())
        with opener.open(request, timeout=10) as response:
            return json.loads(response.read(8 * 1024 * 1024))
    except urllib.error.HTTPError as error:
        try:
            message = json.loads(error.read(65536)).get('error', str(error))
        except (ValueError, AttributeError):
            message = str(error)
        raise ValueError(message) from error
    except urllib.error.URLError as error:
        raise ValueError('Tasktjik is unreachable; check Tailscale: ' + str(error.reason)) from error


def api_request(url, request):
    request = dict(request)
    if request.get('action', 'list') not in ('list', 'capabilities'):
        request.setdefault('request_id', str(uuid.uuid4()))
    return http_request(url, 'request', request)['result']


def connect(url, notifications=False):
    url = base_url(url)
    who = http_request(url, 'identity')
    device = {**device_identity(), 'notifications': notifications}
    http_request(url, 'devices', device)
    root = Path(os.environ.get('XDG_CONFIG_HOME', Path.home() / '.config')) / 'tasktjik'
    root.mkdir(parents=True, exist_ok=True, mode=0o700)
    path = root / 'tasktjik.conf'
    with (root / '.config.lock').open('a') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        content = path.read_text() if path.exists() else '[tasktjik]\n'
        import re
        section = re.search(r'(?ms)^\[tasktjik\]\s*\n(.*?)(?=^\[|\Z)', content)
        if not section:
            content += '\n[tasktjik]\n'
            section = re.search(r'(?ms)^\[tasktjik\]\s*\n(.*?)(?=^\[|\Z)', content)
        body = re.sub(r'(?m)^url\s*=.*\n?', '', section.group(1))
        content = content[:section.start(1)] + 'url = ' + json.dumps(url) + '\n' + body + content[section.end(1):]
        if path.exists():
            import time
            backup = path.with_name(path.name + '.pre-api-' + str(time.time_ns()))
            backup.write_text(path.read_text())
            backup.chmod(0o600)
        fd, tmp = tempfile.mkstemp(dir=root)
        with os.fdopen(fd, 'w') as handle:
            handle.write(content)
        os.replace(tmp, path)
    return {'connected': url, 'identity': who['login'], 'device': device}
