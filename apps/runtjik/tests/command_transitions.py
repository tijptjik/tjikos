"""Every command destination arrives in the activation response, without a requery."""
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
from rust_parity import APP, Backend, fixture

contract = json.loads((APP / 'src/contract.json').read_text())
commands = ['/expand', '/config', '/bindings', '/shortcuts', '/stats', '/run', '/gui', '?',
            '/timer', '/file', '/dir', '/emoji', '/note', '/monitor', '/bw']
for provider in contract['providers']:
    if provider['commands']:
        commands.extend(command[0] for command in provider['commands'])
        commands.extend(alias[0] for alias in provider['aliases'] if alias[0].startswith('/'))

with tempfile.TemporaryDirectory(prefix='runtjik-transitions-') as temp:
    env = fixture(temp)
    with open(env['RUNTJIK_CONFIG'], 'a') as config:
        config.write('[plugins]\nenabled=' + json.dumps([p['name'] for p in contract['providers'] if p['commands']]) + '\n')
    backend = Backend('rust', env, demo=True)
    try:
        for command in commands:
            backend.request(op='query', query='/')
            response = backend.request(op='activate', id=command)
            assert response['kind'] == 'results', (command, response)
            assert response['edit_query'].strip() == command, (command, response)
            assert response['query'] == response['edit_query'], (command, response)
            assert not any(row['id'] == command for row in response['results']), (command, response)
        for command in ('/define', '/synonym'):
            response = backend.request(op='query', query=command)
            assert response['results'][0]['empty_state']
            assert response['results'][0]['disabled']
        timer = backend.request(op='query', query='/timer set ten minutes')
        assert timer['results'][0]['id'] == 'Timer:set', timer
        assert '10 minutes' in timer['results'][0]['name'], timer
        started = backend.request(op='activate', id='Timer:set')
        assert started['kind'] == 'results' and started['message'] == 'Demo: 10 minutes timer set', started
        page = backend.request(op='query', query='/define glimmer', limit=1)
        assert len(page['results']) == 1 and page['has_more'], page
        assert page['word_source_counts']['Wiktionary'] > 1, page
        assert page['word_source_counts']['Urban Dictionary'] == 1, page
        opened = backend.request(op='activate', id='Words:demo-slang')
        assert opened['kind'] == 'message' and opened['message'] == '', opened
        print(f'PASS: {len(commands)} command/alias destinations, full provider counts before pagination and silent source opening')
    finally:
        backend.close()

if all(shutil.which(tool) for tool in ('fd', 'fzf', 'zoxide')):
    with tempfile.TemporaryDirectory(prefix='runtjik-search-providers-') as temp:
        env = fixture(temp)
        root = Path(env['HOME'])
        folder = root / 'Jump Fixture'
        folder.mkdir()
        (folder / 'search-marker.txt').write_text('synthetic')
        scratch = Path(temp) / 'scratch-notes'
        scratch.mkdir()
        note = scratch / 'Scratch fixture.md'
        note.write_text('Synthetic note for provider search')
        vault = Path(temp) / 'obsidian-vault'
        (vault / '.obsidian').mkdir(parents=True)
        (vault / 'Projects').mkdir()
        (vault / 'Research').mkdir()
        env['RUNTJIK_NOTES_DIR'] = str(scratch)
        env['RUNTJIK_OBSIDIAN_VAULT'] = str(vault)
        env['_ZO_DATA_DIR'] = str(Path(temp) / 'zoxide')
        subprocess.run(['zoxide', 'add', str(folder)], env=env, check=True, stdout=subprocess.DEVNULL)
        backend = Backend('rust', env, demo=False)
        try:
            files = backend.request(op='query', query='/file search-marker')
            assert files['results'][0]['name'] == 'search-marker.txt', files
            directories = backend.request(op='query', query='/dir Jump')
            assert any(row['subtitle'] == str(folder) for row in directories['results']), directories
            emojis = backend.request(op='query', query='/emoji smile')
            assert any('smil' in row['name'].lower() for row in emojis['results']), emojis
            history = backend.request(op='query', query='/note history')
            note_row = next(row for row in history['results'] if row['name'] == 'Scratch fixture')
            tree = backend.request(op='activate', id=note_row['id'], mode='switch')
            assert tree['query'] == '/note vault', tree
            branch = backend.request(op='query', query='/note vault project')
            assert any(row['name'] == 'Projects' for row in branch['results']), branch
            assert not any(row['name'] == 'Research' for row in branch['results']), branch
            print('PASS: fd+fzf, zoxide, system emoji data, and searchable Obsidian note tree use isolated fixtures')
        finally:
            backend.close()
