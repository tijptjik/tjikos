"""Exercise only synthetic presentation data in the requested host comparison.

Temporarily pause its mirror supervisor, restore bytes and resume in finally. No
compositor actions, provider mutations, window launches or focus changes.
"""
import copy
import json
import os
from pathlib import Path
import signal
import socket
import subprocess
import time

root = Path(os.environ['XDG_RUNTIME_DIR']) / 'bartjik-gtk-comparison'
session = json.loads((root / 'session.json').read_text())
pid = session['pid']
assert Path(f'/proc/{pid}/stat').read_text().split()[21] == session['start']
assert 'gtk_compare.py' in Path(f'/proc/{pid}/cmdline').read_text()

def status():
    with socket.socket(socket.AF_UNIX) as sock:
        sock.settimeout(3)
        sock.connect(session['socket'])
        sock.sendall(b'{"action":"status"}\n')
        return json.loads(sock.makefile().readline())

def visual(state, module, ring=False):
    for bar in state['bars']:
        if bar['output'] != 'DP-1':
            continue
        for view in bar['motion']:
            if view['id'] == module:
                for v in view['visuals']:
                    if v['ring'] == ring and (ring or v['natural'] > v['viewport'] + 1):
                        return v
    raise AssertionError(f'Missing fixture visual {module}')

def write(rows):
    pending = root / 'motion.pending'
    pending.write_text(json.dumps(rows))
    pending.replace(root / 'items.json')

def until(predicate, timeout=4):
    end = time.monotonic() + timeout
    while time.monotonic() < end:
        state = status()
        try:
            if predicate(state):
                return state
        except AssertionError:
            pass
        time.sleep(.015)
    raise AssertionError('Animation state did not arrive')

def icon(state, module, item):
    for bar in state['bars']:
        for view in bar['motion']:
            if view['id'] == module:
                for entry in view['icons']:
                    if entry['id'] == item:
                        return entry['state']
    raise AssertionError(f'Missing icon {module}/{item}')

os.kill(pid, signal.SIGSTOP)
original = (root / 'items.json').read_bytes()
try:
    rows = json.loads(original)
    for module in rows:
        module['activity'] = {'hyprvoice': 'idle', 'screenRecording': False}
        if module['id'] == 'tray':
            module['items'] = [{'id': 'chat', 'type': 'icon', 'icon': {'kind': 'image', 'value': str(Path.home() / '.local/lib/bartjik/qml/icons/tray/telegram-outline.svg')}, 'state': {'urgent': True}, 'visible': True}]
            module['activity']['hyprvoice'] = 'processing'
        elif module['id'] == 'tailnet':
            module['items'] = [{'id': 'tailnet', 'type': 'icon', 'icon': {'kind': 'symbol', 'value': 'shield'}, 'connection': 'Running', 'visible': True}]
        elif module['id'] == 'vpn':
            module['items'] = [{'id': 'vpn', 'type': 'icon', 'icon': {'kind': 'symbol', 'value': 'shield'}, 'visible': True}]
        elif module['id'] == 'notifications':
            module['items'] = [{'id': 'toggle', 'type': 'icon', 'icon': {'kind': 'symbol', 'value': 'notifications'}, 'visible': True}]
    write(rows)
    until(lambda s: icon(s, 'tray', 'chat')['urgent'] and icon(s, 'tray', '__recording')['pulsing'] and icon(s, 'vpn', 'vpn')['tailnet'])
    time.sleep(.4)  # Let tray insertion finish before inspecting pixels.
    state = status()
    assert all(not v['visible'] for b in state['bars'] for v in b['motion'] if v['id'] == 'tailnet')
    bar = next(b for b in state['bars'] if any(v['id'] == 'tray' for v in b['motion']))
    monitor = next(m for m in json.loads(subprocess.check_output(['hyprctl', '-j', 'monitors'])) if m['name'] == bar['output'])
    g = bar['geometry']
    subprocess.run(['grim', '-g', f"{int(monitor['x']+g['x'])},{int(monitor['y']+g['y'])} {int(g['width'])}x{int(g['height'])}", str(root / 'synthetic-indicators.png')], check=True)
    from PIL import Image
    pixels = list(Image.open(root / 'synthetic-indicators.png').convert('RGB').getdata())
    assert sum(r > 200 and 40 < g < 140 and 130 < b < 220 for r,g,b in pixels) >= 10, 'Unread badge not rendered'
    assert sum(r > 60 and r > g * 1.8 and r > b * 1.5 for r,g,b in pixels) >= 5, 'Recording dot not rendered'
    for module in rows:
        if module['id'] == 'notifications':
            module['items'][0]['icon']['value'] = 'notifications_off'
    write(rows)
    mid = until(lambda s: icon(s, 'notifications', 'toggle')['morphing'] and 0 < icon(s, 'notifications', 'toggle')['progress'] < 1)
    print('PASS reference icon morph intermediate progress', icon(mid, 'notifications', 'toggle')['progress'])
    until(lambda s: not icon(s, 'notifications', 'toggle')['morphing'])
    for module in rows:
        if module['id'] == 'tray':
            module['activity']['screenRecording'] = True
            module['items'][0]['state']['urgent'] = False
        elif module['id'] == 'tailnet':
            module['items'][0]['connection'] = 'Stopped'
    write(rows)
    until(lambda s: icon(s, 'tray', '__recording')['recording'] and not icon(s, 'tray', '__recording')['pulsing'] and not icon(s, 'tray', 'chat')['urgent'] and not icon(s, 'vpn', 'vpn')['tailnet'])
    print('PASS chat attention on/off, Tailscale overlay on/off, processing pulse and steady screen recording')
    for module in rows:
        module['activity'] = {'hyprvoice': 'unavailable', 'screenRecording': False}
    write(rows)
    until(lambda s: all(entry['id'] != '__recording' for bar in s['bars'] for view in bar['motion'] for entry in view['icons']))
    print('PASS recording indicator disappears on inactive/unavailable state')
    # A short activity pulse must reach GTK without waiting for a one-second poll.
    for module in rows:
        if module['id'] == 'tray':
            module['activity']['screenRecording'] = True
    started = time.monotonic()
    write(rows)
    until(lambda s: icon(s, 'tray', '__recording')['recording'], timeout=.3)
    latency = time.monotonic() - started
    time.sleep(.12)
    for module in rows:
        if module['id'] == 'tray':
            module['activity']['screenRecording'] = False
            module['items'][0]['state']['urgent'] = True
    write(rows)
    until(lambda s: icon(s, 'tray', 'chat')['urgent'], timeout=.3)
    until(lambda s: all(entry['id'] != '__recording' for bar in s['bars'] for view in bar['motion'] for entry in view['icons']))
    print(f'PASS short recording pulse delivered in {latency*1000:.1f}ms; subsequent unread update delivered promptly')

finally:
    pending = root / 'motion.pending'
    pending.write_bytes(original)
    pending.replace(root / 'items.json')
    os.kill(pid, signal.SIGCONT)
time.sleep(1.2)
