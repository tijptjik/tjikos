"""Check configurable entry sections, language isolation, and a real term click."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import time

from private_compositor import private_compositor, wait_for

app = Path(__file__).resolve().parents[1]
sections = ['definitions', 'synonyms', 'related_terms', 'derived_terms', 'pronunciation',
            'etymology', 'alternative_forms', 'antonyms', 'usage_notes', 'inflections', 'translations']
with private_compositor(isolate_pointer=True):
    subprocess.run(['hyprctl', 'output', 'create', 'headless', 'ENTRY'], check=True, stdout=subprocess.DEVNULL)
    wait_for(lambda: any(m['name'] == 'ENTRY' for m in json.loads(subprocess.check_output(['hyprctl', '-j', 'monitors'], text=True))))
    subprocess.run(['hyprctl', 'eval', 'hl.monitor({output="ENTRY",mode="1000x680@60",position="-3000x0",scale=1})'], check=True, stdout=subprocess.DEVNULL)
    subprocess.run(['hyprctl', 'eval', 'hl.dispatch(hl.dsp.focus({monitor="ENTRY"}))'], check=True, stdout=subprocess.DEVNULL)
    temp = Path(os.environ['XDG_RUNTIME_DIR']).parent
    home = temp / 'app'
    shutil.copytree(app / 'qml', home / 'qml')
    shutil.copy2(app / 'runtjik-core', home / 'runtjik-core')
    shell = home / 'qml/shell.qml'
    shell.write_text(shell.read_text().replace('function testState(): string {', '''
        function selectDefinition(): void {
            root.selected=root.results.findIndex(row=>(row.sense_synonyms || []).length>1);
            wordPane.view.positionViewAtIndex(root.selected,ListView.Contain);
            root.focusSearch();
        }
        function selectSingleTerm(): void {
            root.selected=root.results.findIndex(row=>row.terms?.length===1 && row.terms[0].term==='glim');
            wordPane.view.positionViewAtIndex(root.selected,ListView.Contain);
            root.focusSearch();
        }
        function sourceAction(): void {root.activate(false);}
        function providerPoint(name: string): string {
            function find(item) {
                if(item.objectName==='wordProvider:'+name)return item;
                for(const child of item.children || []) {const hit=find(child);if(hit)return hit;}
                return null;
            }
            const item=find(frame);if(!item)return '{}';
            const point=item.mapToItem(frame,item.width/2,item.height/2);
            return JSON.stringify({x:frame.x+point.x,y:frame.y+point.y,width:panel.screen.width,height:panel.screen.height});
        }
        function termPoint(): string {
            function find(item) {
                if(item.objectName==='wordTerm' && item.visible && item.text==='glim')return item;
                for(const child of item.children || []) {const hit=find(child);if(hit)return hit;}
                return null;
            }
            const item=find(wordPane);
            if(!item)return '{}';
            const point=item.mapToItem(frame,item.width/2,item.height/2);
            return JSON.stringify({x:frame.x+point.x,y:frame.y+point.y,width:panel.screen.width,height:panel.screen.height});
        }
        function testState(): string {''').replace('resultQuery:root.resultQuery,', 'navigatingTerms:wordPane.navigatingTerms,termIndex:wordPane.termIndex,selectedTerm:wordPane.selectedTerms[wordPane.termIndex]?.term,providers:wordPane.providers,hiddenSources:wordPane.hiddenSources,visibleIds:wordPane.visibleIndices.map(i=>root.results[i].id),selectedId:root.results[root.selected]?.id,allHidden:wordPane.allHidden,searchFocus:searchBox.activeFocus,resultQuery:root.resultQuery,'))
    protocol = app / 'tests/virtual_pointer.xml'
    subprocess.run(['wayland-scanner', 'client-header', str(protocol), str(temp / 'pointer.h')], check=True)
    subprocess.run(['wayland-scanner', 'private-code', str(protocol), str(temp / 'protocol.c')], check=True)
    flags = subprocess.check_output(['pkg-config', '--cflags', '--libs', 'wayland-client'], text=True).split()
    subprocess.run(['cc', str(app / 'tests/pointer.c'), str(temp / 'protocol.c'), '-I' + str(temp), '-o', str(temp / 'pointer'), *flags], check=True)
    config = temp / 'runtjik.conf'
    os.environ.update(RUNTJIK_HOME=str(home), RUNTJIK_DEMO='1', RUNTJIK_CONFIG=str(config),
                      XDG_STATE_HOME=str(temp / 'state'), QT_QUICK_CONTROLS_STYLE='Basic')

    def ipc(*args):
        return subprocess.check_output(['quickshell', 'ipc', '--path', str(home / 'qml'), 'call', 'runtjik', *args], text=True, stderr=subprocess.DEVNULL)

    def state():
        try:
            return json.loads(ipc('testState'))
        except subprocess.CalledProcessError:
            return {}

    def click(point):
        pointer = subprocess.Popen([str(temp / 'pointer'), 'ENTRY'], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
        try:
            motion = f"m {round(point['x'] / point['width'] * 10000)} {round(point['y'] / point['height'] * 10000)}"
            # Let the new virtual device finish mapping before positioning for the click.
            for operation in [motion, motion, 'b 1', 'b 0']:
                pointer.stdin.write(operation + '\n')
                pointer.stdin.flush()
                assert pointer.stdout.readline().strip() == 'ok'
                time.sleep(.15)
        finally:
            pointer.stdin.close()
            pointer.wait(timeout=5)

    for name, languages, enabled in [('all-sections', ['English', 'French'], sections),
                                     ('paged', ['English'], sections),
                                     ('french', ['French'], ['definitions'])]:
        config.write_text('[ui]\nmonitor="ENTRY"\n' + ('max_results=10\n' if name == 'paged' else '') + '[plugin_settings.Words]\nlanguages=' + json.dumps(languages) + '\nsections=' + json.dumps(enabled) + '\n')
        with (temp / (name + '.log')).open('w') as log:
            process = subprocess.Popen(['quickshell', '--path', str(home / 'qml'), '--no-duplicate'], stdout=log, stderr=log)
            try:
                wait_for(lambda: state().get('rows'), timeout=20)
                wait_for(lambda: json.loads(ipc('status')).get('ready'))
                ipc('test', '/define glimmer')
                wait_for(lambda: state().get('resultQuery') == '/define glimmer')
                rows = state()['rows']
                assert {row.get('language') for row in rows} == set(languages), rows
                if name != 'paged':
                    assert {row.get('section') for row in rows} == set(enabled), rows
                if name == 'french':
                    assert len(rows) == 1 and rows[0]['word_source'] == 'Wiktionary', rows
                if name == 'paged':
                    counts = {p['name']: p['count'] for p in state()['providers']}
                    assert counts['Wiktionary'] > 10 and counts['Urban Dictionary'] == 1, counts
                    click(json.loads(ipc('providerPoint', 'Wiktionary')))
                    wait_for(lambda: state()['visibleIds'] == ['Words:demo-slang'])
                    assert state()['selectedId'] == 'Words:demo-slang', state()
                    assert {p['name']: p['count'] for p in state()['providers']} == counts
                ipc('capture', str(temp / (name + '.png')))
                wait_for(lambda: (temp / (name + '.png')).exists())
                if name == 'all-sections':
                    original_counts = {p['name']: p['count'] for p in state()['providers']}
                    assert original_counts['Wiktionary'] == sum(r.get('word_source') == 'Wiktionary' for r in rows)
                    assert original_counts['Urban Dictionary'] == 1
                    click(json.loads(ipc('providerPoint', 'Wiktionary')))
                    wait_for(lambda: len(state()['visibleIds']) == 1)
                    assert state()['selectedId'] == 'Words:demo-slang', state()
                    subprocess.run(['wtype', '-k', 'Down'], check=True)
                    assert state()['selectedId'] == 'Words:demo-slang', state()
                    assert {p['name']: p['count'] for p in state()['providers']} == original_counts
                    ipc('capture', str(temp / 'urban-only.png'))
                    wait_for(lambda: (temp / 'urban-only.png').exists())
                    click(json.loads(ipc('providerPoint', 'Urban Dictionary')))
                    assert state()['allHidden'] and not state()['visibleIds'], state()
                    ipc('capture', str(temp / 'sources-hidden.png'))
                    wait_for(lambda: (temp / 'sources-hidden.png').exists())
                    click(json.loads(ipc('providerPoint', 'Wiktionary')))
                    wait_for(lambda: len(state()['visibleIds']) == original_counts['Wiktionary'])
                    click(wait_for(lambda: json.loads(ipc('termPoint'))))
                    wait_for(lambda: state().get('resultQuery') == '/define glim')
                    ipc('test', '/define glimmer')
                    wait_for(lambda: state().get('resultQuery') == '/define glimmer')
                    ipc('selectDefinition')
                    time.sleep(.2)
                    def key(name):
                        subprocess.run(['wtype', '-k', name], check=True)
                        time.sleep(.15)
                    key('Return')
                    wait_for(lambda: state().get('selectedTerm') == 'gleam')
                    assert state()['navigatingTerms'] and state()['searchFocus'], state()
                    key('Right')
                    assert state()['selectedTerm'] == 'spark', state()
                    key('Left')
                    assert state()['selectedTerm'] == 'gleam', state()
                    key('Escape')
                    assert not state()['navigatingTerms'] and state()['resultQuery'] == '/define glimmer', state()
                    key('Return')
                    key('Right')
                    ipc('capture', str(temp / 'selected-related-word.png'))
                    wait_for(lambda: (temp / 'selected-related-word.png').exists())
                    key('Return')
                    wait_for(lambda: state().get('resultQuery') == '/define spark')
                    assert not state()['navigatingTerms'], state()
                    ipc('test', '/define glimmer')
                    wait_for(lambda: state().get('resultQuery') == '/define glimmer')
                    ipc('selectSingleTerm')
                    time.sleep(.2)
                    key('Return')
                    wait_for(lambda: state().get('resultQuery') == '/define glim')
                    assert not state()['navigatingTerms'], state()
                    ipc('test', '/define glimmer')
                    wait_for(lambda: state().get('resultQuery') == '/define glimmer')
                    ipc('selectDefinition')
                    ipc('sourceAction')
                    time.sleep(.2)
                    assert json.loads(ipc('status'))['message'] == '', ipc('status')
                assert 'Error:' not in (temp / (name + '.log')).read_text()
                assert 'TypeError' not in (temp / (name + '.log')).read_text()
                assert 'Binding loop' not in (temp / (name + '.log')).read_text()
            except Exception:
                if process.poll() is None:
                    print('UI state:', name, {k: v for k, v in state().items() if k != 'rows'}, flush=True)
                    print('UI status:', ipc('status'), flush=True)
                raise
            finally:
                process.terminate()
                process.wait(timeout=5)
    print('PASS sections/languages, provider counts/toggles, hidden-source keyboard navigation and term lookup:', temp)
