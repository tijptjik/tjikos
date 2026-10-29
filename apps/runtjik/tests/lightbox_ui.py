"""Check initial imv geometry on a private compositor with synthetic images."""
import json
import os
from pathlib import Path
import subprocess
import sys
import time
from private_compositor import private_compositor, wait_for

app = Path(__file__).resolve().parents[1]
with private_compositor():
    temp = Path(os.environ['XDG_RUNTIME_DIR']).parent
    config = temp / 'hyprland.lua'
    with config.open('a') as stream:
        stream.write('\nhl.window_rule({name="image-viewer-lightbox",float=true,center=true,match={class="^imv$"}})\n')
    subprocess.run(['hyprctl', 'reload'], check=True, stdout=subprocess.DEVNULL)
    for name, geometry, expected in [('portrait', '600x1200', [306, 612]), ('wide', '4000x1000', [932, 612])]:
        path = temp / (name + '.png')
        subprocess.run(['magick', '-size', geometry, 'xc:pink', str(path)], check=True)
        process = subprocess.Popen([sys.executable, str(app / 'image_viewer.py'), str(path)], stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        try:
            def client():
                return next((c for c in json.loads(subprocess.check_output(['hyprctl', '-j', 'clients'])) if c['class'] == 'imv'), None)
            wait_for(lambda: client() and client()['size'] == expected)
            time.sleep(.4)
            window = client()
            assert window['floating'], window
            assert abs(window['at'][0] - (1000 - expected[0]) / 2) <= 1, window
            assert abs(window['at'][1] - 34) <= 1, window
            subprocess.run(['wtype', 'q'], check=True)
            assert process.wait(timeout=5) == 0
        finally:
            if process.poll() is None:
                subprocess.run(['hyprctl', 'dispatch', 'hl.dsp.window.close()'], stdout=subprocess.DEVNULL)
                process.wait(timeout=5)
    print('PASS: portrait and wide lightboxes, 90% height, matching margins, centered; q closes')
