"""Recent words rendered and selected with real keyboard input; synthetic data only."""
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
    (root / 'runtjik.conf').write_text('[plugins]\nenabled=["Words"]\n[ui]\nauto_launch_commands=false\n')
    def hypr(*args):
        return subprocess.check_output(['hyprctl', *args], text=True)
    hypr('output', 'create', 'headless', 'GTK')
    hypr('eval', 'hl.monitor({output="WAYLAND-1",disabled=true}); hl.monitor({output="GTK",mode="1000x680@60",position="0x0",scale=1}); hl.dispatch(hl.dsp.focus({monitor="GTK"}))')
    time.sleep(.5)
    binary = APP / 'dist/runtjik-gtk'
    def run(action, *args):
        return subprocess.check_output([str(binary), action, *args, '--demo'], text=True)
    def status():
        return json.loads(run('status'))
    def query(text):
        run('test-query', text)
        return wait_for(lambda: (s if s['resultQuery'] == text and not s['busy'] else None) if (s := status()) else None)
    run('show')
    try:
        wait_for(lambda: status().get('focused'))
        for _ in range(3):
            run('close')
            run('show')
            wait_for(lambda: status().get('visible'))
            subprocess.run(['wtype', 'z'], check=True)
            wait_for(lambda: status()['input'] == 'z')
        for term in ['glimmer', 'monster', 'serendipity']:
            query('/define ' + term)
        s = query('/define ')
        assert [r['name'] for r in s['rows']] == ['serendipity', 'monster', 'glimmer'], s
        assert len([w for w in s['wordUi'] if w['class'] == 'sense' and w['visible']]) == 3
        subprocess.run(['wtype', '-k', 'Tab'], check=True)
        wait_for(lambda: status()['selected'] == 1)
        assert [w['name'] for w in status()['wordUi'] if w.get('selected')] == ['word-1']
        subprocess.run(['wtype', '-k', 'Return'], check=True)
        wait_for(lambda: status()['resultQuery'] == '/define monster')
        s = query('/synonym ')
        assert s['rows'][0]['name'] == 'monster'
        subprocess.run(['wtype', '-M', 'shift', '-k', 'Tab', '-m', 'shift'], check=True)
        wait_for(lambda: status()['selected'] == 2)
        subprocess.run(['wtype', '-k', 'Return'], check=True)
        wait_for(lambda: status()['resultQuery'] == '/synonym glimmer')
        query('/define ')
        subprocess.run(['wtype', '-k', 'End'], check=True)
        hypr('dismissnotify')
        time.sleep(.2)
        layer = next(c for monitor in json.loads(hypr('-j', 'layers')).values() for rows in monitor['levels'].values() for c in rows if c['namespace'] == 'runtjik-gtk')
        subprocess.run(['grim', '-g', f'{layer["x"]},{layer["y"]} {layer["w"]}x{layer["h"]}', '/tmp/runtjik-word-history.png'], check=True)
        assert not (root / 'state/runtjik/words.sqlite3').exists(), 'Demo must not persist word data'
        print('Recent words UI: visible list, Tab, Shift-Tab, Enter and mode-preserving relookup passed')
    finally:
        run('quit')
