"""Every command destination arrives in the activation response, without a requery."""
import json
import tempfile
from rust_parity import APP, Backend, fixture

contract = json.loads((APP / 'src/contract.json').read_text())
commands = ['/config', '/bindings', '/shortcuts', '/stats', '/run', '/processes', '?']
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
        page = backend.request(op='query', query='/define glimmer', limit=1)
        assert len(page['results']) == 1 and page['has_more'], page
        assert page['word_source_counts']['Wiktionary'] > 1, page
        assert page['word_source_counts']['Urban Dictionary'] == 1, page
        opened = backend.request(op='activate', id='Words:demo-slang')
        assert opened['kind'] == 'message' and opened['message'] == '', opened
        print(f'PASS: {len(commands)} command/alias destinations, full provider counts before pagination and silent source opening')
    finally:
        backend.close()
