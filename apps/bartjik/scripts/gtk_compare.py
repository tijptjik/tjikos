"""Workspace-scoped live GTK preview above the normal Quickshell bar.

Build through dev-check first. This interactive viewer must not hold its lock.
"""
import argparse
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import time

APP = Path(__file__).resolve().parents[1]
ROOT = Path(os.environ['XDG_RUNTIME_DIR']) / 'bartjik-gtk-comparison'
STATE = ROOT / 'session.json'
SOCKET = ROOT / 'gtk.sock'
CTL = Path.home() / '.local/bin/bartjikctl'
INSTALL = Path.home() / '.local/lib/bartjik'


def command(*args, env=None):
    return json.loads(subprocess.check_output([str(a) for a in args], env=env, text=True, timeout=8))


def legacy(action):
    return command(CTL, action)


def identity(pid):
    try:
        return Path(f'/proc/{pid}/stat').read_text().split(') ', 1)[1].split()[19]
    except FileNotFoundError:
        return None


def session():
    try:
        state = json.loads(STATE.read_text())
        return state if identity(state['pid']) == state['start'] else None
    except (FileNotFoundError, ValueError):
        return None


def write(path, value):
    encoded = json.dumps(value)
    if path.exists() and path.read_text() == encoded:
        return
    pending = path.with_suffix('.pending')
    pending.write_text(encoded)
    pending.chmod(0o600)
    pending.replace(path)


def mirror():
    snapshot = legacy('modules')
    rows = snapshot['modules']
    keep = {'id', 'type', 'icon', 'text', 'state', 'visible', 'enabled', 'tooltip', 'idle', 'artist', 'track'}
    result = []
    for module in rows:
        items = []
        for item in module.get('items', []):
            connection = item.get('panel', {}).get('connection')
            item = {key: value for key, value in item.items() if key in keep}
            if connection is not None:
                item['connection'] = connection
            for kind in ('icon', 'text', 'artist', 'track'):
                if isinstance(item.get(kind), dict):
                    item[kind] = {key: value for key, value in item[kind].items() if key != 'actions'}
            items.append(item)
        result.append({key: module[key] for key in ('id', 'bar', 'monitor')} | {'items': items, 'activity': {key: snapshot.get('native', {}).get(key) for key in ('hyprvoice', 'screenRecording')}})
    write(ROOT / 'items.json', result)


def prepare(gutter):
    current = legacy('config')
    geometry = {b['key']: b['geometry'] for b in legacy('bars')['bars']}
    monitors = {m['name']: m for m in command('hyprctl', '-j', 'monitors')}
    config = current['config']
    for bar in config['bars']:
        if bar['side'] != 'b':
            raise RuntimeError('This above/below comparison currently requires bottom-edge source bars')
        bar.update(autoHide=False, reserve=False, visible=True)
        for resolved in current['bars']:
            if resolved['id'] != bar['id'] or resolved['key'] not in geometry:
                continue
            monitor = monitors[resolved['screen']]
            scale = monitor['scale']
            height = monitor['height'] / scale
            lower = geometry[resolved['key']]
            patch = bar.setdefault('perMonitor', {}).setdefault(resolved['screen'], {})
            patch.update(autoHide=False, reserve=False, visible=True,
                         offsetY=(lower['y'] - gutter - height + resolved.get('stackOffset', 0)) * scale)
            if resolved.get('screenGutter') == 'edgeOffset':
                patch['screenGutter'] = abs(float(resolved['offsetY']))
    write(ROOT / 'config.json', config)
    mirror()


