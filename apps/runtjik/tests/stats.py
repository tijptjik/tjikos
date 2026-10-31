"""Stats lifecycle contracts using isolated synthetic history."""
from pathlib import Path
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
    finally:
        backend.close()
    with Path(env['RUNTJIK_CONFIG']).open('a') as config:
        config.write('\n[history]\nenabled=false\n')
    backend = Backend('rust', env)
    try:
        backend.request(op='resume')
        assert stats()['metrics'][0]['total'] == 2
    finally:
        backend.close()
    backend = Backend('rust', env, demo=True)
    try:
        assert stats()['metrics'][0]['total'] > 2
        assert db.execute("SELECT COUNT(*) FROM events WHERE action='open'").fetchone()[0] == 2
    finally:
        backend.close()
        db.close()
print('PASS stats opening, resume, navigation, legacy history, disabled history and demo isolation')
