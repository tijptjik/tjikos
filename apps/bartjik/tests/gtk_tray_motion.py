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

def tray(state):
    return next(v['tray'] for b in state['bars'] for v in b['motion'] if v['id'] == 'tray')

def frames(predicate):
    samples = []
    end = time.monotonic() + 4
    started = None
    while time.monotonic() < end:
        state = status()
        if predicate(state) and started is None:
            started = time.monotonic()
        if started is not None:
            samples.append({'elapsed': time.monotonic()-started, 'tray': tray(state),
                            'dots': icon(state, 'vpn', 'vpn')['dots'],
                            'width': next(b['width'] for b in state['bars'] if any(v['id'] == 'tray' for v in b['motion']))})
            if time.monotonic()-started > .5:
                return samples
        time.sleep(.01)
    raise AssertionError('No animated fixture update')

os.kill(pid, signal.SIGSTOP)
original = (root / 'items.json').read_bytes()
try:
    rows = json.loads(original)
    chat = {'id': 'chat-one', 'type': 'icon', 'icon': {'kind': 'image', 'value': str(Path.home() / '.local/lib/bartjik/qml/icons/tray/telegram-outline.svg')}, 'state': {}, 'visible': True}
    for module in rows:
        module['activity'] = {'hyprvoice': 'idle', 'screenRecording': False}
        if module['id'] == 'tray':
            module['items'] = [chat]
        elif module['id'] == 'vpn':
            module['items'] = [{'id': 'vpn', 'type': 'icon', 'icon': {'kind': 'symbol', 'value': 'vpn_lock'}, 'visible': True}]
        elif module['id'] == 'tailnet':
            module['items'] = [{'id': 'tailnet', 'type': 'icon', 'icon': {'kind': 'symbol', 'value': 'vpn_lock'}, 'connection': 'Stopped', 'visible': True}]
    write(rows)
    until(lambda s: [v['id'] for v in tray(s)['slots']] == ['chat-one'] and not tray(s)['moving'] and icon(s, 'vpn', 'vpn')['dots'] == [0,0,0])
    baseline = tray(status())['span']
    for module in rows:
        if module['id'] == 'tailnet':
            module['items'][0]['connection'] = 'Running'
        elif module['id'] == 'tray':
            module['activity']['screenRecording'] = True
            second = copy.deepcopy(chat)
            second['id'] = 'chat-two'
            module['items'].append(second)
    write(rows)
    entering = frames(lambda s: icon(s, 'vpn', 'vpn')['tailnet'] and len(tray(s)['slots']) == 3)
    full = entering[-1]['tray']['span']
    assert any(baseline + 1 < f['tray']['span'] < full - 1 for f in entering), entering
    assert any(f['dots'][0] > f['dots'][1] > f['dots'][2] for f in entering), entering
    assert entering[-1]['dots'] == [1,1,1]
    assert entering[-1]['tray']['moving'] is False
    for module in rows:
        if module['id'] == 'tailnet':
            module['items'][0]['connection'] = 'Stopped'
        elif module['id'] == 'tray':
            module['activity']['screenRecording'] = False
            module['items'] = [chat]
    write(rows)
    leaving = frames(lambda s: not icon(s, 'vpn', 'vpn')['tailnet'])
    assert any(baseline + 1 < f['tray']['span'] < full - 1 for f in leaving), leaving
    assert any(f['dots'][2] < f['dots'][1] < f['dots'][0] for f in leaving), leaving
    assert leaving[-1]['dots'] == [0,0,0]
    assert abs(leaving[-1]['tray']['span']-baseline) < .01
    assert [v['id'] for v in leaving[-1]['tray']['slots']] == ['chat-one']
    assert not leaving[-1]['tray']['slots'][0]['state']['ticking']
    (root / 'tray-motion-evidence.json').write_text(json.dumps({'entering':entering,'leaving':leaving},indent=2))
    print(f'PASS ordered dots on/off; tray grows {baseline:.0f} -> {full:.0f}px and collapses to {baseline:.0f}px with intermediate frames; removed icons released and idle ticks stopped')
finally:
    pending = root / 'motion.pending'
    pending.write_bytes(original)
    pending.replace(root / 'items.json')
    os.kill(pid, signal.SIGCONT)
time.sleep(1.5)
