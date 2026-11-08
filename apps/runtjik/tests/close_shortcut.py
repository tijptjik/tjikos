"""Close routing against real GTK focus/pointer input on an isolated display."""
import json
import os
from pathlib import Path
import subprocess
import time

from gtk_ui import APP, silent_compositor
from private_compositor import wait_for

with silent_compositor():
    root = Path(os.environ['XDG_RUNTIME_DIR']).parent
    os.environ.update(RUNTJIK_HOME=str(APP / 'dist'), RUNTJIK_CONFIG=str(root / 'runtjik.conf'),
                      XDG_STATE_HOME=str(root / 'state'), GTK_A11Y='none', GSK_RENDERER='cairo')
    (root / 'runtjik.conf').write_text('[ui]\nclose_on_outside_click=false\nauto_launch_applications=false\nauto_launch_commands=false\n')
    def hypr(*args):
        return subprocess.check_output(['hyprctl', *args], text=True)
    hypr('output', 'create', 'headless', 'GTK')
    hypr('eval', 'hl.monitor({output="WAYLAND-1",disabled=true}); hl.monitor({output="GTK",mode="1000x680@60",position="0x0",scale=1}); hl.dispatch(hl.dsp.focus({monitor="GTK"}))')
    time.sleep(.5)
    protocol = APP / 'tests/virtual_pointer.xml'
    subprocess.run(['wayland-scanner', 'client-header', str(protocol), str(root / 'pointer.h')], check=True)
    subprocess.run(['wayland-scanner', 'private-code', str(protocol), str(root / 'protocol.c')], check=True)
    flags = subprocess.check_output(['pkg-config', '--cflags', '--libs', 'wayland-client'], text=True).split()
    subprocess.run(['cc', str(APP / 'tests/pointer.c'), str(root / 'protocol.c'), '-I'+str(root), '-o', str(root / 'pointer'), *flags], check=True)
    pointer = subprocess.Popen([str(root / 'pointer'), 'GTK'], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
    def move(x, y):
        pointer.stdin.write(f'm {int(x*10)} {int(y*10000/680)}\n')
        pointer.stdin.flush()
        assert pointer.stdout.readline().strip() == 'ok'
    binary = APP / 'dist/runtjik-gtk' 
    def run(action, *args):
        return subprocess.check_output([str(binary), action, *args, '--demo'], text=True).strip()
    def status():
        return json.loads(run('status'))
    assert run('close-target') == 'false'  # No app must not start it.
    run('show')
    try:
        wait_for(lambda: status().get('focused'))
        move(5, 5)
        wait_for(lambda: not status()['hovered'])
        assert status()['focused']
        assert run('close-target') == 'true'
        assert not status()['visible']
        assert run('close-target') == 'false'
        run('show')
        wait_for(lambda: status()['visible'] and not status()['busy'])
        run('test-query', '/pin')
        wait_for(lambda: status()['resultQuery'] == '/pin' and not status()['busy'])
        run('test-activate', 'default')
        wait_for(lambda: status()['pinned'] and not status()['busy'])
        # A second real client takes focus; outside-click policy keeps the app open.
        client = subprocess.Popen(['kitty', '--class', 'close-shortcut-fixture', 'sleep', '120'], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        try:
            wait_for(lambda: not status()['focused'])
            move(5, 5)
            wait_for(lambda: not status()['hovered'])
            assert status()['visible']
            assert run('close-target') == 'false'
            assert status()['visible']
            layer = next(c for monitor in json.loads(hypr('-j', 'layers')).values() for rows in monitor['levels'].values() for c in rows if c['namespace'] == 'runtjik-gtk')
            move(layer["x"] + 100, layer["y"] + 100)
            wait_for(lambda: status()['hovered'])
            assert not status()['focused'], 'Hover should not acquire keyboard focus'
            assert run('close-target') == 'true'
            assert not status()['visible']
            assert client.poll() is None, 'Applet dismissal closed the other client'
        finally:
            client.terminate()
            client.wait(timeout=5)
    finally:
        run('quit')
        pointer.stdin.close()
        pointer.wait(timeout=5)
print('Close shortcut: absent, focused-only, hidden, neither, and hover-only passed')
