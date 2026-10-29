"""Bundled Wayland clipboard and capture providers; private data stays outside the repo."""
from datetime import datetime
from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
import os
from pathlib import Path
import re
import select
import shutil
import shlex
import socket
import subprocess
import sys
import time
import tempfile
import uuid

DEFAULT_OPTIONS = {
    'Clipboard': {'max_results': 50},
    'Screenshots': {'directory': 'xdg:PICTURES/screenshots', 'copy_to_clipboard': True, 'viewer': ['imv'], 'max_results': 40, 'gallery_columns': 3, 'gallery_rows': 3},
    'Recorder': {'directory': 'xdg:VIDEOS', 'audio': False, 'fps': 30, 'max_results': 40, 'gallery_columns': 3, 'gallery_rows': 3, 'countdown_seconds': 3},
}


def validate_options(settings):
    for name, defaults in DEFAULT_OPTIONS.items():
        options = settings[name]
        if set(options) - set(defaults):
            raise ValueError(f'Unknown {name} setting')
        for key, default in defaults.items():
            if type(options[key]) is not type(default):
                raise ValueError(f'{name}.{key} must be {type(default).__name__}')
        if not 1 <= options['max_results'] <= 200:
            raise ValueError(f'{name}.max_results must be between 1 and 200')
        if name == 'Screenshots' and (not options['viewer'] or not all(isinstance(v, str) and v for v in options['viewer'])):
            raise ValueError('Screenshots.viewer must be a nonempty argv array')
        if 'directory' in options:
            capture_directory(options['directory'])
        for key in ('gallery_columns', 'gallery_rows'):
            if key in options and not 2 <= options[key] <= 6:
                raise ValueError(f'{name}.{key} must be between 2 and 6')
        if name == 'Recorder' and not 0 <= options['countdown_seconds'] <= 10:
            raise ValueError('Recorder.countdown_seconds must be between 0 and 10')
        if name == 'Recorder' and not 1 <= options['fps'] <= 120:
            raise ValueError('Recorder.fps must be between 1 and 120')


def capture_directory(value):
    if value.startswith('xdg:'):
        kind, _, suffix = value[4:].partition('/')
        if kind not in ('VIDEOS', 'PICTURES') or '..' in Path(suffix).parts:
            raise ValueError('Use xdg:VIDEOS or xdg:PICTURES with an optional subdirectory')
        base = Path.home() / ('Videos' if kind == 'VIDEOS' else 'Pictures')
        config = Path(os.environ.get('XDG_CONFIG_HOME', str(Path.home() / '.config'))) / 'user-dirs.dirs'
        if config.exists():
            for line in config.read_text().splitlines():
                if line.startswith('XDG_' + kind + '_DIR='):
                    values = shlex.split(line.partition('=')[2], comments=True)
                    if len(values) == 1:
                        base = Path(values[0].replace('$HOME', str(Path.home())))
        value = str(base / suffix)
    path = Path(value).expanduser()
    if not path.is_absolute():
        raise ValueError('Capture directory must be absolute or start with ~/')
    path = path.resolve()
    # Includes source checkouts and installed payloads. Never store captures here.
    roots = [Path(__file__).resolve().parent]
    repository = next((parent for parent in Path(__file__).resolve().parents if (parent / '.git').exists()), None)
    if repository is not None and repository != Path.home():
        roots.append(repository)
    if any(path == root or root in path.parents for root in roots):
        raise ValueError('Captures must be stored outside the repository/application directory')
    return path


def run(argv, **kwargs):
    try:
        # wl-copy forks a selection owner that retains its output descriptors.
        # Pipes would wait for that owner to exit, not just the successful copy.
        if 'input' not in kwargs:
            kwargs.setdefault('stdin', subprocess.DEVNULL)
        output = subprocess.DEVNULL if argv[0] == 'wl-copy' else subprocess.PIPE
        return subprocess.run(argv, check=True, stdout=output, stderr=output, timeout=kwargs.pop('timeout', 5), **kwargs).stdout or b''
    except FileNotFoundError:
        raise ValueError(f'Install {argv[0]} to use this action') from None
    except subprocess.CalledProcessError as exc:
        raise RuntimeError(f'{argv[0]} failed: {(exc.stderr or b"unknown error").decode(errors="replace")[:300]}') from None


def hypr(kind):
    return json.loads(run(['hyprctl', '-j', kind]))


def visible_workspace(monitor):
    return monitor.get('specialWorkspace') if monitor.get('specialWorkspace', {}).get('id', 0) else monitor['activeWorkspace']


