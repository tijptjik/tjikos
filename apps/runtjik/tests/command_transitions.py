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
        print(f'PASS: {len(commands)} command/alias destinations without an intermediate query')
    finally:
        backend.close()
