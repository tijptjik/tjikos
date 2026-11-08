"""Persistent dictionary reuse with a synthetic service; no external requests."""
import json
import os
from pathlib import Path
import tempfile
import time
from rust_parity import Backend, fixture

with tempfile.TemporaryDirectory(prefix='runtjik-word-history-') as temp:
    root = Path(temp)
    env = fixture(root, count=0)
    config = root / 'runtjik.conf'
    config.write_text(config.read_text() + '[plugins]\nenabled=["Words"]\n[plugin_settings.Words]\ndefine_sources=["free_dictionary"]\nsynonym_sources=["free_dictionary"]\n')
    tools = root / 'bin'
    tools.mkdir()
    service = tools / 'curl'
    service.write_text('''#!/usr/bin/env python3
import json, os
from pathlib import Path
root = Path(os.environ['WORD_FIXTURE'])
with (root / 'requests').open('a') as f: f.write('request\\n')
if (root / 'fail').exists():
    print('{}\\n503', end='')
else:
    print(json.dumps([{'meanings':[{'partOfSpeech':'noun','synonyms':['spark'], 'definitions':[{'definition':'A synthetic definition', 'example':'A synthetic example'}]}]}]))
    print('200', end='')
''')
    service.chmod(0o755)
    env.update(PATH=str(tools) + os.pathsep + env['PATH'], WORD_FIXTURE=str(root))
    def calls():
        return len((root / 'requests').read_text().splitlines()) if (root / 'requests').exists() else 0
    def query(backend, term, command='/define'):
        for _ in range(200):
            response = backend.request(op='query', query=f'{command} {term}')
            if not response.get('pending'):
                return response
            time.sleep(.01)
        raise AssertionError('Dictionary fixture timed out')
    backend = Backend('rust', env)
    try:
        assert backend.request(op='query', query='/define')['results'][0]['empty_state']
        first = query(backend, 'glimmer')
        assert first['results'][0]['name'] == 'A synthetic definition', first
        query(backend, 'monster')
        recent = backend.request(op='query', query='/define')['results']
        assert [r['name'] for r in recent] == ['monster', 'glimmer'], recent
        assert calls() == 2
        restored = backend.request(op='activate', id=recent[1]['id'])
        assert restored['edit_query'] == '/define glimmer'
        assert restored['word_cache_hits'] == 1 and not restored.get('pending'), restored
        assert calls() == 2
    finally:
        backend.close()
    backend = Backend('rust', env)
    try:
        assert query(backend, 'glimmer')['word_cache_hits'] == 1
        assert calls() == 2
        assert [r['name'] for r in backend.request(op='query', query='/synonym')['results']] == ['glimmer', 'monster']
        assert query(backend, 'glimmer', '/synonym').get('word_cache_hits', 0) == 0
        assert calls() == 3  # Definition and synonym results cannot share a cache entry.
        (root / 'fail').touch()
        failed = query(backend, 'retry')
        assert failed['results'][0]['disabled']
        failed_history = backend.request(op='query', query='/define')['results']
        assert 'retry' not in [r['name'] for r in failed_history]
        (root / 'fail').unlink()
        retried = query(backend, 'retry')
        assert not retried['results'][0]['disabled'] and retried.get('word_cache_hits', 0) == 0
        assert calls() == 5
    finally:
        backend.close()
    config.write_text(config.read_text().replace('[plugin_settings.Words]', '[plugin_settings.Words]\nmax_results_per_source=1'))
    backend = Backend('rust', env)
    try:
        assert query(backend, 'glimmer').get('word_cache_hits', 0) == 0
        assert calls() == 6, 'Changed word settings must not reuse the old response'
    finally:
        backend.close()
    config.write_text(config.read_text() + '[history]\nenabled=false\n')
    database = root / 'state/runtjik/words.sqlite3'
    before = database.read_bytes()
    backend = Backend('rust', env)
    try:
        query(backend, 'private-term')
        assert backend.request(op='query', query='/define')['results'][0]['empty_state']
    finally:
        backend.close()
    assert database.read_bytes() == before, 'Disabled history must not write persistent word data'
    print('Word history: persistent MRU, activation, cache hits, mode isolation, error retry and history-off passed')
