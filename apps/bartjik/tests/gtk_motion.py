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

os.kill(pid, signal.SIGSTOP)
original = (root / 'items.json').read_bytes()
try:
    rows = json.loads(original)
    for module in rows:
        if module['id'] == 'spaces':
            module['items'] = [{'id': str(i), 'type': 'text', 'text': {'value': str(i)},
                                'state': {'active': i == 1}, 'visible': True}
                               for i in range(1, 5)]
        elif module['id'] == 'music':
            module['items'] = [{'id': 'fixture', 'type': 'icon_text', 'idle': False,
                                'artist': {'value': 'Synthetic artist with a deliberately long name'},
                                'track': {'value': 'Synthetic track with a deliberately long title for ticker verification'},
                                'icon': {'kind': 'symbol', 'value': 'music_note'}, 'visible': True}]
        else:
            module['items'] = []
    write(rows)
    until(lambda s: visual(s, 'spaces', True)['target'] == 16 and visual(s, 'music')['ticking'])
    time.sleep(.3)
    moved = copy.deepcopy(rows)
    for module in moved:
        if module['id'] == 'spaces':
            for item in module['items']:
                item['state']['active'] = item['id'] == '4'
    write(moved)
    mid = until(lambda s: 16 < visual(s, 'spaces', True)['position'] < 123.9)
    assert visual(mid, 'spaces', True)['target'] == 124
    until(lambda s: abs(visual(s, 'spaces', True)['position'] - 124) < .01)
    first = until(lambda s: visual(s, 'music')['offset'] < -5)
    before = visual(first, 'music')['offset']
    # Tooltip and playback-icon refreshes must not restart unchanged text.
    for module in moved:
        for item in module['items']:
            item['tooltip'] = 'Synthetic updated tooltip'
        if module['id'] == 'music':
            module['items'][0]['icon'] = {'kind': 'symbol', 'value': 'pause'}
    write(moved)
    time.sleep(1.1)
    after = visual(status(), 'music')['offset']
    assert after < before - 20, (before, after)
    print(f'PASS ring interpolated {visual(mid, "spaces", True)["position"]:.2f} -> 124; ticker {before:.2f} -> {after:.2f} survived update')
finally:
    pending = root / 'motion.pending'
    pending.write_bytes(original)
    pending.replace(root / 'items.json')
    os.kill(pid, signal.SIGCONT)

# Let the renderer consume the restored mirror before subsequent screenshots.
time.sleep(1.2)
