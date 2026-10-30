"""Repeatable synthetic worker measurements; no global filesystem cache drops."""
import json, os, select, statistics, subprocess, sys, tempfile, time, sqlite3, argparse
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
BINARY=Path(os.environ.get('NOTJIK_TEST_BINARY',ROOT/'dist/bin/notjik-core'))
reference=ROOT/'tests/reference/notjik.py'

parser=argparse.ArgumentParser();parser.add_argument("--events",type=int,default=0);options=parser.parse_args()
result={}
with tempfile.TemporaryDirectory(prefix='notjik-bench-') as directory:
    directory=Path(directory)
    env={**os.environ,'NOTJIK_HOME':str(ROOT),'NOTJIK_CONFIG':str(directory/'config.json'),
         'NOTJIK_STATE_DIR':str(directory/'state'),'XDG_RUNTIME_DIR':str(directory),'WAYLAND_DISPLAY':'absent'}
    (directory/'config.json').write_text('{}')
    for name,command in [('python',[sys.executable,'-B','-c',f'import sys;sys.path.insert(0,{str(reference.parent)!r});from pathlib import Path;from notjik import worker;worker(Path({str(ROOT)!r}))']),('rust',[str(BINARY),'worker'])]:
        env['NOTJIK_STATE_DIR']=str(directory/name)
        if options.events:
            state=Path(env['NOTJIK_STATE_DIR']);state.mkdir()
            with sqlite3.connect(state/'history.sqlite3') as db:
                db.execute('CREATE TABLE events(id INTEGER PRIMARY KEY,time TEXT NOT NULL,event TEXT NOT NULL,notice_id TEXT,app TEXT,payload TEXT NOT NULL)')
                db.execute('CREATE TABLE avatars(chat_id TEXT PRIMARY KEY,title TEXT NOT NULL,image TEXT NOT NULL)')
                for index in range(options.events):
                    event={'session':'synthetic','event':'received','time':'2026-10-30T00:00:00.000+00:00','notice':{'id':str(index),'app':'synthetic benchmark','title':f'Fixture {index}','body':'Synthetic notification content','received':index}}
                    db.execute('INSERT INTO events(time,event,notice_id,app,payload) VALUES(?,?,?,?,?)',(event['time'],'received',str(index),'synthetic benchmark',json.dumps(event)))
        starts=[];rss=[];lookups=[]
        for iteration in range(11):
            start=time.perf_counter()
            process=subprocess.Popen(command,env=env,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
            buffer=b''
            def response():
                global buffer
                deadline=time.monotonic()+5
                while time.monotonic()<deadline:
                    if b'\n' not in buffer:
                        if not select.select([process.stdout],[],[],.1)[0]:continue
                        block=os.read(process.stdout.fileno(),65536)
                        if not block:raise RuntimeError(process.stderr.read().decode())
                        buffer+=block
                    while b'\n' in buffer:
                        raw,buffer=buffer.split(b'\n',1)
                        item=json.loads(raw)
                        if 'history' in item:return item
                raise TimeoutError('worker history response')
            try:
                response();starts.append((time.perf_counter()-start)*1000)
                status=Path(f'/proc/{process.pid}/status').read_text()
                rss.append(int(next(line for line in status.splitlines() if line.startswith('VmRSS:')).split()[1]))
                if iteration==10:
                    for index in range(10):
                        time.sleep(.22)
                        start=time.perf_counter();process.stdin.write(b'{"op":"history"}\n');process.stdin.flush();response()
                        lookups.append((time.perf_counter()-start)*1000)
            finally:
                process.stdin.close();process.wait(timeout=3)
                process.stdout.close();process.stderr.close()
        result[name]={'first_launch_ms':round(starts[0],3),'warm_launch_p50_ms':round(statistics.median(starts[1:]),3),
                      'history_request_p50_ms':round(statistics.median(lookups),3),'worker_rss_kib':int(statistics.median(rss))}
print(json.dumps({'method':f'{options.events} synthetic history events; first process launch, ten warm launches, ten steady-worker history requests; no filesystem cache flush', 'results':result},indent=2))