def run(gutter, binary, native=False, workspace="7"):
    old = legacy('bars')['bars']
    prepare(gutter)
    env = {**os.environ, 'BARTJIK_HOME': str(INSTALL), 'BARTJIK_CONFIG': str(ROOT / 'config.json'),
           'BARTJIK_GTK_SOCKET': str(SOCKET), 'BARTJIK_GTK_COMPARE_FILE': str(ROOT / 'items.json'),
           'TJIK_COMPOSITOR': 'hyprland', 'GTK_A11Y': 'none'}
    if workspace != "all":
        env["BARTJIK_GTK_WORKSPACE"] = workspace
    else:
        env.pop("BARTJIK_GTK_WORKSPACE", None)
    if native:
        env.pop("BARTJIK_GTK_COMPARE_FILE", None)
    child = None
    stopping = False
    def stop(*_):
        nonlocal stopping
        stopping = True
    signal.signal(signal.SIGTERM, stop)
    signal.signal(signal.SIGINT, stop)
    try:
        if workspace == 'all':
            legacy('show-reveal')
        with (ROOT / 'gtk.log').open('w') as log:
            restarts = 0
            while not stopping:
                child = subprocess.Popen(([str(binary), 'serve'] if native else ['dbus-run-session', '--', str(binary), 'serve']), env=env,
                                         stdout=log, stderr=log, start_new_session=True)
                state = {'pid': os.getpid(), 'start': identity(os.getpid()), 'child': child.pid,
                         'native': native, 'workspace': workspace, 'restarts': restarts,
                         'gutter': gutter, 'socket': str(SOCKET), 'binary': str(binary)}
                write(STATE, state)
                reveal_at = 0.0
                while not stopping and child.poll() is None:
                    if not native:
                        mirror()
                    if workspace == 'all' and time.monotonic() >= reveal_at:
                        legacy('show-reveal')
                        reveal_at = time.monotonic() + 1
                    time.sleep(.2)
                if stopping or child.returncode == 0:
                    break
                restarts += 1
                print(f'GTK exited with {child.returncode}; restarting workspace {workspace} preview ({restarts})', file=log, flush=True)
                # Bounded restart backoff; stopping stays responsive.
                for _ in range(min(50, restarts * 5)):
                    if stopping:
                        break
                    time.sleep(.1)
    finally:
        if child is not None and child.poll() is None:
            try:
                command(binary, 'quit', env=env)
                child.wait(timeout=5)
            except (subprocess.SubprocessError, OSError):
                os.killpg(child.pid, signal.SIGTERM)
                child.wait(timeout=5)
        if workspace == 'all':
            for bar_id in {b['key'].split('/', 1)[1] for b in old}:
                previous = [b for b in old if b['key'].endswith('/' + bar_id)]
                action = 'hide-reveal' if all(b['concealed'] for b in previous) else 'show'
                subprocess.run([str(CTL), action, bar_id], timeout=8, stdout=subprocess.DEVNULL, check=False)
        STATE.unlink(missing_ok=True)
        SOCKET.unlink(missing_ok=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action', choices=['start', 'status', 'stop', 'run'], nargs='?', default='start')
    parser.add_argument('--gutter', type=int, default=8)
    parser.add_argument('--binary', type=Path, default=INSTALL / 'bin/bartjik-gtk')
    parser.add_argument('--workspace', default='7', help='Show only on this workspace (default: 7); all restores the old all-workspace comparison')
    parser.add_argument('--native', action='store_true', help='Independent GTK providers and real configured actions on the session bus')
    args = parser.parse_args()
    ROOT.mkdir(mode=0o700, exist_ok=True)
    ROOT.chmod(0o700)
    active = session()
    if args.action == 'status':
        print(json.dumps(active or {'running': False}))
    elif args.action == 'stop':
        if active:
            os.kill(active['pid'], signal.SIGTERM)
            deadline = time.monotonic() + 10
            while session() and time.monotonic() < deadline:
                time.sleep(.1)
            if session():
                raise RuntimeError('Comparison did not close')
        print('Comparison closed; the existing bar remains running.')
    elif args.action == 'run':
        run(args.gutter, args.binary, args.native, args.workspace)
    elif active:
        if active.get('workspace', 'all') != args.workspace or active.get('native') != args.native:
            raise RuntimeError('A different preview mode is active; stop it before starting the requested mode')
        print(json.dumps(active))
    else:
        with (ROOT / 'supervisor.log').open('w') as log:
            subprocess.Popen([sys.executable, str(Path(__file__).resolve()), 'run', '--gutter', str(args.gutter),
                              '--binary', str(args.binary), '--workspace', args.workspace] + (['--native'] if args.native else []), stdout=log, stderr=log, start_new_session=True)
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            active = session()
            if active and SOCKET.exists():
                env = {**os.environ, 'BARTJIK_GTK_SOCKET': str(SOCKET)}
                try:
                    state = command(args.binary, 'status', env=env)
                except (subprocess.SubprocessError, OSError, ValueError):
                    time.sleep(.1)
                    continue
                ready = state.get('backendReady') if args.native else state.get('revision') is not None
                if ready and (args.workspace != 'all' or (state['bars'] and all(b['width'] > 1 for b in state['bars']))):
                    print(json.dumps(active))
                    return
            time.sleep(.2)
        raise RuntimeError(f'Comparison did not become ready; see {ROOT}/supervisor.log and gtk.log')


if __name__ == '__main__':
    main()
