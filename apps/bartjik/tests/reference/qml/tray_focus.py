"""Bring a tray app forward without toggling its existing window closed."""
import json
import re
import subprocess
import sys
import time

CLASSES = {
    'chatgpt': ('chatgpt',),
    'telegram': ('org.telegram.desktop', 'telegramdesktop', 'telegram-desktop'),
    'zapzap': ('com.rtosta.zapzap', 'zapzap'),
    'codex': ('codex', 'com.openai.codex'),
    'spotify': ('spotify', 'com.spotify.client'),
}


def select_client(app, clients):
    def matches(client):
        names = [str(client.get(k, '')).lower() for k in ('class', 'initialClass')]
        return client.get('mapped', True) and any(
            name == candidate or (app == 'telegram' and name.startswith(candidate + '._'))
            for name in names for candidate in CLASSES[app]
        )
    return min((c for c in clients if matches(c)),
               key=lambda c: c.get('focusHistoryID', 9999), default=None)


def quoted(value):
    return '"' + ''.join('\\' + c if c in '\\"' else f'\\{ord(c):03d}'
                         if ord(c) < 32 else c for c in str(value)) + '"'


def focus_commands(client, monitors):
    address = client.get('address', '')
    if not re.fullmatch(r'0x[0-9a-fA-F]+', address):
        raise ValueError('Invalid compositor window address')
    monitor = next(m for m in monitors if m['id'] == client['monitor'])
    workspace = client['workspace']['name']
    commands = ['hl.dsp.focus({ monitor = ' + quoted(monitor['name']) + ' })']
    if workspace.startswith('special:') and monitor.get('specialWorkspace', {}).get('name') != workspace:
        commands.append('hl.dsp.workspace.toggle_special(' + quoted(workspace[8:]) + ')')
    commands.append('hl.dsp.focus({ window = ' + quoted('address:' + address) + ' })')
    return [['hyprctl', 'dispatch', expression] for expression in commands]


def snapshot(kind):
    return json.loads(subprocess.check_output(['hyprctl', '-j', kind], timeout=3))


def activate(app):
    # Only used when no mapped window exists. Never toggle an existing window.
    import dbus
    bus = dbus.SessionBus()
    props = 'org.freedesktop.DBus.Properties'
    sni = 'org.kde.StatusNotifierItem'
    watcher = 'org.kde.StatusNotifierWatcher'
    names = dbus.Interface(bus.get_object(watcher, '/StatusNotifierWatcher'), props).Get(
        watcher, 'RegisteredStatusNotifierItems', timeout=2)
    for name in names:
        service, _, tail = str(name).partition('/')
        obj = bus.get_object(service, '/' + tail if tail else '/StatusNotifierItem')
        try:
            values = dbus.Interface(obj, props).GetAll(sni, byte_arrays=True, timeout=2)
        except dbus.DBusException:
            continue
        names = [str(values.get('Id', '')).lower(), str(values.get('Title', '')).lower(),
                 str(values.get('ToolTip', ('', [], '', ''))[2]).lower()]
        expected = {'chatgpt': 'chatgpt', 'telegram': 'telegramdesktop',
                    'zapzap': 'zapzap', 'codex': 'codex'}[app]
        if expected in names:
            dbus.Interface(obj, sni).Activate(0, 0, timeout=2)
            return
    raise RuntimeError('Tray app is no longer registered')


def main(app):
    if app not in CLASSES:
        raise ValueError('Unknown tray app')
    client = select_client(app, snapshot('clients'))
    if client is None and app == 'spotify':
        raise RuntimeError('Spotify window is not mapped')
    if client is None:
        activate(app)
        for _ in range(20):
            time.sleep(.1)
            client = select_client(app, snapshot('clients'))
            if client is not None:
                break
    if client is None:
        raise RuntimeError('Tray activation did not expose a window')
    for command in focus_commands(client, snapshot('monitors')):
        subprocess.run(command, check=True, capture_output=True, timeout=3)
    for _ in range(10):
        if snapshot('activewindow').get('address') == client['address']:
            return
        time.sleep(.05)
    raise RuntimeError('Compositor did not focus the requested app')


if __name__ == '__main__':
    try:
        main(sys.argv[1])
    except (ValueError, RuntimeError, OSError, subprocess.SubprocessError, StopIteration) as error:
        print('Tray focus: ' + str(error), file=sys.stderr)
        sys.exit(1)
