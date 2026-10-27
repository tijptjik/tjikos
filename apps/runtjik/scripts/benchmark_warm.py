import json,subprocess,time,statistics
from pathlib import Path

def run(action):subprocess.run(['runtjik',action],check=True,stdout=subprocess.DEVNULL)
def status():return json.loads(subprocess.check_output(['runtjik','status']))
def memory(pid):
    parents={}
    for p in Path('/proc').glob('[0-9]*/stat'):
        try:parents[int(p.parent.name)]=int(p.read_text().rsplit(')',1)[1].split()[1])
        except (OSError,ValueError,IndexError):pass
    ids={pid}
    while True:
        more={p for p,parent in parents.items() if parent in ids}
        if more<=ids:break
        ids|=more
    pss=rss=0
    for p in ids:
        try:
            for line in Path(f'/proc/{p}/smaps_rollup').read_text().splitlines():
                if line.startswith('Pss:'):pss+=int(line.split()[1])
                if line.startswith('Rss:'):rss+=int(line.split()[1])
        except OSError:pass
    return {'pss_mib':round(pss/1024,2),'rss_mib':round(rss/1024,2),'processes':len(ids)}
result={'version':'0.6.0','host':'Fi','date':time.strftime('%Y-%m-%d'),'method':'CLI invocation to confirmed layer/input readiness; live desktop, OS caches retained. PSS/RSS sum root and descendants three seconds after open/hide.','cold_ms':[],'warm_ms':[]}
for i in range(5):
    run('quit');start=time.monotonic();run('show');result['cold_ms'].append(round((time.monotonic()-start)*1000,2))
    state=status();assert state['ready'] and state['visible'] and state['version']=='0.6.0',state
pid=state['pid'];time.sleep(3);result['open_memory']=memory(pid)
for i in range(5):
    run('close');assert status()['pid']==pid and not status()['visible']
    start=time.monotonic();run('show');result['warm_ms'].append(round((time.monotonic()-start)*1000,2))
    state=status();assert state['pid']==pid and state['ready'] and state['visible'],state
result['toggle_ready_ms']=[]
for i in range(5):
    run('close');start=time.monotonic();run('toggle')
    for _ in range(100):
        state=status()
        if state['ready'] and state['visible']:break
        time.sleep(.002)
    assert state['ready'] and state['visible'] and state['pid']==pid
    result['toggle_ready_ms'].append(round((time.monotonic()-start)*1000,2))
run('close');time.sleep(3);result['hidden_memory']=memory(pid)
result['toggle_ready_median_ms']=statistics.median(result['toggle_ready_ms'])
result['cold_median_ms']=statistics.median(result['cold_ms']);result['warm_median_ms']=statistics.median(result['warm_ms']);result['indexed']=state['indexed']
Path('/tmp/runtjik-warm-0.6.0.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result))
