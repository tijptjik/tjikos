"""Render synthetic word lookups in an isolated compositor; never call APIs."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import time

from private_compositor import private_compositor, wait_for

app = Path(__file__).resolve().parents[1]
with private_compositor(isolate_pointer=True):
    subprocess.run(['hyprctl', 'output', 'create', 'headless', 'WORDS'], check=True, stdout=subprocess.DEVNULL)
    wait_for(lambda: any(m['name'] == 'WORDS' for m in json.loads(subprocess.check_output(['hyprctl', '-j', 'monitors'], text=True))))
    subprocess.run(['hyprctl', 'eval', 'hl.monitor({output="WORDS",mode="1000x680@60",position="-3000x0",scale=1})'], check=True, stdout=subprocess.DEVNULL)
    subprocess.run(['hyprctl', 'eval', 'hl.dispatch(hl.dsp.focus({monitor="WORDS"}))'], check=True, stdout=subprocess.DEVNULL)
    temp = Path(os.environ['XDG_RUNTIME_DIR']).parent
    home = temp / 'app'
    home.mkdir()
    shutil.copytree(app / 'qml', home / 'qml')
    shutil.copy2(app / 'runtjik-core', home / 'runtjik-core')
    shell = home / 'qml/shell.qml'
    shell.write_text(shell.read_text().replace('function send(data) {', 'property bool testFreeze: false\n    function send(data) {if(testFreeze)return;').replace('function receive(msg) {', 'function receive(msg) {if(testFreeze && !msg.synthetic)return;').replace('function testState(): string {', '''
        function streamWords(stage: string): void {
            root.testFreeze=true;
            const group=(source)=>Array.from({length:18},(_,i)=>({id:'Words:'+source+i,provider:'Words',subtitle:'Synthetic streamed result',word_source:source,part_of_speech:'noun',name:root.commandQuery.startsWith('/synonym')?'Related word '+i:'A full definition from '+source+' with enough text to wrap onto multiple lines and preserve the reader’s place.',examples:['A separate usage example for this definition.'],url:'https://en.wiktionary.org/wiki/test'}));
            const loading={id:'Words:loading',disabled:true,name:'Looking up…',subtitle:'Results appear as each source responds'};
            const rows=stage==='loading'?[loading]:stage==='first'?group('B').concat([loading]):group('A').concat(group('B'));
            root.receive({synthetic:true,kind:'results',query:root.query,results:rows,pending:stage!=='complete',indexed:root.indexed,total:rows.length,config:root.settings});
        }
        function endStream(): void {root.testFreeze=false;}
        function longWords(): void {
            root.testFreeze=true;
            root.results=Array.from({length:24}, (_,i)=>({id:'Words:long'+i,provider:'Words',subtitle:'Synthetic long definition',word_source:i<12?'Synthetic dictionary':'Second dictionary',part_of_speech:'noun',name:'A deliberately long definition that must wrap completely without clipping, ellipsis, overlap, or requiring a tooltip. '.repeat(3),examples:['An example sentence that stays separate from the meaning and wraps naturally across the available width.'],url:'https://en.wiktionary.org/wiki/test'}));
        }
        function testState(): string {''').replace('resultQuery:root.resultQuery,scrollY:items.contentY,',
        'searchFocus:searchBox.activeFocus,wordPosition:wordPane.readingPosition(),wordAtTop:wordPane.view.atYBeginning,wordIndicator:wordPane.showIndicator,selectedId:root.results[root.selected]?.id,wordEmpty:wordPane.empty,wordListVisible:wordPane.view.visible,wordHeight:wordPane.view.contentHeight,wordViewport:wordPane.view.height,selected:root.selected,resultQuery:root.resultQuery,scrollY:root.activeItems.contentY-root.activeItems.originY,'))
    config = temp / 'runtjik.conf'
    config.write_text('[theme]\nicon_theme="rose-punk"\n[ui]\nmonitor="WORDS"\n')
    os.environ.update(RUNTJIK_HOME=str(home), RUNTJIK_DEMO='1', RUNTJIK_CONFIG=str(config),
                      XDG_STATE_HOME=str(temp / 'state'), QT_QUICK_CONTROLS_STYLE='Basic')

    def ipc(*args):
        return subprocess.check_output(['quickshell', 'ipc', '--path', str(home / 'qml'), 'call', 'runtjik', *args], text=True, stderr=subprocess.DEVNULL)

    def state():
        try:
            return json.loads(ipc('testState'))
        except subprocess.CalledProcessError:
            return {}

    def query(text):
        ipc('test', text)
        wait_for(lambda: state().get('resultQuery') == text)

    def capture(name):
        ipc('capture', str(temp / (name + '.png')))
        wait_for(lambda: (temp / (name + '.png')).is_file())

    with (temp / 'launcher.log').open('w') as log:
        process = subprocess.Popen(['quickshell', '--path', str(home / 'qml'), '--no-duplicate'], stdout=log, stderr=log)
        try:
            wait_for(lambda: state().get('rows'), timeout=20)
            wait_for(lambda: json.loads(ipc('status')).get('ready'), timeout=20)
            query('/config plugin_settings.Words')
            assert len(state()['rows']) == 6
            capture('word-settings')
            query('?')
            assert all(row['windows'] > 0 for row in state()['rows'])
            query('/running')
            assert state()['rows'] == []
            for command in ('define', 'synonym'):
                query('/' + command)
                assert state()['wordEmpty'] and not state()['wordListVisible'], state()
                capture(command + '-empty')
                query('/' + command + ' define')
                wait_for(lambda: any('Synthetic dictionary' in row['subtitle'] for row in state()['rows']))
                time.sleep(.2)
                capture(command)
                assert state()['wordListVisible'], state()
                if command == 'synonym':
                    assert state()['wordHeight'] < 250, state()
                else:
                    assert 0 < state()['wordHeight'] < state()['wordViewport'], state()
                    ipc('longWords')
                    wait_for(lambda: state()['wordHeight'] > state()['wordViewport'])
                    ipc('focusSearch')
                    initial_selection = state()['selected']
                    subprocess.run(['wtype', '-s', '150', '-k', 'Down', '-s', '150', '-k', 'Down', '-s', '150', '-k', 'Down'], check=True)
                    wait_for(lambda: state()['selected'] == (initial_selection + 3) % 24 and state()['scrollY'] > 0)
                    capture('define-long')
                    ipc('endStream')
            for command in ('define', 'synonym'):
                query('/' + command + ' glimmer')
                wait_for(lambda: any(row.get('section') for row in state()['rows']))
                assert all(row.get('language') == 'English' for row in state()['rows']), state()
                if command == 'define':
                    assert any(row.get('sense_number') == '1.1' for row in state()['rows']), state()
                    assert any(row.get('word_source') == 'Urban Dictionary' for row in state()['rows']), state()
                else:
                    assert all(row['section'] in ('synonyms', 'related_terms') for row in state()['rows']), state()
                capture(command + '-entry')
            for command in ('define', 'synonym'):
                query('/' + command + ' streaming')
                ipc('streamWords', 'loading')
                wait_for(lambda: state()['wordIndicator'] and not state()['wordListVisible'])
                capture(command + '-loading')
                ipc('streamWords', 'first')
                ipc('focusSearch')
                time.sleep(.15)
                wait_for(lambda: state()['wordListVisible'])
                ipc('streamWords', 'complete')
                time.sleep(.3)
                assert state()['wordAtTop'] and state()['selected'] == 0, state()
                assert state()['searchFocus'], state()
                # A slow, higher-priority source must not steal the reader's place.
                ipc('streamWords', 'first')
                for _ in range(14 if command == 'synonym' else 5):
                    subprocess.run(['wtype', '-k', 'Down'], check=True)
                    time.sleep(.1)
                time.sleep(.2)
                before = state()
                assert not before['wordAtTop'], before
                ipc('streamWords', 'complete')
                time.sleep(.3)
                after = state()
                assert after['selectedId'] == before['selectedId'], (before['selectedId'], after['selectedId'])
                assert after['wordPosition']['id'] == before['wordPosition']['id'], (before['wordPosition'], after['wordPosition'])
                assert abs(after['wordPosition']['y'] - before['wordPosition']['y']) < 2, (before['wordPosition'], after['wordPosition'])
                assert after['searchFocus'], after
                # Repeated provider polls must also leave focus and scroll untouched.
                for _ in range(3):
                    ipc('streamWords', 'complete')
                    time.sleep(.4)
                assert state()['wordPosition'] == after['wordPosition'], state()
                capture(command + '-streaming')
                ipc('endStream')
            for command, name in (('/grab history', 'gallery'), ('/rec history', 'recordings'), ('!', 'commands')):
                query(command)
                time.sleep(.2)
                capture(name)
            assert 'TypeError' not in (temp / 'launcher.log').read_text()
            assert 'Error:' not in (temp / 'launcher.log').read_text()
            print('PASS: quiet empty/loading states, definitions, synonyms, keyboard scrolling and stable streamed results/focus', flush=True)
        finally:
            print('UI process exit:', process.poll(), flush=True)
            print('Final UI state:', {k:v for k,v in state().items() if k not in ('rows', 'statistics')}, flush=True)
            process.terminate()
            process.wait(timeout=5)
            print('Word UI evidence:', temp, flush=True)
