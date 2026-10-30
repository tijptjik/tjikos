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
    success = temp / 'countdown-complete'
    args = ['quickshell', '--path', str(app / 'qml/countdown')]
    env = {**os.environ, 'RUNTJIK_COUNTDOWN_SECONDS':'3', 'RUNTJIK_COUNTDOWN_OUTPUT':monitor['name'], 'RUNTJIK_COUNTDOWN_SUCCESS':str(success)}
    with (temp / 'countdown.log').open('w') as log:
        process = subprocess.Popen(args, stdout=log, stderr=log, env=env)
        try:
            wait_for(lambda: 'runtjik-countdown' in subprocess.check_output(['hyprctl', '-j', 'layers'], text=True))
            subprocess.run(['grim', '-o', monitor['name'], str(temp / 'countdown.png')], check=True)
            subprocess.run(['wtype', '-k', 'Escape'], check=True)
            assert process.wait(timeout=5) == 0
            assert not success.exists()
            start = time.monotonic()
            process = subprocess.Popen(args, stdout=log, stderr=log, env=env)
            assert process.wait(timeout=8) == 0
            assert success.exists()
            assert time.monotonic() - start >= 2.5
            print('PASS: visible 3-second countdown and Escape cancellation', temp, flush=True)
        finally:
            if process.poll() is None:
                process.terminate()
                process.wait(timeout=5)
