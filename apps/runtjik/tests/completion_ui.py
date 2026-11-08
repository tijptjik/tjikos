"""Real typing through automatic command completion on an isolated GTK display."""
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
    (root / 'runtjik.conf').write_text('[plugins]\nenabled=["Words"]\n[ui]\nauto_launch_applications=false\nauto_launch_commands=true\n')
    def hypr(*args):
        return subprocess.check_output(['hyprctl', *args], text=True)
    hypr('output', 'create', 'headless', 'GTK')
    hypr('eval', 'hl.monitor({output="WAYLAND-1",disabled=true}); hl.monitor({output="GTK",mode="1000x680@60",position="0x0",scale=1}); hl.dispatch(hl.dsp.focus({monitor="GTK"}))')
    time.sleep(.5)
    binary = APP / 'dist/runtjik-gtk'
    def run(action, *args):
        return subprocess.check_output([str(binary), action, *args, '--demo'], text=True).strip()
    def status():
        return json.loads(run('status'))
    def type_text(text):
        subprocess.run(['wtype', text], check=True)
    def begin():
        run('quit')
        wait_for(lambda: not status().get('running'))
        run('show')
        wait_for(lambda: status().get('focused') and not status()['busy'])
        started = time.monotonic()
        type_text('/d')
        wait_for(lambda: status()['query'] == '/define ')
        assert time.monotonic() - started < .45, 'Completion fixture exceeded the typing guard window'
    try:
        begin()
        type_text('ef')
        assert status()['query'] == '/define ', status()
        time.sleep(.55)
        type_text('i')
        wait_for(lambda: status()['query'] == '/define i')
        begin()
        type_text('monster')
        wait_for(lambda: status()['query'] == '/define monster')
        begin()
        type_text('efine monster')
        wait_for(lambda: status()['query'] == '/define monster')
        print('Completion: fast tail, 500 ms idle release, mismatch and complete spelling passed')
    finally:
        run('quit')
