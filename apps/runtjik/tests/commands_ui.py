"""Find slash commands through general search using synthetic launcher data."""
import json
import os
from pathlib import Path
import shutil
import subprocess
from gtk_ui import silent_compositor
from private_compositor import wait_for

app = Path(os.environ.get('RUNTJIK_TEST_HOME', Path(__file__).resolve().parents[1]))
with silent_compositor():
    subprocess.run(['hyprctl', 'output', 'create', 'headless', 'COMMANDS'], check=True, stdout=subprocess.DEVNULL)
    wait_for(lambda: any(m['name'] == 'COMMANDS' for m in json.loads(subprocess.check_output(['hyprctl', '-j', 'monitors'], text=True))))
    subprocess.run(['hyprctl', 'eval', 'hl.dispatch(hl.dsp.focus({monitor="COMMANDS"}))'], check=True, stdout=subprocess.DEVNULL)
    if os.environ.get('TASKTJIK_TEST_HOME'):
        os.environ['TASKTJIK_HOME'] = os.environ['TASKTJIK_TEST_HOME']
    temp = Path(os.environ['XDG_RUNTIME_DIR']).parent
    home = temp / 'app'
    shutil.copytree(app / 'qml', home / 'qml')
    shutil.copy2(app / 'runtjik-core', home / 'runtjik-core')
    shell = home / 'qml/shell.qml'
    shell.write_text(shell.read_text().replace('    function receive(msg) {', '''    property bool intermediateEdit: false
    function receive(msg) {
        if(msg.kind==='edit')intermediateEdit=true;
''').replace('resultQuery:root.resultQuery,scrollY:',
        'intermediateEdit:root.intermediateEdit,appMode:root.appMode,editor:todoApp.editor,cursorPosition:searchBox.cursorPosition,providerHeader:root.providerHeader,resultQuery:root.resultQuery,scrollY:'))
    config = temp / 'runtjik.conf'
    config.write_text('[theme]\nicon_theme="rose-punk"\n[ui]\nmonitor="COMMANDS"\nauto_launch_commands=false\n[plugins]\nenabled=["Words","Clipboard","Screenshots","Recorder","Todo","Browser"]\n')
    os.environ.update(RUNTJIK_HOME=str(home), RUNTJIK_DEMO='1', RUNTJIK_CONFIG=str(config),
                      XDG_STATE_HOME=str(temp / 'state'), QT_QUICK_CONTROLS_STYLE='Basic')

    def ipc(*args):
        return subprocess.check_output(['quickshell', 'ipc', '--path', str(home / 'qml'),
                                       'call', 'runtjik', *args], text=True, stderr=subprocess.DEVNULL)

    def state():
        try:
            return json.loads(ipc('testState'))
        except subprocess.CalledProcessError:
            return {}

    with (temp / 'launcher.log').open('w') as log:
        process = subprocess.Popen(['quickshell', '--path', str(home / 'qml'), '--no-duplicate'],
                                   stdout=log, stderr=log)
        try:
            wait_for(lambda: state().get('rows'), timeout=20)
            wait_for(lambda: json.loads(ipc('status')).get('ready'), timeout=20)
            ipc('test', '/')
            wait_for(lambda: state().get('resultQuery') == '/')
            assert all(Path(row['icon']).is_file() for row in state()['rows']), state()
            ipc('capture', str(temp / 'commands.png'))
            wait_for(lambda: (temp / 'commands.png').is_file())
            for command in ('synonym', 'define', 'grab', 'rec', 'browser', 'tabs', 'history', 'cliphist', 'config', 'bindings', 'run', 'gui', 'file', 'dir', 'emoji', 'note', 'monitor', 'bw', 'timer', 'stats'):
                ipc('test', command)
                try:
                    wait_for(lambda: state().get('resultQuery') == command)
                except ValueError:
                    print('Query entry failed:', command, state(), flush=True)
                    raise
                assert state()['rows'][0]['id'] == '/' + command, (command, state())
                subprocess.run(['wtype', '-k', 'Return'], check=True)
                try:
                    wait_for(lambda: state().get('resultQuery', '').strip() == '/' + command)
                except ValueError:
                    print('Command transition failed:', command, state(), flush=True)
                    raise
                assert not state()['intermediateEdit'], (command, state())
                if command == 'file':
                    header = state()['providerHeader']
                    assert header['title'].startswith('RESULTS from')
                    assert header['actions'][0]['label'] == 'CHANGE ROOT'
                if command == 'monitor':
                    assert [action['label'] for action in state()['providerHeader']['actions']] == ['CPU', 'MEMORY']
                if command == 'timer':
                    assert state()['providerHeader']['title'] == 'TIMERS'
                if command in ('define', 'synonym'):
                    assert state()['rows'][0]['id'] == 'Words:hint', state()
            ipc('test', '')
            wait_for(lambda: state().get('resultQuery') == '')
            ipc('test', 'alpha beta')
            wait_for(lambda: state().get('resultQuery') == 'alpha beta')
            subprocess.run(['wtype', '-M', 'ctrl', '-k', 'Left', '-m', 'ctrl'], check=True)
            wait_for(lambda: state().get('cursorPosition') == 6)
            subprocess.run(['wtype', '-M', 'ctrl', '-k', 'Right', '-m', 'ctrl'], check=True)
            wait_for(lambda: state().get('cursorPosition') == 10)
            ipc('test', '')
            wait_for(lambda: state().get('resultQuery') == '')
            subprocess.run(['wtype', 'todo'], check=True)
            wait_for(lambda: state().get('resultQuery') == 'todo')
            assert state()['rows'][0]['id'] == '/todo', state()
            ipc('capture', str(temp / 'search.png'))
            wait_for(lambda: (temp / 'search.png').is_file())
            subprocess.run(['wtype', '-k', 'Return'], check=True)
            wait_for(lambda: state().get('appMode') == 'Todo')
            assert not state()['intermediateEdit'], state()
            subprocess.run(['wtype', 'n'], check=True)
            wait_for(lambda: state().get('editor') == 'edit')
            ipc('capture', str(temp / 'editor.png'))
            wait_for(lambda: (temp / 'editor.png').is_file())
            assert 'TypeError' not in (temp / 'launcher.log').read_text()
            assert 'ERROR' not in (temp / 'launcher.log').read_text()
            print('PASS atomic command navigation, general todo search, Enter opens Todo, N opens editor:', temp)
        finally:
            process.terminate()
            process.wait(timeout=5)
