"""Read-only geometry checks for the explicitly requested same-session comparison."""
import json
import os
from pathlib import Path
import subprocess
import time

root = Path(os.environ['XDG_RUNTIME_DIR']) / 'bartjik-gtk-comparison'
session = json.loads((root / 'session.json').read_text())
env = {**os.environ, 'BARTJIK_GTK_SOCKET': session['socket']}

def read(*args, env=None):
    return json.loads(subprocess.check_output([str(a) for a in args], env=env, text=True, timeout=8))

ctl = Path.home() / '.local/bin/bartjikctl'
end = time.monotonic() + 12
while True:
    upper = read(session['binary'], 'status', env=env)
    lower = {b['key']: b for b in read(ctl, 'bars')['bars']}
    if upper['bars'] and all(b['width'] > 10 for b in upper['bars']):
        break
    assert time.monotonic() < end, upper
    time.sleep(.2)
assert upper['comparison'] == (not session.get('native', False)) and not upper['reserve']
if session.get('native'):
    assert upper['nativeRuntime'] and upper['backendReady']
assert upper['clockProviders'] == 0 and not upper['pointerSampling']
assert len(upper['bars']) == len(lower)
monitors = {m['name']: m for m in read('hyprctl', '-j', 'monitors')}
for bar in upper['bars']:
    reference = lower[bar['key']]
    actual = bar['geometry']
    expected = reference['geometry']
    gap = expected['y'] - actual['y'] - actual['height']
    assert abs(gap-session['gutter']) <= 1, (bar['key'], gap, actual, expected)
    assert abs(actual['height'] - expected['height']) <= 1, (bar['key'], 'bar height', actual, expected)
    assert abs(bar['height'] - expected['height']) <= 1, (bar['key'], 'allocated height', bar['height'], expected)
    for island in bar['islands']:
        assert island['x'] >= -1 and island['x'] + island['width'] <= bar['width'] + 1, (bar['key'], 'clipped island', island, bar['width'])
    expected_islands = [i for i in reference['islands'] if i['width'] > 0]
    assert len(bar['islands']) == len(expected_islands), (bar['key'], 'island count')
    for island, original in zip(bar['islands'], expected_islands):
        assert abs(island['height'] - original['height']) <= 1, (bar['key'], original['id'], island['height'], original['height'])
    assert bar['visible'] and reference['visible']
    assert not bar['concealed'] and not reference['concealed']
    assert actual['y'] >= 0
    output = monitors[bar['output']]
    x = int(output['x'] + min(actual['x'], expected['x']))
    y = int(output['y'] + actual['y'])
    right = int(output['x'] + max(actual['x'] + actual['width'], expected['x'] + expected['width']))
    bottom = int(output['y'] + expected['y'] + expected['height'])
    name = bar['key'].replace('/', '-') + '.png'
    subprocess.run(['grim', '-g', f'{x},{y} {right-x}x{bottom-y}', str(root / name)], check=True)
    print(f"PASS {bar['key']}: GTK above Quickshell, {actual['height']:g}px matching island height, {gap:g}px gutter; {root / name}")
print('PASS: same session, no extra edge reservation; ' + ('independent native providers' if session.get('native') else 'read-only mirror'))
