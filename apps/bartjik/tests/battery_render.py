"""Render synthetic charge states and exercise real QML AC morphs on a private bus."""

import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time

from render import wait_for

ROOT = Path(__file__).resolve().parents[1]


def main():
    assert os.environ.get('DBUS_SESSION_BUS_ADDRESS', '').startswith('unix:path=/tmp/')
    destination = Path(sys.argv[1]).resolve()
    destination.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='bartjik-battery-') as td:
        path = Path(td) / 'qml/BatteryFixture.qml'
        shutil.copytree(ROOT / 'qml', path.parent)
        path.write_text((ROOT / 'tests/BatteryFixture.qml').read_text().replace('../qml', '.'))

        def state(percent=67, external=False, capture=''):
            result = subprocess.run(['quickshell', 'ipc', '-p', str(path), 'call',
                                     'battery', 'state', str(percent), str(external).lower(), str(capture)],
                                    capture_output=True, text=True)
            if result.returncode:
                raise ValueError(result.stderr)
            return json.loads(result.stdout)

        log_path = destination / 'render.log'
        with log_path.open('w') as log:
            process = subprocess.Popen(['quickshell', '-p', str(path)], stdout=log, stderr=log)
            try:
                wait_for(lambda: state()['count'] == 7)
                for percent, count, tone in [(100, 9, 'green'), (52, 6, 'green'), (51, 5, 'yellow'), (22, 3, 'yellow'), (21, 2, 'red'), (5, 1, 'red'), (4, 9, 'red'), (0, 9, 'red')]:
                    result = state(percent)
                    assert (result['count'], result['tone']) == (count, tone), result
                    time.sleep(.1)
                    image = destination / f'battery-{percent}.png'
                    state(percent, capture=image)
                    wait_for(lambda: image.exists() and image.read_bytes().endswith(b'IEND\xaeB`\x82'))
                state(67)
                assert state(67, True)['running']
                time.sleep(.06)
                result = state(67, True, destination / 'morph.png')
                assert 0 < result['progress'] < 1, result
                # Reverse mid-flight without snapping to either endpoint.
                result = state(67, False)
                assert result['running'] and 0 < result['progress'] < 1, result
                wait_for(lambda: state()['progress'] == 0)
                state(67, True)
                wait_for(lambda: state(67, True)['progress'] == 1)
                image = destination / 'ac.png'
                state(67, True, image)
                wait_for(lambda: image.exists() and image.read_bytes().endswith(b'IEND\xaeB`\x82'))
            finally:
                process.terminate()
                process.wait(timeout=5)
        errors = [line for line in log_path.read_text().splitlines()
                  if any(word in line for word in ('WARN scene', 'ERROR', 'Binding loop', 'ReferenceError'))]
        assert not errors, errors
    print('Battery: charge and critical states, actual-size icon, AC morph and rapid reversal rendered cleanly')


if __name__ == '__main__':
    main()
