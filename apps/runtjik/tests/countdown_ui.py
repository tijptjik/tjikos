"""Check the visible countdown and Escape cancellation on a private compositor."""
import json
import os
from pathlib import Path
import subprocess
import time
from private_compositor import private_compositor, wait_for

app = Path(__file__).resolve().parents[1]
with private_compositor():
    temp = Path(os.environ['XDG_RUNTIME_DIR']).parent
    monitor = json.loads(subprocess.check_output(['hyprctl', '-j', 'monitors']))[0]
    args = ['/usr/bin/python3', '-B', str(app / 'capture_countdown.py'), json.dumps({'countdown_seconds': 3, 'output': monitor['name']})]
    with (temp / 'countdown.log').open('w') as log:
        process = subprocess.Popen(args, stdout=log, stderr=log)
        try:
            wait_for(lambda: 'runtjik-countdown' in subprocess.check_output(['hyprctl', '-j', 'layers'], text=True))
            subprocess.run(['grim', '-o', monitor['name'], str(temp / 'countdown.png')], check=True)
            subprocess.run(['wtype', '-k', 'Escape'], check=True)
            assert process.wait(timeout=5) == 2
            start = time.monotonic()
            process = subprocess.Popen(args, stdout=log, stderr=log)
            assert process.wait(timeout=8) == 0
            assert time.monotonic() - start >= 2.5
            print('PASS: visible 3-second countdown and Escape cancellation', temp, flush=True)
        finally:
            if process.poll() is None:
                process.terminate()
                process.wait(timeout=5)
