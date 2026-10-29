#!/usr/bin/python3
"""Guarded SnapRAID maintenance; never uses force flags."""
import collections,datetime,fcntl,json,os,pathlib,subprocess,sys
STATE=pathlib.Path('/var/lib/si-snapraid-maintenance')
CONFIG=pathlib.Path('/etc/si-snapraid-maintenance/config.json')
def now():return datetime.datetime.now(datetime.timezone.utc).isoformat()
def save(name,data):
 p=STATE/name;t=p.with_suffix('.tmp');t.write_text(json.dumps(data,indent=2));t.replace(p)
def run(args,**kw):return subprocess.run(args,check=True,**kw)
def output(args):return subprocess.check_output(args,text=True).strip()
def guard(config):
 for mount,uuid in config['mounts'].items():
  if output(['findmnt','-n','-o','UUID','--mountpoint',mount])!=uuid:raise RuntimeError('Wrong or missing disk: '+mount)
  options=output(['findmnt','-n','-o','OPTIONS','--mountpoint',mount]).split(',')
  if 'rw' not in options:raise RuntimeError('Read-only disk: '+mount)
 actual=[]
 for line in pathlib.Path('/etc/snapraid.conf').read_text().splitlines():
  words=line.split()
  if words and words[0] in ('data','1-parity','2-parity','content'):actual.append(words)
 if actual!=config['layout']:raise RuntimeError('SnapRAID layout changed; review maintenance config')
 for proc in pathlib.Path('/proc').glob('[0-9]*/comm'):
  try:
   if proc.read_text().strip()=='snapraid':raise RuntimeError('Another SnapRAID process is running')
  except (FileNotFoundError,ProcessLookupError):pass
def validate_diff(text,max_removed):
 # Require the final summary, so a truncated diff cannot silently pass.
 match=__import__('re').search(r'^\s*(\d+)\s+removed\s*$',text,__import__('re').M)
 if not match:raise RuntimeError('Missing diff summary')
 removed=int(match[1])
 if removed>max_removed:raise RuntimeError(f'{removed} removals exceed limit {max_removed}; review diff before any sync')
 return removed
def resume():
 p=STATE/'paused.json'
 if not p.exists():return
 failures=[]
 for container in json.loads(p.read_text()):
  r=subprocess.run(['docker','inspect','--format','{{.State.Paused}}',container],capture_output=True,text=True)
  if r.returncode:failures.append(container);continue
  if r.stdout.strip()=='true':
   if subprocess.run(['docker','unpause',container]).returncode:failures.append(container)
 if failures:raise RuntimeError('Could not resume: '+','.join(failures))
 p.unlink()
def verify_status():
 # Serialize a read-only post-update check without waiting behind a long sync.
 with open('/run/lock/si-snapraid-maintenance.lock','w') as lock:
  try:fcntl.flock(lock,fcntl.LOCK_EX|fcntl.LOCK_NB)
  except BlockingIOError:
   print('SnapRAID readability check deferred: maintenance is running.',flush=True)
   return 75
  config=json.loads(CONFIG.read_text());guard(config)
  run(['snapraid','status'])
 return 0
def main():
 if sys.argv[1]=='status':raise SystemExit(verify_status())
 os.umask(0o077);STATE.mkdir(parents=True,exist_ok=True)
 lock=open('/run/lock/si-snapraid-maintenance.lock','w');fcntl.flock(lock,fcntl.LOCK_EX)
 mode=sys.argv[1]
 if mode=='resume':resume();return
 config=json.loads(CONFIG.read_text());guard(config)
 if mode=='sync' and (STATE/'scrub-failure.json').exists():raise RuntimeError('Previous scrub failed; investigate and complete a successful scrub before syncing')
 if mode=='guard':print('All seven disk identities and configuration layout verified.');return
 backup_lock=open('/run/lock/si-app-backup.lock','w');fcntl.flock(backup_lock,fcntl.LOCK_EX)
 guard(config);resume()
 logdir=pathlib.Path('/var/log/si-snapraid-maintenance');logdir.mkdir(exist_ok=True)
 stamp=datetime.datetime.now().strftime('%Y%m%d-%H%M%S')
 try:
  if mode in ('sync','preflight'):
   if mode=='sync':
    containers=json.loads(output(['docker','ps','-q']) and output(['docker','inspect',*output(['docker','ps','-q']).split()]) or '[]')
    ids=[c['Id'] for c in containers if c['Name'].lstrip('/') in config['writers'] and not c['State']['Paused']]
    save('paused.json',ids)
    for cid in ids:run(['docker','pause',cid])
   diff=subprocess.run(['snapraid','diff'],capture_output=True,text=True)
   (logdir/(stamp+'-diff.log')).write_text(diff.stdout+diff.stderr)
   if diff.returncode not in (0,2):raise RuntimeError('SnapRAID diff failed; inspect log')
   removed=validate_diff(diff.stdout,config['max_removed'])
   print('Diff gate passed; removed files:',removed,flush=True)
   if mode=='preflight':return
   run(['snapraid','-l',str(logdir/(stamp+'-sync.log')),'sync'])
  elif mode=='scrub':
   run(['snapraid','-l',str(logdir/(stamp+'-scrub.log')),'-p','25','-o','7','scrub'])
  else:raise RuntimeError('Unknown mode')
  save('last-'+mode+'-success.json',{'completed_at':now(),'mode':mode})
  (STATE/(mode+'-failure.json')).unlink(missing_ok=True)
 except BaseException as exc:
  save(mode+'-failure.json',{'failed_at':now(),'error':str(exc)})
  raise
 finally:
  resume()
if __name__=='__main__':main()
