"""Real copy shortcuts, clipboard bytes, acknowledgement and dismissal in private Wayland."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import struct
import time
import zlib

from private_compositor import private_compositor, wait_for

app = Path(os.environ.get('RUNTJIK_TEST_HOME', str(Path(__file__).resolve().parents[1])))
with private_compositor(isolate_pointer=True):
    subprocess.run(['hyprctl', 'output', 'create', 'headless', 'COPY'], check=True, stdout=subprocess.DEVNULL)
    wait_for(lambda: any(m['name'] == 'COPY' for m in json.loads(subprocess.check_output(['hyprctl', '-j', 'monitors'], text=True))))
    subprocess.run(['hyprctl', 'eval', 'hl.monitor({output="COPY",mode="1000x680@60",position="-3000x0",scale=1})'], check=True, stdout=subprocess.DEVNULL)
    subprocess.run(['hyprctl', 'eval', 'hl.dispatch(hl.dsp.focus({monitor="COPY"}))'], check=True, stdout=subprocess.DEVNULL)
    temp = Path(os.environ['XDG_RUNTIME_DIR']).parent
    home = temp / 'app'
    shutil.copytree(app / 'qml', home / 'qml')
    shutil.copy2(app / 'runtjik-core', home / 'runtjik-core')
    # Test-only controls in a temporary runtime; every capture is synthetic.
    shell = home / 'qml/shell.qml'
    shell.write_text(shell.read_text().replace('function testState(): string {', '''
        function copyState(): string {
            const tile=items.itemAtIndex(root.selected);
            const badge=tile?.children.find(item=>item.objectName==='copySuccessBadge');
            return JSON.stringify({query:root.resultQuery,copied:root.copiedId,
                visible:root.opened && panel.visible,busy:root.busy,message:root.message,
                pending:root.providerPending || debounce.running || root.resultQuery!==root.query,
                focus:searchBox.activeFocus,selection:searchBox.selectedText,input:searchBox.text,
                rows:root.results,opacity:badge?.opacity || 0,scale:badge?.scale || 0});
        }
        function copyQuery(query: string): void {searchBox.text=query;}
        function copyPreview(path: string): void {frame.grabToImage(image=>image.saveToFile(path));}
        function testState(): string {'''))
    captures = temp / 'captures'
    captures.mkdir()
    png = captures / 'Synthetic image.png'
    def chunk(kind, data):
        return struct.pack('!I', len(data)) + kind + data + struct.pack('!I', zlib.crc32(kind + data))
    png.write_bytes(b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('!2I5B', 480, 270, 8, 2, 0, 0, 0))
                    + chunk(b'IDAT', zlib.compress((b'\0' + b'\x9c\xcf\xd8' * 480) * 270)) + chunk(b'IEND', b''))
    video = captures / 'Synthetic video # 中文.mp4'
    subprocess.run(['ffmpeg', '-v', 'error', '-f', 'lavfi', '-i', 'color=c=0xc4a7e7:s=480x270:d=0.2',
                    '-c:v', 'libx264', '-pix_fmt', 'yuv420p', str(video)], check=True)
    config = temp / 'runtjik.conf'
    config.write_text('[ui]\nmonitor="COPY"\n[index]\npath_commands=false\n[plugins]\nenabled=["Screenshots","Recorder"]\n'
                      f'[plugin_settings.Screenshots]\ndirectory="{captures}"\n'
                      f'[plugin_settings.Recorder]\ndirectory="{captures}"\n')
    env = {**os.environ, 'RUNTJIK_HOME': str(home), 'RUNTJIK_CONFIG': str(config),
           'XDG_STATE_HOME': str(temp / 'state'), 'QT_QUICK_CONTROLS_STYLE': 'Basic'}
    env.pop('RUNTJIK_DEMO', None)

    def ipc(*args):
        return subprocess.check_output(['quickshell', 'ipc', '--path', str(home / 'qml'),
                                        'call', 'runtjik', *args], text=True, stderr=subprocess.DEVNULL)

    def state():
        try:
            return json.loads(ipc('copyState'))
        except subprocess.CalledProcessError:
            return {}

    def key(*args):
        # Give Qt time to process modifier transitions from the virtual keyboard.
        sequence = ['wtype', '-s', '100']
        for offset in range(0, len(args), 2):
            sequence.extend([*args[offset:offset + 2], '-s', '30'])
        subprocess.run(sequence, check=True)

    with (temp / 'launcher.log').open('w') as log:
        process = subprocess.Popen(['quickshell', '--path', str(home / 'qml'), '--no-duplicate'],
                                   env=env, stdout=log, stderr=log)
        try:
            wait_for(lambda: state().get('visible') and state().get('rows'), timeout=20)
            wait_for(lambda: json.loads(ipc('status')).get('ready'), timeout=20)
            for query, path, mime, modifier, copy_key in [
                ('/grab history', png, 'image/png', 'ctrl', 'c'),
                ('/rec history', video, 'text/uri-list', 'ctrl', 'c'),
                ('/grab history', png, 'image/png', 'shift', 'Return'),
                ('/rec history', video, 'text/uri-list', 'shift', 'Return'),
                ('/grab history', png, 'image/png', 'shift', 'KP_Enter'),
                ('/rec history', video, 'text/uri-list', 'shift', 'KP_Enter'),
            ]:
                ipc('reopen')
                wait_for(lambda: state().get('visible'))
                ipc('copyQuery', query)
                wait_for(lambda: state().get('query') == query and state().get('rows'))
                ipc('focusSearch')
                time.sleep(.15)
                key('-M', 'ctrl', '-k', 'a', '-k', 'c', '-m', 'ctrl')
                try:
                    wait_for(lambda: subprocess.run(['wl-paste', '--no-newline'], capture_output=True, text=True).stdout == query)
                except ValueError:
                    raise AssertionError(state())
                assert state()['visible'] and not state()['copied']
                if modifier == 'ctrl':
                    key('-k', 'End')
                    wait_for(lambda: state().get('focus') and state().get('selection') == '')
                # Shift+Enter copies the capture even when search text is selected.
                wait_for(lambda: state().get('focus') and not state().get('busy') and not state().get('pending'))
                started = time.monotonic()
                key('-M', modifier, '-k', copy_key, '-m', modifier)
                try:
                    feedback = wait_for(lambda: state() if state().get('copied') else None)
                except ValueError:
                    raise AssertionError(state())
                assert feedback['visible'], feedback
                wait_for(lambda: state().get('opacity', 0) > .9 and state().get('scale', 0) > .95)
                preview = temp / (path.suffix[1:] + '-copied.png')
                ipc('copyPreview', str(preview))
                wait_for(preview.exists)
                wait_for(lambda: not state().get('visible'))
                assert .45 < time.monotonic() - started < 2
                expected = png.read_bytes() if mime == 'image/png' else (path.as_uri() + '\r\n').encode()
                assert subprocess.check_output(['wl-paste', '--no-newline', '--type', mime]) == expected
                print(f'PASS: {modifier}+{copy_key} in {query} copies {mime}, animates success, closes, and clipboard survives', flush=True)
            # A file removed after querying reports failure and leaves the gallery open.
            ipc('reopen')
            wait_for(lambda: state().get('visible'))
            ipc('copyQuery', '/grab history')
            wait_for(lambda: state().get('query') == '/grab history')
            ipc('focusSearch')
            time.sleep(.15)
            png.unlink()
            key('-M', 'shift', '-k', 'Return', '-m', 'shift')
            wait_for(lambda: 'Capture is no longer available' in state().get('message', ''))
            time.sleep(.65)
            assert state()['visible'] and not state()['copied']
            print('PASS: failure stays open without success animation', flush=True)
            print('Synthetic animation previews: ' + str(temp), flush=True)
        finally:
            process.terminate()
            process.wait(timeout=8)
            shutil.rmtree(home)
