"""Opt-in Firefox/Zen provider. Browsing state stays in the browser."""
import json
import os
from pathlib import Path
import socket
from urllib.parse import urlsplit, parse_qs

DEFAULT_OPTIONS = {'columns': 3}


def runtime_directory():
    root = Path(os.environ['XDG_RUNTIME_DIR'])
    info = root.stat()
    if info.st_uid != os.getuid() or info.st_mode & 0o077:
        raise RuntimeError('XDG_RUNTIME_DIR must be owned by you and private')
    return root / 'runtjik-browser'


def request(path, message):
    with socket.socket(socket.AF_UNIX) as client:
        client.settimeout(12)
        client.connect(str(path))
        client.sendall(json.dumps(message).encode() + b'\n')
        with client.makefile('rb') as stream:
            data = stream.readline(4 * 1024 * 1024 + 1)
        if len(data) > 4 * 1024 * 1024:
            raise RuntimeError('Browser response too large')
        response = json.loads(data)
        if 'error' in response:
            raise RuntimeError(response['error'])
        return response


class Browser:
    name = 'Browser'
    commands = {'/browser': 'Browser windows', '/tabs': 'Browser tabs'}
    aliases = {'/zen': '/browser'}

    def __init__(self, demo=False):
        self.demo = demo

    def query(self, query, settings):
        command, _, term = query.partition(' ')
        tabs_mode = command not in ('/browser', '/zen')
        if not query.startswith('/'):
            term = query
        if self.demo:
            sessions = [('demo', {'windows': [
                {'id': 1, 'title': 'Research', 'tabs': [
                    {'id': 11, 'title': 'Firefox extension documentation', 'url': 'https://example.org/firefox'},
                    {'id': 12, 'title': 'Hyprland window management', 'url': 'https://example.org/hyprland'}]},
                {'id': 2, 'title': 'Reading', 'tabs': [
                    {'id': 21, 'title': 'A synthetic reading list', 'url': 'https://example.org/reading'}]}]})]
        else:
            paths = sorted(runtime_directory().glob('*.sock'))
            sessions = []
            for path in paths:
                try:
                    sessions.append((path.name, request(path, {'op': 'list'})))
                except (ConnectionRefusedError, FileNotFoundError):
                    continue
            if not sessions:
                raise RuntimeError('Connect the Runtjik Browser extension in Zen or Firefox; see browser-extension/README.md')
        rows = []
        for session, state in sessions:
            for window in state['windows']:
                title = window['title'] or 'Browser window'
                tabs = [tab for tab in window['tabs'] if not tab.get('pinned')]
                items = tabs if tabs_mode else [window]
                for item in items:
                    searchable = item['title'] if tabs_mode else ' '.join([title, *(tab['title'] for tab in tabs)])
                    if term.casefold().strip() not in searchable.casefold():
                        continue
                    url = item.get('url', '') if tabs_mode else ''
                    try:
                        domain = urlsplit(url).hostname or ''
                    except ValueError:
                        domain = ''
                    status = [domain]
                    if item.get('muted'):
                        status.append('Muted')
                    elif item.get('audible'):
                        status.append('Playing audio')
                    if item.get('discarded'):
                        status.append('Unloaded')
                    favicon = item.get('favicon', '')
                    if favicon.startswith('moz-remote-image://'):
                        favicon = parse_qs(urlsplit(favicon).query).get('url', [''])[0]
                    if not (favicon.startswith('https://') or favicon.startswith('data:image/')) or len(favicon) > 131072:
                        favicon = ''
                    rows.append({
                        'id': f'Browser:{session}:{"tab" if tabs_mode else "window"}:{item["id"]}',
                        'provider': self.name, 'name': item['title'] or 'Untitled',
                        'subtitle': ' · '.join(filter(None, status)) if tabs_mode else f'{len(tabs)} tabs',
                        'icon': 'zen-browser',
                        'data': {'session': session, 'window': window['id'], 'url': url,
                                 'favicon': favicon, 'keep_open': True,
                                 'tab': item['id'] if tabs_mode else None}})
        return rows

    def tab_request(self, data, op, **extra):
        if data.get('tab') is None and op not in ('focus', 'close'):
            raise ValueError('Select a tab, not a browser window')
        if self.demo:
            return {'tab': {'id': data.get('tab'), 'title': data.get('title', 'Synthetic reading item'),
                            'url': data.get('url', 'https://example.org/reading')}}
        session = data['session']
        if Path(session).name != session or not session.endswith('.sock'):
            raise ValueError('Invalid browser session')
        return request(runtime_directory() / session,
                       {'op': op, 'window': data['window'], 'tab': data['tab'], **extra})

    def reading_draft(self, entry):
        tab = self.tab_request({**entry.data, 'title': entry.name}, 'details')['tab']
        url = tab['url']
        if urlsplit(url).scheme not in ('http', 'https') or not urlsplit(url).hostname:
            raise ValueError('Read later supports web pages with an HTTP or HTTPS URL')
        return {'kind': 'reading', 'title': tab['title'], 'reference': url, 'group': 'All',
                'tags': [], 'due': '', 'repeat': {}}

    def activate(self, entry, settings, mode):
        if mode == 'close':
            response = self.tab_request(entry.data, 'close', url=entry.data.get('url', ''))
            return {'refresh_query': '/tabs' if entry.data.get('tab') is not None else '/browser',
                    'message': 'Tab closed' if entry.data.get('tab') is not None else response.get('message', 'Hyprland window close requested')}
        self.tab_request(entry.data, 'focus')
        return {'kind': 'activated', 'demo': self.demo}
