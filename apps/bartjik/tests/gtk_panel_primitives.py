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
ROUTER = False
NETWORK = '--network' in sys.argv
VPN = False
HISTORY = False
LOCALE = False
AUDIO = False
STEM = 'history' if HISTORY else 'locale' if LOCALE else 'audio' if AUDIO else 'connectivity' if VPN else 'network' if NETWORK else 'openrouter' if ROUTER else 'codex' if CODEX else 'panel'


def hypr(*args):
    return subprocess.check_output(['hyprctl', *args], text=True)

rule = f'bartjik_panel_fixture_{os.getpid()}'
result = hypr('eval', f'_G.{rule}=hl.window_rule({{name="{rule}",workspace="7 silent",float=true,no_initial_focus=true,no_focus=true,match={{initial_title="^Bartjik Panel Fixture$"}}}})')
assert 'error' not in result.lower(), result
try:
    env = {**os.environ, 'BARTJIK_PANEL_ARTIFACT': f'/tmp/bartjik-{STEM}-primitives.png', 'BARTJIK_HOME': str(APP / 'dist'), 'GTK_A11Y': 'none', 'PKG_CONFIG_PATH': str(APP / '../../packages/tjik-gtk/target/native/lib/pkgconfig')}
    if '--empty' in sys.argv:
        env['BARTJIK_EMPTY_USAGE'] = '1'
    if CODEX or ROUTER or NETWORK or VPN or AUDIO or HISTORY or LOCALE:
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
    if HISTORY or LOCALE:
        print(f'PASS: synthetic {STEM} render and interactions on workspace 7')
        sys.exit(0)
    if TRANSITION:
        print('PASS: fixed destination allocation, clipping, shrink, interrupted/equal-size handoff; /tmp/bartjik-panel-transition-mid.png')
        sys.exit(0)
    with tempfile.TemporaryDirectory(prefix='bartjik-panel-reference-') as directory:
        root = Path(directory)
        components = root / 'components'
        components.mkdir()
        for name in ['PanelHeader', 'PanelHandle', 'PanelButton', 'PanelToolTip', 'StatusDetails', 'BatteryDetails', 'CodexDetails', 'OpenRouterDetails', 'NetworkDetails', 'NetworkConnection', 'NetworkTraffic', 'NetworkAddresses', 'MullvadLocations', 'CountryButton', 'AudioDetails', 'AudioGroup']:
            shutil.copy2(APP / 'qml' / (name + '.qml'), components)
        shutil.copy2(APP / 'qml/CodexTimeline.js', components)
        (components / 'icons').mkdir()
        shutil.copy2(APP / ('qml/icons/' + ('openrouter' if ROUTER else 'codex') + '.svg'), components / 'icons')
        source = (APP / 'tests' / ('AudioCard.qml' if AUDIO else 'ConnectivityCard.qml' if VPN else 'NetworkCard.qml' if NETWORK else 'OpenRouterCard.qml' if ROUTER else 'CodexCard.qml' if CODEX else 'PanelPrimitives.qml')).read_text().replace('../qml', 'components')
        row = json.loads((APP / ('tests/fixtures/' + ('audio' if AUDIO else 'connectivity' if VPN else 'network' if NETWORK else 'openrouter' if ROUTER else 'codex') + '-card.json')).read_text())
        if ROUTER and '--empty' in sys.argv:
            row['panel']['rows'] = []
            row['state']['stale'] = False
            row['panel']['activityNote'] = 'Daily usage isn’t available for this key.'
        source = source.replace('FIXTURE_ROW', json.dumps(row))
        if VPN:
            shutil.copytree(APP / 'qml/icons/flags', components / 'icons/flags')
            (components / 'icons/tray').mkdir()
            shutil.copy2(APP / 'qml/icons/tray/mullvad.svg', components / 'icons/tray')
            shutil.copy2(APP / 'qml/icons/tailscale.svg', components / 'icons')
        if AUDIO:
            (components / "icons/proicons").mkdir(parents=True, exist_ok=True)
            shutil.copy2(APP / "qml/icons/proicons/volume.svg", components / "icons/proicons")
        if NETWORK:
            (components / 'icons/proicons').mkdir(parents=True)
            shutil.copy2(APP / 'qml/icons/proicons/wi-fi.svg', components / 'icons/proicons')
        (root / 'shell.qml').write_text(source)
        artifact = Path(f'/tmp/bartjik-{STEM}-reference.png')
        artifact.unlink(missing_ok=True)
        reference = subprocess.Popen(['quickshell', '-p', str(root / 'shell.qml')], env={**os.environ, 'QT_QPA_PLATFORM': 'offscreen', 'QT_QUICK_BACKEND': 'software'})
        try:
            reference.wait(timeout=15)
            assert reference.returncode == 0
            assert artifact.exists()
        finally:
            if reference.poll() is None:
                reference.terminate()
                reference.wait(timeout=5)
    print('PASS: rendered synthetic primitives on workspace 7 without focus; ' + env['BARTJIK_PANEL_ARTIFACT'])
finally:
    result = hypr('eval', f'if _G.{rule} then _G.{rule}:set_enabled(false); _G.{rule}=nil end')
    assert 'error' not in result.lower(), result
