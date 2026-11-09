"""Expansion editor keyboard flow on synthetic fixtures, silently on workspace 7."""
import json
import os
from pathlib import Path
import subprocess
import time
from gtk_ui import APP, silent_compositor, wait_for

with silent_compositor():
    root = Path(os.environ['XDG_RUNTIME_DIR']).parent
    os.environ.update(RUNTJIK_HOME=str(APP), RUNTJIK_CONFIG=str(root / 'expand.conf'),
                      XDG_STATE_HOME=str(root / 'state'), GTK_A11Y='none', GSK_RENDERER='cairo')
    (root / 'expand.conf').write_text('[theme]\nicon_theme="Adwaita"\n[ui]\nauto_launch_commands=false\n')
    def hypr(*args):
        return subprocess.check_output(['hyprctl', *args], text=True)
    hypr('output', 'create', 'headless', 'EXPAND')
    hypr('eval', 'hl.monitor({output="WAYLAND-1",disabled=true}); hl.monitor({output="EXPAND",mode="1000x720@60",position="0x0",scale=1}); hl.dispatch(hl.dsp.focus({monitor="EXPAND"}))')
    time.sleep(.4)
    hypr('eval', 'hl.monitor({output="EXPAND",mode="1000x720@60",position="0x0",scale=1}); hl.dispatch(hl.dsp.focus({monitor="EXPAND"}))')
    def run(action, text=None):
        args = [str(APP / 'runtjik-gtk'), action]
        if text is not None:
            args.append(text)
        return subprocess.check_output([*args, '--demo'], text=True)
    def status():
        return json.loads(run('status'))
    def settled(query):
        return wait_for(lambda: (s if s['query'] == query and not s['busy'] else None) if (s := status()) else None)
    def key(*args):
        subprocess.run(['wtype', *args], check=True)
        time.sleep(.15)
    try:
        run('show')
        wait_for(lambda: status().get('ready'))
        run('test-query', '/expand')
        rows = settled('/expand')['rows']
        assert [r['name'] for r in rows] == ['New', ';hello', ';thanks'], rows
        subprocess.run(['grim', '-o', 'EXPAND', '/tmp/runtjik-expand-list.png'], check=True)
        key('-k', 'Up', '-k', 'Up', '-k', 'Return')
        wait_for(lambda: status()['content_view'] == 'expansion-editor')
        key(';test', '-k', 'Tab', 'Synthetic multiline expansion', '-k', 'Return', 'Second line')
        subprocess.run(['grim', '-o', 'EXPAND', '/tmp/runtjik-expand-editor.png'], check=True)
        # Click the visible Save action with a pointer bound to the private output.
        subprocess.run(['wayland-scanner', 'client-header', str(APP / 'tests/virtual_pointer.xml'), str(root / 'pointer.h')], check=True)
        subprocess.run(['wayland-scanner', 'private-code', str(APP / 'tests/virtual_pointer.xml'), str(root / 'protocol.c')], check=True)
        flags = subprocess.check_output(['pkg-config', '--cflags', '--libs', 'wayland-client'], text=True).split()
        subprocess.run(['cc', str(APP / 'tests/pointer.c'), str(root / 'protocol.c'), '-I' + str(root), '-o', str(root / 'pointer'), *flags], check=True)
        layer = next(c for monitor in json.loads(hypr('-j', 'layers')).values() for rows in monitor['levels'].values() for c in rows if c['namespace'] == 'runtjik-gtk')
        pointer = subprocess.Popen([str(root / 'pointer'), 'EXPAND'], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
        try:
            for event in [f"m {int((layer['x'] + layer['w'] - 144) * 10)} {int((layer['y'] + 54) * 10000 / 720)}", 'b 1', 'b 0']:
                pointer.stdin.write(event + '\n')
                pointer.stdin.flush()
                assert pointer.stdout.readline().strip() == 'ok'
                time.sleep(.1)
        finally:
            pointer.stdin.close()
            pointer.wait(timeout=5)
        wait_for(lambda: 'Demo mode' in status()['message'])
        time.sleep(.3)
        subprocess.run(['grim', '-o', 'EXPAND', '/tmp/runtjik-expand-message.png'], check=True)
        key('-k', 'Escape')
        wait_for(lambda: status()['content_view'] == 'launcher')
        run('test-query', '/expand WORLD')
        assert [r['name'] for r in settled('/expand WORLD')['rows']] == ['New', ';hello']
        key('-k', 'Down', '-k', 'Return')
        wait_for(lambda: status()['content_view'] == 'expansion-editor')
        key('-k', 'Escape')
        wait_for(lambda: status()['content_view'] == 'launcher')
        if status()['selected'] == 0:
            key('-k', 'Down')
        assert status()['rows'][status()['selected']]['name'] == ';hello'
        key('-M', 'ctrl', '-k', 'Return', '-m', 'ctrl')
        wait_for(lambda: 'Demo mode' in status()['message'])
        assert status()['content_view'] == 'launcher'
        print('PASS: rendered expansion list/editor, Enter, multiline input, Save, Escape, filter and Ctrl+Enter; synthetic workspace 7 screenshots in /tmp/runtjik-expand-*.png')
    except Exception:
        print('UI failure state:', status(), flush=True)
        subprocess.run(['grim', '-o', 'EXPAND', '/tmp/runtjik-expand-failure.png'])
        raise
    finally:
        run('quit')
