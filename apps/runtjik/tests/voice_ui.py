"""Render voice choices and exercise keyboard actions using synthetic history."""
import json
import os
from pathlib import Path
import subprocess
import time
from gtk_ui import APP, silent_compositor
from private_compositor import wait_for


def main():
    with silent_compositor():
        root = Path(os.environ['XDG_RUNTIME_DIR']).parent
        os.environ.update(RUNTJIK_HOME=str(APP / 'dist'), RUNTJIK_CONFIG=str(root / 'voice.conf'),
                          XDG_STATE_HOME=str(root / 'state'), GTK_A11Y='none', GSK_RENDERER='cairo')
        (root / 'voice.conf').write_text('[theme]\nicon_theme="Adwaita"\n[ui]\nauto_launch_applications=false\nauto_launch_commands=false\n')
        signature = os.environ['HYPRLAND_INSTANCE_SIGNATURE']
        def hypr(*args):
            return subprocess.check_output(['hyprctl', '--instance', signature, *args], text=True, timeout=10)
        hypr('output', 'create', 'headless', 'VOICE')
        hypr('eval', 'hl.monitor({output="WAYLAND-1",disabled=true}); hl.monitor({output="VOICE",mode="1200x900@60",position="0x0",scale=1}); hl.dispatch(hl.dsp.focus({monitor="VOICE"}))')
        binary = APP / 'dist/runtjik-gtk'
        def run(*args):
            return subprocess.check_output([str(binary), *args, '--demo'], text=True, timeout=10)
        def status():
            return json.loads(run('status'))
        def settled(q):
            return wait_for(lambda: (s if s.get('query') == q and s.get('resultQuery') == q and not s.get('busy') else None) if (s := status()) else None)
        process = subprocess.Popen([str(binary), 'serve', '--demo'], stdout=subprocess.DEVNULL)
        try:
            wait_for(lambda: status().get('ready') and not status().get('startup'))
            run('test-query', '/voice')
            state = settled('/voice')
            assert [r['name'] for r in state['rows']] == ['Voice Record', 'Voice History'], state
            assert state['columns'] == 2, state
            time.sleep(.5)
            subprocess.run(['grim', '-o', 'VOICE', '/tmp/runtjik-voice-menu.png'], check=True, timeout=10)
            subprocess.run(['wtype', '-k', 'Right', '-k', 'Return'], check=True, timeout=10)
            state = settled('/voice history')
            assert len(state['rows']) == 2, state
            subprocess.run(['wtype', '-k', 'Return'], check=True, timeout=10)
            wait_for(lambda: status().get('message') == 'Copied voice text')
            subprocess.run(['wtype', '-M', 'ctrl', '-k', 'Return', '-m', 'ctrl'], check=True, timeout=10)
            wait_for(lambda: status().get('message') == 'Removed from voice history')
            subprocess.run(['grim', '-o', 'VOICE', '/tmp/runtjik-voice-history.png'], check=True, timeout=10)
            run('test-query', '/voice')
            settled('/voice')
            time.sleep(.2)
            cards = [w for w in status()['captureUi'] if w['visible']]
            assert len(cards) == 2 and abs(cards[0]['y'] - cards[1]['y']) < 1, cards
            layer = next(c for monitor in json.loads(hypr('-j', 'layers')).values()
                         for rows in monitor['levels'].values() for c in rows if c['namespace'] == 'runtjik-gtk')
            protocol = APP / 'tests/virtual_pointer.xml'
            subprocess.run(['wayland-scanner', 'client-header', str(protocol), str(root / 'pointer.h')], check=True)
            subprocess.run(['wayland-scanner', 'private-code', str(protocol), str(root / 'protocol.c')], check=True)
            flags = subprocess.check_output(['pkg-config', '--cflags', '--libs', 'wayland-client'], text=True).split()
            subprocess.run(['cc', str(APP / 'tests/pointer.c'), str(root / 'protocol.c'), '-I'+str(root), '-o', str(root / 'pointer'), *flags], check=True)
            x = int((layer['x'] + cards[0]['x'] + cards[0]['width'] - 30) / 1200 * 10000)
            y = int((layer['y'] + cards[0]['y'] + 27) / 900 * 10000)
            subprocess.run([str(root / 'pointer'), 'VOICE'], input=f'm {x} {y}\nb 1\nb 0\n', text=True, check=True, timeout=10, stdout=subprocess.DEVNULL)
            wait_for(lambda: status().get('lastActivation') == 'Voice:record' and not status().get('busy'))
            subprocess.run(['wtype', '-M', 'shift', '-k', 'Return', '-m', 'shift'], check=True, timeout=10)
            wait_for(lambda: not status().get('busy'))
            assert not status().get('backend_error'), status()
            print('Voice GTK layout and keyboard actions passed')
        finally:
            run('quit')
            process.wait(timeout=10)


if __name__ == '__main__':
    main()
