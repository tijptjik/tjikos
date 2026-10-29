"""Exercise real slurp drag selection while the backend's stdin remains open."""
import json
import os
from pathlib import Path
import shlex
import subprocess
import sys
from private_compositor import private_compositor, wait_for

app = Path(__file__).resolve().parents[1]
with private_compositor():
    temp = Path(os.environ['XDG_RUNTIME_DIR']).parent
    config = temp / 'runtjik.conf'
    config.write_text('[plugin_settings.Screenshots]\ndirectory=' + json.dumps(str(temp / 'screenshots')) + '\ncopy_to_clipboard=false\n')
    env = {**os.environ, 'RUNTJIK_CONFIG': str(config), 'PYTHONPATH': str(app), 'RUNTJIK_DEMO': '1', 'RUNTJIK_HOME': str(app)}
    pointer = temp / 'pointer'
    subprocess.run(['wayland-scanner', 'client-header', str(app/'tests/virtual_pointer.xml'), str(temp/'pointer.h')], check=True)
    subprocess.run(['wayland-scanner', 'private-code', str(app/'tests/virtual_pointer.xml'), str(temp/'protocol.c')], check=True)
    flags = shlex.split(subprocess.check_output(['pkg-config', '--cflags', '--libs', 'wayland-client'], text=True))
    subprocess.run(['cc', str(app/'tests/pointer.c'), str(temp/'protocol.c'), '-I'+str(temp), '-o', str(pointer), *flags], check=True)
    monitor = json.loads(subprocess.check_output(['hyprctl', '-j', 'monitors']))[0]
    ptr = subprocess.Popen([str(pointer), monitor['name']], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
    with (temp/'launcher.log').open('w') as log:
        launcher = subprocess.Popen(['quickshell', '--path', str(app/'qml'), '--no-duplicate'], env=env, stdout=log, stderr=log)
        helper = None
        try:
            wait_for(lambda: 'runtjik' in subprocess.check_output(['hyprctl','-j','layers'], text=True))
            code = "import json,runtjik,desktop_plugins as d;print(json.dumps(d.Screenshots().capture({'capture':'region'},runtjik.config()['plugin_settings']['Screenshots'])),flush=True)"
            helper = subprocess.Popen([sys.executable, '-B', '-c', code], env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
            wait_for(lambda: any(name in subprocess.check_output(['hyprctl', '-j', 'layers'], text=True) for name in ('slurp', 'selection')))
            def send(command):
                ptr.stdin.write(command+'\n');ptr.stdin.flush();assert ptr.stdout.readline().strip()=='ok'
            send('m 2000 2000');send('b 1');send('m 6000 6000');send('b 0')
            assert helper.wait(timeout=10)==0,helper.stderr.read()
            result=json.loads(helper.stdout.readline())
            assert result['kind']=='activated'
            from PIL import Image
            image=next((temp/'screenshots').glob('*.png'))
            with Image.open(image) as shot: assert shot.width>10 and shot.height>10
            print('PASS: real region drag screenshot with persistent stdin', temp, flush=True)
        finally:
            if helper:
                if helper.poll() is None:helper.terminate();helper.wait(timeout=5)
                helper.stdin.close();helper.stdout.close();helper.stderr.close()
            ptr.stdin.close();ptr.wait(timeout=5)
            launcher.terminate();launcher.wait(timeout=5)