def geometry(client):
    x, y = client['at']
    width, height = client['size']
    if width < 2 or height < 2:
        raise ValueError('Target must be at least 2 pixels wide and high')
    return f'{int(x)},{int(y)} {int(width)}x{int(height)}'


def selection():
    # slurp otherwise reads rectangle candidates from the launcher's persistent stdin.
    result = subprocess.run(['slurp'], stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                            stderr=subprocess.PIPE, timeout=120)
    if result.returncode:
        if result.returncode == 1 and result.stderr.strip() in (b'', b'selection cancelled'):
            return None
        if result.stderr.strip():
            raise RuntimeError('Region selector failed: ' + result.stderr.decode(errors='replace').strip())
        return None
    value = result.stdout.decode().strip()
    if not re.fullmatch(r'-?\d+,-?\d+ [1-9]\d*x[1-9]\d*', value):
        raise ValueError('Invalid selected region')
    return value


def trash_action(operation, path=None):
    argv = ['/usr/bin/python3', '-B', str(Path(__file__).with_name('capture_trash.py')), operation]
    if path is not None:
        argv.append(str(path))
    result = subprocess.run(argv, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=15)
    try:
        response = json.loads(result.stdout)
    except ValueError:
        raise RuntimeError('Trash service unavailable: ' + result.stderr.decode(errors='replace')[-300:]) from None
    if result.returncode or response.get('error'):
        raise RuntimeError(response.get('error', 'Trash operation failed'))
    return response


def runtime_socket():
    directory = Path(os.environ.get('XDG_RUNTIME_DIR', f'/run/user/{os.getuid()}')) / 'runtjik-capture'
    directory.mkdir(mode=0o700, parents=True, exist_ok=True)
    return directory / 'recorder.sock'


def recorder_request(action='status'):
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as connection:
        connection.settimeout(.5)
        try:
            connection.connect(str(runtime_socket()))
        except (FileNotFoundError, ConnectionRefusedError):
            return None
        connection.sendall((action + '\n').encode())
        return json.loads(connection.recv(8192))


class DesktopProvider:
    icon = 'applications-multimedia'
    aliases = {}

    def __init__(self, demo=False):
        self.demo = demo

    def row(self, key, title, subtitle='', **data):
        return {'id': self.name + ':' + key, 'name': title, 'subtitle': subtitle,
                'provider': self.name, 'icon': self.icon, 'data': {'keep_open': not (data.get('capture') or data.get('file')), **data}}

    def message(self, text):
        return {'kind': 'message', 'message': text}


class Clipboard(DesktopProvider):
    name = 'Clipboard'
    icon = 'edit-paste'
    commands = {'/cliphist': 'Search clipboard history · Enter copies an item'}

    def __init__(self, demo=False):
        super().__init__(demo)
        self.executor = ThreadPoolExecutor(max_workers=2)
        # Session-private previews disappear with the provider, never enter history.
        self.preview_dir = None
        self.previews = {}
        self.pending = False

    def render_preview(self, ident):
        target = Path(self.preview_dir.name) / (ident + '.png')
        try:
            if self.demo:
                target = target.with_suffix('.svg')
                target.write_text('<svg xmlns="http://www.w3.org/2000/svg" width="320" height="180"><rect width="320" height="180" fill="#41365c"/><circle cx="160" cy="90" r="60" fill="#fb92ba"/></svg>')
            else:
                data = run(['cliphist', 'decode'], input=ident.encode())
                run(['magick', '-limit', 'memory', '64MiB', '-limit', 'map', '128MiB',
                     '-[0]', '-thumbnail', '320x180>', str(target)], input=data, timeout=10)
            target.chmod(0o600)
            return str(target)
        except Exception:
            target.unlink(missing_ok=True)
            return ''

    def preview(self, ident):
        if self.preview_dir is None:
            try:
                self.preview_dir = tempfile.TemporaryDirectory(prefix='runtjik-clipboard-', dir=os.environ.get('XDG_RUNTIME_DIR'))
            except OSError:
                self.preview_dir = tempfile.TemporaryDirectory(prefix='runtjik-clipboard-')
        if ident not in self.previews:
            self.previews[ident] = self.executor.submit(self.render_preview, ident)
        future = self.previews[ident]
        self.pending = self.pending or not future.done()
        return future.result() if future.done() else ''

    def query(self, query, settings):
        self.pending = False
        command, _, term = query.partition(' ')
        if command != '/cliphist':
            return []
        if self.demo:
            lines = ['1\tSynthetic clipboard note', '2\thttps://example.com', '3\t[[ binary data 1200 bytes png 32x32 ]]']
        else:
            lines = run(['cliphist', 'list']).decode(errors='replace').splitlines()
        rows = []
        for line in lines:
            ident, separator, preview = line.partition('\t')
            if separator and ident.isdigit() and term.casefold() in preview.casefold():
                is_image = preview.startswith('[[ binary data ') and bool(re.search(r'\b(png|jpeg|jpg|gif|webp|bmp|tiff)\b', preview, re.I))
                title = preview
                if is_image:
                    kind = re.search(r'\b(png|jpeg|jpg|gif|webp|bmp|tiff)\b', preview, re.I).group().upper()
                    size = re.search(r'\b\d+x\d+\b', preview)
                    title = kind + ' image' + (' · ' + size.group().replace('x', '×') if size else '')
                rows.append(self.row(ident, title, 'Enter copies to clipboard', clip_id=ident,
                                     thumbnail=self.preview(ident) if is_image else ''))
            if len(rows) >= settings['plugin_settings'][self.name]['max_results']:
                break
        return rows or [self.row('empty', 'No matching clipboard items', 'History comes from cliphist; its wl-paste watcher must be running')]

    def activate(self, entry, settings, mode):
        if self.demo:
            return self.message('Demo clipboard item selected; clipboard unchanged')
        if 'clip_id' not in entry.data:
            return self.message('No clipboard item selected')
        data = run(['cliphist', 'decode'], input=entry.data['clip_id'].encode())
        run(['wl-copy'], input=data)
        # Do not log clipboard IDs or content in launcher history.
        return {'kind': 'activated'}


