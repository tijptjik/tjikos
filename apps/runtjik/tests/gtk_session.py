"""Synthetic cold/warm grid acceptance in the current session, silently on workspace 7."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import sys
import time

if '--inside' not in sys.argv:
    raise SystemExit(subprocess.call(['dbus-run-session', '--config-file', str(Path(__file__).resolve().parents[2] / 'bartjik/tests/bus.conf'), '--', sys.executable, __file__, '--inside']))

APP = Path(__file__).resolve().parents[1]
BINARY = APP / 'dist/runtjik-gtk'


def hypr(*args):
    return subprocess.check_output(['hyprctl', *args], text=True)


def wait(predicate, seconds=15):
    until = time.monotonic() + seconds
    while time.monotonic() < until:
        result = predicate()
        if result:
            return result
        time.sleep(.05)
    raise AssertionError('Fixture readiness timed out')


with tempfile.TemporaryDirectory(prefix='runtjik-gtk-session-') as directory:
    root = Path(directory)
    config = root / 'runtjik.conf'
    config.write_text('[ui]\nauto_launch_applications=false\nauto_launch_commands=false\n')
    identity = f'acceptance-{os.getpid()}'
    title = f'Runtjik comparison GTK {identity}'
    env = {**os.environ, 'TJIK_GTK_WINDOWED': '1', 'RUNTJIK_SESSION': identity,
           'RUNTJIK_COMPARE_ID': identity, 'RUNTJIK_HOME': str(APP / 'dist'),
           'RUNTJIK_CONFIG': str(config), 'XDG_STATE_HOME': str(root / 'state'),
           'XDG_CACHE_HOME': str(root / 'cache'), 'GTK_A11Y': 'none', 'GIO_USE_VFS': 'local'}
    rule = f'runtjik_acceptance_{os.getpid()}'
    result = hypr('eval', f'_G.{rule}=hl.window_rule({{name="{rule}",workspace="7 silent",float=true,no_initial_focus=true,no_focus=true,match={{initial_title="^{title}$"}}}})')
    assert 'error' not in result.lower(), result

    def run(action, *args):
        return json.loads(subprocess.check_output([str(BINARY), action, *args, '--demo'], env=env, text=True) or '{}')

    def ready():
        state = run('status')
        return state if state.get('ready') and state.get('resultGeometry') and state.get('rows') else None

    try:
        run('show')
        state = wait(ready)
        pid = state['pid']
        client = wait(lambda: next((c for c in json.loads(hypr('-j', 'clients')) if c.get('pid') == pid), None))
        assert client['workspace']['id'] == 7, client
        assert json.loads(hypr('-j', 'activewindow')).get('address') != client['address']
        rows = state['rows']
        ids = [r['id'] for r in rows]
        columns = state['columns']
        for count in (1, 2, 3):
            rows[0]['windows'] = count
            run('test-response', json.dumps({'kind': 'results', 'query': '', 'results': rows, 'total': len(rows)}))
            state = run('status')
            assert [r['id'] for r in state['rows']] == ids
            assert state['columns'] == columns
        rows.append({**rows[-1], 'id': 'extra-fixture', 'name': 'Extra fixture'})
        run('test-response', json.dumps({'kind': 'results', 'query': '', 'results': rows, 'total': len(rows)}))
        assert [r['id'] for r in run('status')['rows']] == ids + ['extra-fixture']
        run('close')
        assert not run('status')['visible']
        run('show')
        state = wait(ready)
        assert state['pid'] == pid, 'Warm show replaced the resident UI'
        assert [r['id'] for r in state['rows']] == ids
        print('PASS: cold stable order, metadata changes, append and warm reopen on workspace 7')
    finally:
        subprocess.run([str(BINARY), 'quit', '--demo'], env=env, check=False)
        result = hypr('eval', f'if _G.{rule} then _G.{rule}:set_enabled(false); _G.{rule}=nil end')
        assert 'error' not in result.lower(), result
