#!/usr/bin/python3
"""Copy the retained consistent si snapshots into an independent local repository."""
import datetime, fcntl, json, os, pathlib, shlex, subprocess, sys, time
from zoneinfo import ZoneInfo

def weekly_copy_current(receipt, now):
 """A verified copy must have completed since the latest Sunday 06:00 HKT."""
 local = now.astimezone(ZoneInfo('Asia/Hong_Kong'))
 due = (local - datetime.timedelta(days=(local.weekday() + 1) % 7)).replace(hour=6, minute=0, second=0, microsecond=0)
 if local < due:
  due -= datetime.timedelta(days=7)
 try:
  copied = datetime.datetime.fromisoformat(receipt['copiedAt'])
  return receipt.get('dataCheck') == 'passed' and due <= copied <= now and bool(receipt.get('sourceTime'))
 except (KeyError, TypeError, ValueError):
  return False

os.umask(0o077)
home=pathlib.Path.home();base=home/'.local/share/si-app-backups';config=home/'.config/si-app-backup'
base.mkdir(mode=0o700,parents=True,exist_ok=True)
lock=(base/'copy.lock').open('w');fcntl.flock(lock,fcntl.LOCK_EX)
ssh_base=['/usr/bin/mullvad-exclude','/usr/bin/ssh','-x','-o','HostKeyAlias=ssh.type.hk','-o','StrictHostKeyChecking=yes','-o','BatchMode=yes','-o','ConnectTimeout=8','-o','ServerAliveInterval=30','-o','ServerAliveCountMax=6']
endpoints=[os.environ['SI_BACKUP_SSH_HOST']] if 'SI_BACKUP_SSH_HOST' in os.environ else ['m@192.168.1.150','m@ssh.type.hk']
ssh=None
for endpoint in endpoints:
 try:
  probe=subprocess.run(ssh_base+[endpoint,'hostname'],capture_output=True,text=True,timeout=40)
  if probe.returncode==0 and probe.stdout.strip()=='si':ssh=ssh_base+[endpoint];break
 except subprocess.TimeoutExpired:pass
if ssh is None:raise RuntimeError('si is unreachable using its verified SSH endpoints')
sftp=shlex.join(ssh+['sudo','-n','/usr/libexec/openssh/sftp-server'])
source='sftp:si:/mnt/data/disk42/.recovery-preservation/app-backups/repository'
common=['restic','--retry-lock','15m','--limit-download','2048','-o','sftp.command='+sftp]
os.environ.update(RESTIC_REPOSITORY=str(base/'repository'),RESTIC_PASSWORD_FILE=str(config/'local-password'),RESTIC_CACHE_DIR=str(base/'cache'))
remote=['--from-repo',source,'--from-password-file',str(config/'server-password')]
def run(args,**kw):return subprocess.run(args,check=True,**kw)
query=run(common+['-r',source,'-p',str(config/'server-password'),'snapshots','--host','si','--tag','app-state','--json'],capture_output=True,text=True)
snapshots=json.loads(query.stdout)
if not snapshots:raise RuntimeError('No si backup found')
# Source paths expanded to include the library. Select across old/new path sets;
# Restic --latest groups by paths, while --group-by changes its JSON structure.
snapshot=max(snapshots,key=lambda item:datetime.datetime.fromisoformat(item['time'].replace('Z','+00:00')))
stamp=datetime.datetime.fromisoformat(snapshot['time'].replace('Z','+00:00'))
health='--health-check' in sys.argv
max_age=24 if health else 36
if datetime.datetime.now(datetime.timezone.utc)-stamp>datetime.timedelta(hours=max_age):raise RuntimeError('Server backup is stale')
if health:
 try:
  receipt=json.loads((base/'last-success.json').read_text())
 except (OSError, ValueError):
  receipt={}
 if isinstance(receipt,dict) and weekly_copy_current(receipt,datetime.datetime.now(datetime.timezone.utc)) and (base/'repository/config').exists():
  local_snapshots=json.loads(run(['restic','--retry-lock','15m','snapshots','--host','si','--tag','app-state','--json'],capture_output=True,text=True).stdout)
  if any(item['time']==receipt['sourceTime'] for item in local_snapshots):
   print('Server backup is fresh; verified copy for the latest Sunday is present.');sys.exit(0)
 print('Weekly verified copy is missing or overdue; fetching it now.',flush=True)
if not (base/'repository/config').exists():run(common+['init','--copy-chunker-params',*remote])
for attempt in range(1,4):
 try:
  run(common+['copy',*remote,'--host','si','--tag','app-state'])
  break
 except subprocess.CalledProcessError:
  if attempt==3:raise
  print(f'Copy interrupted; reconnecting in {attempt*10}s. Existing repository chunks are retained.',flush=True)
  time.sleep(attempt*10)
run(['restic','--retry-lock','15m','forget','--host','si','--tag','app-state','--group-by','host,tags','--keep-daily','7','--keep-weekly','4','--prune'])
run(['restic','--retry-lock','15m','check','--read-data'])
receipt={'copiedAt':datetime.datetime.now(datetime.timezone.utc).isoformat(),'sourceSnapshot':snapshot['id'],'sourceTime':snapshot['time'],'dataCheck':'passed'}
tmp=base/'last-success.tmp';tmp.write_text(json.dumps(receipt,indent=2));tmp.replace(base/'last-success.json')
print(json.dumps(receipt),flush=True)
