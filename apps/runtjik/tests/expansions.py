"""Synthetic Espanso CRUD, preservation, filtering and stale-editor checks."""
import json
import os
import subprocess
from pathlib import Path
import tempfile
import yaml

APP = Path(__file__).resolve().parents[1]

class NativeEditor:
    def handle(self, request):
        # No executable lookup is available to the native editor, including Python.
        result = subprocess.run(
            [str(APP / 'runtjik-core'), 'expansions'],
            input=json.dumps(request), text=True, capture_output=True,
            env={**os.environ, 'PATH': '/nonexistent'},
        )
        response = json.loads(result.stdout)
        if result.returncode:
            raise ValueError(response.get('message', response))
        return response['ok']

    def entries(self, root):
        return self.handle({'action': 'list'})

module = NativeEditor()

with tempfile.TemporaryDirectory(prefix='runtjik-expansions-') as temporary:
    root = Path(temporary)
    os.environ['RUNTJIK_EXPANSIONS_DIR'] = str(root)
    path = root / 'base.yml'
    initial = '''# Keep this comment
matches:
  - trigger: ";zulu"
    replace: "last"
  - trigger: ";alpha"
    replace: |
      Hello {{name}}
      second line
    vars:
      - name: name
        type: date
        params:
          format: "%Y"
# Keep this too
other: true
'''
    path.write_text(initial)
    rows = module.handle({'action': 'list'})
    assert [row['trigger'] for row in rows] == [';alpha', ';zulu']
    module.handle(dict(action='save', original=rows[0], trigger=';beta', expansion='こんにちは\n{{name}}'))
    saved = path.read_text()
    assert saved.startswith(initial[:initial.index('  - trigger: ";alpha"')])
    assert saved.endswith(initial[initial.index('    vars:'):])
    assert yaml.safe_load(saved)['matches'][1]['replace'] == 'こんにちは\n{{name}}'
    try:
        module.handle(dict(action='delete', original=rows[0]))
        raise AssertionError('Stale delete accepted')
    except ValueError as error:
        assert 'changed' in str(error)
    rows = module.entries(root)
    try:
        module.handle(dict(action='save', trigger=';zulu', expansion='duplicate'))
        raise AssertionError('Duplicate accepted')
    except ValueError as error:
        assert 'already exists' in str(error)
    module.handle(dict(action='delete', original=rows[0]))
    assert yaml.safe_load(path.read_text())['matches'] == [{'trigger': ';zulu', 'replace': 'last'}]
    module.handle(dict(action='delete', original=module.entries(root)[0]))
    assert yaml.safe_load(path.read_text()) == {'matches': [], 'other': True}
    module.handle(dict(action='save', trigger=';new', expansion='new\nvalue'))
    assert module.entries(root)[0]['expansion'] == 'new\nvalue'
print('PASS: alphabetical dictionary, CRUD, Unicode/multiline, vars and surrounding YAML, duplicate and stale-write guards')

from rust_parity import Backend, fixture
with tempfile.TemporaryDirectory(prefix='runtjik-expansion-protocol-') as temporary:
    root = Path(temporary)
    env = fixture(root, count=0)
    dictionary = root / 'matches'
    dictionary.mkdir()
    (dictionary / 'base.yml').write_text('matches:\n  - trigger: ";zulu"\n    replace: "Needle in the expansion"\n  - trigger: ";Alpha"\n    replace: "First"\n')
    env['RUNTJIK_EXPANSIONS_DIR'] = str(dictionary)
    backend = Backend('rust', env)
    try:
        rows = backend.request(op='query', query='/expand')['results']
        assert [r['name'] for r in rows] == ['New', ';Alpha', ';zulu'], rows
        for query, expected in [('/expand NEEDLE', ';zulu'), ('/expand alpha', ';Alpha')]:
            rows = backend.request(op='query', query=query)['results']
            assert [r['name'] for r in rows] == ['New', expected], rows
        opened = backend.request(op='activate', id=rows[1]['id'])
        assert opened['kind'] == 'expansion-editor', opened
        saved = backend.request(op='activate', id=opened['id'], mode='save-expansion', original=opened['original'], trigger=';beta', expansion='Edited\ntext')
        assert [r['name'] for r in saved['results']] == ['New', ';beta', ';zulu'], saved
        deleted = backend.request(op='activate', id=saved['results'][1]['id'], mode='close')
        assert [r['name'] for r in deleted['results']] == ['New', ';zulu'], deleted
        new = backend.request(op='activate', id='Expand:new')
        created = backend.request(op='activate', id='Expand:new', mode='save-expansion', original=new['original'], trigger=';new', expansion='Created')
        assert [r['name'] for r in created['results']] == ['New', ';new', ';zulu'], created
        opened = backend.request(op='activate', id=created['results'][1]['id'])
        path = dictionary / opened['original']['file']
        path.write_text(path.read_text() + '\n# External change\n')
        rejected = backend.request(op='activate', id=opened['id'], mode='save-expansion', original=opened['original'], trigger=';new', expansion='Overwrite')
        assert rejected['kind'] == 'error' and 'changed' in rejected['message'], rejected
    finally:
        backend.close()
print('PASS: /expand protocol, trigger/expansion filters, inline edit, create/delete and concurrent modification refusal')