class CaptureProvider(DesktopProvider):
    def __init__(self, demo=False):
        super().__init__(demo)
        self.executor = ThreadPoolExecutor(max_workers=2)
        self.thumbnails = {}
        self.pending = False

    def thumbnail(self, path):
        if self.name == 'Screenshots':
            return str(path)
        stamp = path.stat()
        key = hashlib.sha256(f'{path}:{stamp.st_mtime_ns}:{stamp.st_size}'.encode()).hexdigest()
        cache = Path(os.environ.get('XDG_CACHE_HOME', str(Path.home() / '.cache'))) / 'runtjik/thumbnails'
        cache.mkdir(parents=True, exist_ok=True, mode=0o700)
        target = cache / (key + '.jpg')
        if target.exists():
            return str(target)
        if key not in self.thumbnails:
            self.thumbnails[key] = self.executor.submit(self.render_thumbnail, path, target)
        self.pending = self.pending or not self.thumbnails[key].done()
        return ''

    @staticmethod
    def render_thumbnail(path, target):
        temporary = target.with_suffix('.tmp.jpg')
        try:
            run(['ffmpeg', '-v', 'error', '-nostdin', '-y', '-i', str(path), '-frames:v', '1',
                 '-vf', 'scale=480:-1', str(temporary)], timeout=15)
            temporary.chmod(0o600)
            temporary.replace(target)
        except Exception:
            temporary.unlink(missing_ok=True)

    def query(self, query, settings):
        self.pending = False
        command, _, term = query.partition(' ')
        if command not in self.commands and command not in self.aliases:
            return []
        base = self.base
        term = term.strip()
        if term == 'history' or term.startswith('history '):
            return self.history(term[7:].strip(), settings)
        if term in ('screen', 'workspace', 'window'):
            if self.demo:
                return [self.row('target', 'Demo ' + term, 'Synthetic target · no capture', capture=term, target='demo')]
            if term == 'window':
                if self.name == 'Recorder':
                    return [self.row('portal', 'Choose a window', 'Use the desktop sharing picker; recording follows the selected window', capture='window')]
                clients = sorted([c for c in hypr('clients') if c.get('mapped') and not c.get('hidden')], key=lambda c: (c.get('title') or c.get('class', '')).casefold())
                return [self.row(c['address'], c.get('title') or c.get('class', 'Window'),
                                 f"Workspace {c['workspace']['name']} · visible window area", capture=term, target=c['address']) for c in clients]
            monitors = sorted(hypr('monitors'), key=lambda m: (m['name'] if term == 'screen' else str(visible_workspace(m)['name'])).casefold())
            return [self.row(m['name'], m['name'] if term == 'screen' else 'Workspace ' + str(visible_workspace(m)['name']),
                             m.get('description', '') if term == 'screen' else m['name'] + ' · visible workspace; recording stops when switched',
                             capture=term, target=m['name'], workspace=visible_workspace(m)['id']) for m in monitors]
        rows = []
        if self.name == 'Recorder':
            status = {'file': 'Synthetic recording.mp4'} if self.demo else recorder_request()
            if status:
                rows.append(self.row('stop', 'Stop recording', Path(status['file']).name, stop=True))
        rows.extend([
            self.row('screen', 'Screen', 'Choose a monitor', edit=base + ' screen'),
            self.row('workspace', 'Workspace', 'Choose a currently visible workspace', edit=base + ' workspace'),
            self.row('window', 'Window', 'Choose a window', edit=base + ' window'),
            self.row('region', 'Region', 'Drag to select an area · Escape cancels', capture='region'),
            self.row('history', 'Previous recordings' if self.name == 'Recorder' else 'Previous screenshots',
                     'Browse newest first · Enter opens a file', edit=base + ' history'),
        ])
        return [row for row in rows if not term or term.casefold() in (row['name'] + ' ' + row['subtitle']).casefold()]

    def history(self, term, settings):
        self.pending = False
        if self.demo:
            cache = Path(os.environ.get('XDG_CACHE_HOME', '/tmp')) / 'runtjik/demo-thumbnails'
            cache.mkdir(parents=True, exist_ok=True)
            rows = []
            for index in range(9):
                path = cache / f'{index}.svg'
                color = ['#eb6f92', '#9ccfd8', '#c4a7e7'][index % 3]
                path.write_text(f'<svg xmlns="http://www.w3.org/2000/svg" width="480" height="270"><rect width="480" height="270" fill="#191724"/><rect x="24" y="24" width="432" height="222" rx="18" fill="{color}"/><circle cx="{90+index*25}" cy="125" r="45" fill="#26233a"/><path d="M50 220 L210 80 L350 220" fill="#403a52"/></svg>')
                rows.append(self.row(f'demo-file-{index}', f'Demo capture {index + 1}', 'Synthetic capture history', thumbnail=str(path)))
            return rows
        directory = capture_directory(settings['plugin_settings'][self.name]['directory'])
        extensions = {'.mp4', '.mkv', '.webm'} if self.name == 'Recorder' else {'.png', '.jpg', '.jpeg', '.webp'}
        files = []
        if directory.exists():
            for path in directory.iterdir():
                if path.suffix.lower() in extensions and path.is_file() and not path.is_symlink() and term.casefold() in path.name.casefold():
                    try:
                        files.append((path.stat().st_mtime, path))
                    except FileNotFoundError:
                        continue
        status = recorder_request() if self.name == 'Recorder' else None
        active = status.get('file') if status else None
        rows = [self.row(str(path), path.name, datetime.fromtimestamp(stamp).strftime('%Y-%m-%d %H:%M') + ' · ' + str(directory), file=str(path), thumbnail=self.thumbnail(path))
                for stamp, path in sorted(files, reverse=True)[:settings['plugin_settings'][self.name]['max_results']] if str(path) != active]
        return rows[:settings['plugin_settings'][self.name]['max_results']] or [self.row('empty', 'No captures found', str(directory))]

    def activate(self, entry, settings, mode):
        if 'edit' in entry.data:
            return {'kind': 'edit', 'text': entry.data['edit']}
        if self.demo:
            return self.message('Synthetic capture action; no files or recording created')
        options = settings['plugin_settings'][self.name]
        if 'file' in entry.data:
            path = Path(entry.data['file'])
            if path.parent != capture_directory(options['directory']) or not path.is_file() or path.is_symlink():
                raise ValueError('Capture is no longer available')
            if mode == 'close':
                if self.name == 'Recorder':
                    status = recorder_request()
                    if status and status.get('file') == str(path):
                        raise ValueError('Stop recording before moving it to Trash')
                return {**trash_action('trash', path), 'refresh_query': self.base + ' history'}
            argv = options['viewer'] if self.name == 'Screenshots' else ['xdg-open']
            if self.name == 'Screenshots' and Path(argv[0]).name == 'imv':
                argv = [sys.executable, '-B', str(Path(__file__).with_name('image_viewer.py')), '--viewer', json.dumps(argv), '--', str(path)]
            else:
                argv = [*argv, str(path)]
            subprocess.Popen(argv, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)
            return {'kind': 'activated'}
        if entry.data.get('stop'):
            status = recorder_request('stop')
            return self.message('Stopping recording; the file will appear in history when finalized' if status else 'No Runtjik recording is active')
        if 'capture' not in entry.data:
            return self.message('Choose a capture type or previous file')
        return self.capture(entry.data, options)

    def capture(self, data, options):
        mode = data['capture']
        recording = self.name == 'Recorder'
        require = ['gpu-screen-recorder'] if recording and mode == 'window' else ['wf-recorder'] if recording else ['grim']
        if mode == 'region':
            require.append('slurp')
        if recording and mode != 'window' and options['audio']:
            require.append('pactl')
        for tool in require:
            if not shutil.which(tool):
                raise ValueError(f'Install {tool} to use this action')
        if recording and recorder_request():
            raise ValueError('A Runtjik recording is already active; stop it first')
        area, output, workspace = None, None, None
        if mode == 'region':
            area = selection()
            if not area:
                return self.message('Capture cancelled')
        elif mode == 'window' and not recording:
            client = next((c for c in hypr('clients') if c['address'] == data['target'] and c.get('mapped') and not c.get('hidden')), None)
            if not client:
                raise ValueError('Window has closed')
            from runtjik import focus_window
            focus_window(client)
            time.sleep(.3)
            client = next((c for c in hypr('clients') if c['address'] == data['target']), None)
            if not client:
                raise ValueError('Window has closed')
            area = geometry(client)
        elif mode in ('screen', 'workspace'):
            monitor = next((m for m in hypr('monitors') if m['name'] == data['target']), None)
            if not monitor:
                raise ValueError('Monitor disconnected')
            if mode == 'workspace':
                workspace = data['workspace']
                if visible_workspace(monitor)['id'] != workspace:
                    raise ValueError('Workspace changed; choose the target again')
            output = monitor['name']
        if recording and options['countdown_seconds']:
            countdown = subprocess.run(['/usr/bin/python3', '-B', str(Path(__file__).with_name('capture_countdown.py')),
                                        json.dumps({'countdown_seconds': options['countdown_seconds'], 'area': area, 'output': output})],
                                       capture_output=True, timeout=options['countdown_seconds'] + 15)
            if countdown.returncode == 2:
                return self.message('Recording cancelled')
            if countdown.returncode:
                raise RuntimeError('Could not display recording countdown: ' + countdown.stderr.decode(errors='replace')[-300:])
            time.sleep(.3)  # Let the overlay unmap before the first captured frame.
        directory = capture_directory(options['directory'])
        directory.mkdir(parents=True, exist_ok=True, mode=0o700)
        filename = directory / (('Recording-' if recording else 'Screenshot-') + datetime.now().strftime('%Y-%m-%d_%H-%M-%S-') + uuid.uuid4().hex[:8] + ('.mp4' if recording else '.png'))
        if recording:
            spec = {'mode': mode, 'area': area, 'output': output, 'workspace': workspace, 'file': str(filename), **options}
            process = subprocess.Popen([sys.executable, '-B', str(Path(__file__).with_name('capture_worker.py')), json.dumps(spec)],
                                       stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, start_new_session=True)
            try:
                ready, _, _ = select.select([process.stdout], [], [], 5)
                reply = json.loads(process.stdout.readline()) if ready else {}
            finally:
                process.stdout.close()
            if not reply.get('ready'):
                if process.poll() is None:
                    process.terminate()
                    process.wait(timeout=25)
                raise RuntimeError(reply.get('error', 'Recorder did not start; check the Runtjik capture log'))
        else:
            args = ['grim', *(['-o', output] if output else []), *(['-g', area] if area else []), str(filename)]
            run(args, timeout=15)
            filename.chmod(0o600)
            if options['copy_to_clipboard']:
                try:
                    run(['wl-copy', '--type', 'image/png'], input=filename.read_bytes())
                except Exception as exc:
                    return self.message(f'Saved {filename}; clipboard copy failed: {exc}')
        return {'kind': 'activated', 'close_launcher': True, 'message': ('Recording started' if recording else 'Saved screenshot') + ': ' + str(filename)}


class Screenshots(CaptureProvider):
    name = 'Screenshots'
    base = '/grab'
    icon = 'camera-photo'
    commands = {'/grab': 'Take a screenshot or browse previous screenshots'}
    aliases = {'/shot': '/grab', '/screenshot': '/grab', '/ss': '/grab', '/snap': '/grab'}


class Recorder(CaptureProvider):
    name = 'Recorder'
    base = '/rec'
    icon = 'media-record'
    commands = {'/rec': 'Record screen, workspace, window or region · previous recordings'}
    aliases = {'/screenrecord': '/rec'}
