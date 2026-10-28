#!/usr/bin/env python3
# Run: python3 apps/runtjik/scripts/benchmark.py /tmp/new-results.json
import json,os,platform,statistics,subprocess,time,sys,tempfile,argparse
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/"tests"))
from private_compositor import private_compositor
original_xdg={key:os.environ.get(key) for key in ("XDG_CONFIG_HOME","XDG_CACHE_HOME","XDG_DATA_HOME")}
from pathlib import Path
parser=argparse.ArgumentParser(description='Fi launcher benchmark; requires a TjikOS source checkout and installed Hamr/Runtjik.')
parser.add_argument('output', type=Path)
OUT=parser.parse_args().output
if OUT.exists():raise SystemExit('Choose a new output path')
WORK=Path(tempfile.mkdtemp(prefix='runtjik-benchmark-'))
print('Private logs: '+str(WORK),flush=True)
UNITS=['hamr-gtk.service','hamr-daemon.service']
def run(*args,check=True):
    return subprocess.run(args,text=True,capture_output=True,check=check).stdout.strip()
def wait(fn,timeout=15):
    deadline=time.monotonic()+timeout
    while time.monotonic()<deadline:
        value=fn()
        if value:return value
        time.sleep(.01)
    raise RuntimeError('Timed out waiting for launcher')
def mapped(app):
    surface=any(l['namespace']==app and l.get('alpha',1)>0 for m in json.loads(run('hyprctl','-j','layers')).values() for ls in m['levels'].values() for l in ls)
    if app=='runtjik' and surface:
        try:return json.loads(run('runtjik','status'))['ready']
        except (ValueError,subprocess.CalledProcessError):return False
    return surface

def process_alive(pid):
    try:return Path(f'/proc/{pid}/stat').read_text().rsplit(')',1)[1].split()[0]!='Z'
    except OSError:return False

def roots():return [p.pid for p in hamr_processes]
def memory(root_pids):
    procs={}
    for path in Path('/proc').glob('[0-9]*'):
        try:
            stat=(path/'stat').read_text().rsplit(')',1)[1].split();procs[int(path.name)]=int(stat[1])
        except (OSError,ValueError):pass
    included=set(root_pids)-{0}
    while True:
        more={pid for pid,parent in procs.items() if parent in included}-included
        if not more:break
        included|=more
    rows=[]
    for pid in sorted(included):
        try:
            values={k:int(v.split()[0]) for line in Path(f'/proc/{pid}/smaps_rollup').read_text().splitlines()[1:] if ':' in line for k,v in [line.split(':',1)]}
            rows.append({'process':Path(f'/proc/{pid}/comm').read_text().strip(),'pss_kib':values['Pss'],'rss_kib':values['Rss']})
        except (OSError,KeyError):pass
    return {'pss_mib':round(sum(x['pss_kib'] for x in rows)/1024,2),'rss_mib':round(sum(x['rss_kib'] for x in rows)/1024,2),'processes':rows}
hamr_processes=[]
def stop_hamr():
    for p in reversed(hamr_processes):
        p.terminate()
        try:p.wait(timeout=5)
        except subprocess.TimeoutExpired:p.kill();p.wait()
    hamr_processes.clear()
def start_hamr():
    log=open(WORK/'hamr.log','w')
    for cmd in [['/usr/local/bin/hamr-daemon'],[str(Path.home()/'.local/share/hamr-layout/hamr-gtk')]]:
        hamr_processes.append(subprocess.Popen(cmd,stdout=log,stderr=log))
    log.close()
result={'date':time.strftime('%Y-%m-%d'),'host':'Fi','kernel':platform.release(),'hamr':run('hamr','--version'),'runtjik':run('runtjik','--version'),'quickshell':run('quickshell','--version'),'method':{'trials':5,'settle_seconds':3,'os_caches':'warm; not dropped','runtjik_index':'existing cached real index','endpoint':'Hyprland visible mapped surface; Runtjik top layer ready','poll_seconds':.01,'hamr_show':'one show request after GTK logs its daemon connection','environment':'private nested Hyprland with normal user D-Bus, real user config/index/icon caches, direct process launch for both', 'memory':'PSS/RSS from smaps_rollup of new launcher roots and descendants only, excluding pre-existing service-cgroup applications'},'trials':[]}
with private_compositor():
    for key,value in original_xdg.items():
        if value is None:os.environ.pop(key,None)
        else:os.environ[key]=value
    os.environ['GDK_BACKEND']='wayland'
    run('runtjik','close',check=False)
    stop_hamr()
    try:
        for i in range(5):
            start=time.monotonic();start_hamr()
            wait(lambda: 'Connected to hamr daemon' in Path(WORK/'hamr.log').read_text())
            run('hamr','show')
            wait(lambda:mapped('hamr'))
            elapsed=time.monotonic()-start
            time.sleep(3)
            row={'app':'hamr','trial':i+1,'cold_mapped_s':round(elapsed,4),'visible_memory':memory(roots())}
            run('hamr','hide');wait(lambda:not mapped('hamr'));time.sleep(.3)
            row['hidden_memory']=memory(roots())
            start=time.monotonic();run('hamr','show');wait(lambda:mapped('hamr'))
            row['warm_mapped_s']=round(time.monotonic()-start,4)
            stop_hamr()
            assert row['visible_memory']['pss_mib']>0
            result['trials'].append(row);OUT.write_text(json.dumps(result,indent=2)+'\n');print(row['app'],row['trial'],row['cold_mapped_s'],row['visible_memory']['pss_mib'],flush=True)
            start=time.monotonic()
            with open(WORK/'runtjik.log','a') as log:
                p=subprocess.Popen(['runtjik','show'],stdout=log,stderr=log)
            try:
                wait(lambda:mapped('runtjik'))
                elapsed=time.monotonic()-start
                status=json.loads(run('runtjik','status'))
                time.sleep(3)
                assert process_alive(status['pid']), 'Runtjik exited during trial'
                row={'app':'runtjik','trial':i+1,'cold_mapped_s':round(elapsed,4),'indexed':status['indexed'],'visible_memory':memory([status['pid']])}
            finally:
                run('runtjik','close',check=False)
                try:p.wait(timeout=5)
                except subprocess.TimeoutExpired:p.terminate();p.wait(timeout=5)
            wait(lambda:not process_alive(status['pid']))
            row['closed_root_exited']=not process_alive(status['pid'])
            assert row['visible_memory']['pss_mib']>0
            result['trials'].append(row);OUT.write_text(json.dumps(result,indent=2)+'\n');print(row['app'],row['trial'],row['cold_mapped_s'],row['visible_memory']['pss_mib'],flush=True)
    finally:
        stop_hamr()
    for app in ['hamr','runtjik']:
        rows=[r for r in result['trials'] if r['app']==app]
        print(app,'startup median',statistics.median(r['cold_mapped_s'] for r in rows),'PSS median',statistics.median(r['visible_memory']['pss_mib'] for r in rows),flush=True)
