"""Stats lifecycle contracts using isolated synthetic history."""
from pathlib import Path
import json
import sqlite3
import tempfile
from rust_parity import Backend, fixture

with tempfile.TemporaryDirectory(prefix='runtjik-stats-') as temp:
    env = fixture(temp, 2)
    backend = Backend('rust', env)
    try:
        def stats():
            return backend.request(op='query', query='/stats')['statistics']

        assert stats()['metrics'][0]['total'] == 1
        assert stats()['metrics'][0]['total'] == 1  # Searching does not count as opening.
        backend.request(op='resume')
        assert stats()['metrics'][0]['total'] == 2
        opened = backend.request(op='activate', id='/stats')
        assert opened['edit_query'] == '/stats' and opened['results'] == []
        assert opened['statistics']['metrics'][0]['total'] == 2
        db = sqlite3.connect(Path(env['XDG_STATE_HOME']) / 'runtjik/history.sqlite3')
        db.execute("INSERT INTO events(time,entry_id,action,success) VALUES(strftime('%s','now','-2 days'),'fixture','launch',1)")
        db.commit()
        assert stats()['metrics'][1]['total'] == 1
        assert stats()['metrics'][1]['days'] == 3
        assert backend.request(op='query', query='')['results']
        # Pause all usage records independently of command history, via the UI setting.
        result = backend.request(op='activate', id='/config stats.enabled false')
        assert result['config']['stats']['enabled'] is False
        assert result['config']['history']['enabled'] is True
        saved = {table: db.execute('SELECT * FROM ' + table).fetchall()
                 for table in ('events', 'counts', 'frecency')}
        backend.request(op='resume')
        assert backend.request(op='activate', id='apps:fixture-0000.desktop', mode='launch')['kind'] == 'activated'
        backend.request(op='activate', id='/refresh')
        for table, rows in saved.items():
            assert db.execute('SELECT * FROM ' + table).fetchall() == rows, table
        backend.process.stdin.write(json.dumps({'op': 'remember-command', 'query': '/stats'}) + '\n')
        backend.process.stdin.flush()
        assert '/stats' in backend.request(op='resume')['command_history']
        assert stats()['metrics'][0]['total'] == 2
    finally:
        backend.close()
    # Persisted opt-out also covers a cold start.
    backend = Backend('rust', env)
    try:
        assert stats()['metrics'][0]['total'] == 2
        for table, rows in saved.items():
            assert db.execute('SELECT * FROM ' + table).fetchall() == rows, table
        result = backend.request(op='activate', id='/config stats.enabled true')
        assert result['config']['stats']['enabled'] is True
        backend.request(op='resume')
        assert stats()['metrics'][0]['total'] == 3
        backend.request(op='activate', id='/refresh')
        assert stats()['metrics'][4]['total'] == 1
    finally:
        backend.close()
    with Path(env['RUNTJIK_CONFIG']).open('a') as config:
        config.write('\n[history]\nenabled=false\n')
    backend = Backend('rust', env)
    try:
        backend.request(op='resume')
        assert stats()['metrics'][0]['total'] == 3
    finally:
        backend.close()
    backend = Backend('rust', env, demo=True)
    try:
        assert stats()['metrics'][0]['total'] > 3
        assert db.execute("SELECT COUNT(*) FROM events WHERE action='open'").fetchone()[0] == 3
    finally:
        backend.close()
        db.close()
print('PASS stats opening, resume, navigation, legacy history, stats opt-out, re-enable, independent command history, disabled history and demo isolation')
