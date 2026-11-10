"""Render only synthetic card primitives in an ordinary window on workspace 7.

No nested compositor, host pointer movement, or provider/device actions.
"""
import json
import os
from pathlib import Path
import subprocess
import shutil
import tempfile
import time
import sys

APP = Path(__file__).resolve().parents[1]
TRANSITION = '--transition' in sys.argv
CODEX = '--codex' in sys.argv
ROUTER = '--openrouter' in sys.argv
NETWORK = '--network' in sys.argv
VPN = '--connectivity' in sys.argv
HISTORY = '--history' in sys.argv
LOCALE = '--locale' in sys.argv
AUDIO = '--audio' in sys.argv
SYSTEM = next((p for p in ['caffeine', 'nightlight', 'monitor-layout'] if '--'+p in sys.argv), None)
LEFT = next((p for p in ['crypto', 'playerctl', 'workspaces'] if '--'+p in sys.argv), None)
STEM = LEFT if LEFT else SYSTEM if SYSTEM else 'history' if HISTORY else 'locale' if LOCALE else 'audio' if AUDIO else 'connectivity' if VPN else 'network' if NETWORK else 'openrouter' if ROUTER else 'codex' if CODEX else 'panel'


def hypr(*args):
    return subprocess.check_output(['hyprctl', *args], text=True)

rule = f'bartjik_panel_fixture_{os.getpid()}'
result = hypr('eval', f'_G.{rule}=hl.window_rule({{name="{rule}",workspace="7 silent",float=true,no_initial_focus=true,no_focus=true,match={{initial_title="^Bartjik Panel Fixture$"}}}})')
assert 'error' not in result.lower(), result
try:
    env = {**os.environ, 'BARTJIK_PANEL_ARTIFACT': f'/tmp/bartjik-{STEM}-primitives.png', 'BARTJIK_HOME': str(APP / 'dist'), 'GTK_A11Y': 'none', 'PKG_CONFIG_PATH': str(APP / '../../packages/tjik-gtk/target/native/lib/pkgconfig')}
    if '--empty' in sys.argv:
        env['BARTJIK_EMPTY_USAGE'] = '1'
        if LEFT in ['crypto', 'playerctl']:
            env['BARTJIK_PANEL_ARTIFACT'] = f'/tmp/bartjik-{LEFT}-loading-primitives.png'
    if CODEX or ROUTER or NETWORK or VPN or AUDIO or HISTORY or LOCALE or SYSTEM or LEFT:
        env['BARTJIK_CARD_FIXTURE'] = STEM
    process = subprocess.Popen(['cargo', 'test', '--release', '--locked', '--features', 'gtk-ui', '--bin', 'bartjik-gtk', ('panel_reveal_render' if TRANSITION else 'panel_primitives_render'), '--', '--ignored', '--nocapture'], cwd=APP, env=env)
    placed = False
    while process.poll() is None:
        clients = json.loads(hypr('-j', 'clients'))
        for client in clients:
            if client['title'] == 'Bartjik Panel Fixture':
                assert client['workspace']['id'] == 7, client
                assert json.loads(hypr('-j', 'activewindow')).get('address') != client['address'], 'Fixture stole focus'
                placed = True
        time.sleep(.1)
    assert process.returncode == 0, process.returncode
    assert placed, 'Fixture placement was not observed'
    if HISTORY or LOCALE or LEFT:
        print(f'PASS: synthetic {STEM} render and interactions on workspace 7')
        sys.exit(0)
    if TRANSITION:
        print('PASS: fixed destination allocation, clipping, shrink, interrupted/equal-size handoff; /tmp/bartjik-panel-transition-mid.png')
        sys.exit(0)
    print('PASS: rendered synthetic primitives on workspace 7 without focus; ' + env['BARTJIK_PANEL_ARTIFACT'])
finally:
    result = hypr('eval', f'if _G.{rule} then _G.{rule}:set_enabled(false); _G.{rule}=nil end')
    assert 'error' not in result.lower(), result
