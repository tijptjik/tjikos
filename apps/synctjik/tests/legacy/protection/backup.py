#!/usr/bin/python3
"""Online database snapshots, followed by encrypted/deduplicated retention."""
import datetime, fcntl, json, os, pathlib, subprocess, sys, sqlite3, time, stat
BASE=pathlib.Path('/mnt/data/disk42/.recovery-preservation/app-backups')
STAGE=BASE/'stage'
STATE=pathlib.Path('/var/lib/si-app-backup')
PASSWORD='/etc/si-app-backup/password'
LIBRARY=pathlib.Path('/mnt/storage/public/library')
EXPECTED_UUID='9d49130f-b477-4990-b2c2-93ebebf2d0c6'
os.umask(0o077)
def run(args,**kw):
 print('+', ' '.join(str(a) for a in args),flush=True)
 return subprocess.run([str(a) for a in args],check=True,**kw)
def output(args):return subprocess.check_output(args,text=True).strip()
def save(p,data):
 tmp=p.with_suffix('.tmp');tmp.write_text(json.dumps(data,indent=2));os.replace(tmp,p)
STATE.mkdir(parents=True,exist_ok=True)
lock=open('/run/lock/si-app-backup.lock','w');fcntl.flock(lock,fcntl.LOCK_EX)
if output(['findmnt','-n','-o','UUID','--mountpoint','/mnt/data/disk42'])!=EXPECTED_UUID:raise RuntimeError('Backup disk UUID mismatch')
for line in pathlib.Path('/etc/snapraid.conf').read_text().splitlines():
 parts=line.split()
 if len(parts)>=3 and parts[0]=='data' and not os.path.ismount(parts[2]):
  raise RuntimeError('Data disk is not mounted: '+parts[2])
if not os.path.ismount('/mnt/storage') or not LIBRARY.is_dir():
 raise RuntimeError('Required library source or storage mount missing')
BASE.mkdir(parents=True,exist_ok=True);STAGE.mkdir(exist_ok=True)
os.environ.update(RESTIC_REPOSITORY=str(BASE/'repository'),RESTIC_PASSWORD_FILE=PASSWORD,RESTIC_CACHE_DIR=str(BASE/'cache'))
if not (BASE/'repository/config').exists():run(['restic','--retry-lock','15m','init'])
# Each exclusion is relative to its source; retain unknown app data by default.
app_excludes=['/docker/','/flatpak/','/booklore.bak/','*.fq.gz','*/Backups/','*/backups/',
 '/radarr/MediaCover/','/sonarr/MediaCover/','/lidarr/MediaCover/','/bookshelf/MediaCover/',
 '/radarr/logs/','/sonarr/logs/','/lidarr/logs/','/bookshelf/logs/','/prowlarr/logs/',
 '/seerr/logs/','/seerr/cache/',
 '/profilarr/logs/','/profilarr/cache/',
 '/tautulli/logs/','/tautulli/cache/','/lazylibrarian/log/','/lazylibrarian/cache/',
 '/navidrome/artwork/','/navidrome/cache/','/grimmory/cbx_cache/',
 '/beets/genres/.venv/','**/.git/','**/__pycache__/']
# Keep Grimmory images: they can contain user-supplied covers, unlike known download caches.
plex='/mnt/storage/meta/opt/plexmediaserver/Library/Application Support/Plex Media Server'
sources=[
 ('/mnt/storage/meta/appData',app_excludes),
 (plex,['/Cache/','/Codecs/','/Drivers/','/Media/','/Metadata/','/Updates/','/Logs/','/Crash Reports/','/Diagnostics/','/Plug-in Support/Caches/','/Plug-in Support/Databases/*.db-????-??-??']),
 ('/mnt/storage/meta/appCache/audiobookshelf',['/cache/','/logs/','/backups/']),
 ('/appData/mariadb/config',['/log/','/databases/']),
 ('/home/m/dockers',[]),('/home/m/.config',['/BraveSoftware/','/bun/bin/','/fnm/','/gcloud/logs/','/flatpak/']),
 ('/home/m/.local/bin',[]),('/home/m/.local/share/dotjik',['/.git/']),('/home/m/.local/share/systjik',['/.git/']),('/home/m/.ssh',[]),('/home/m/.keys',[]),
 ('/etc',['/si-app-backup/password','/nginx/.git/','/lvm/cache/','/httpd/logs/','**/__pycache__/'])]
for src,_ in sources:
 if not pathlib.Path(src).is_dir():raise RuntimeError('Required source missing: '+src)
def stage(checksum=False):
 for src,excludes in sources:
  dest=STAGE/src.lstrip('/');dest.mkdir(parents=True,exist_ok=True)
  args=['rsync','-aHAX','--numeric-ids','--delete','--delete-excluded']
  if checksum:args+=['--checksum']
  for pattern in excludes:args+=['--exclude',pattern]
  try:
   run(args+[src+'/',str(dest)+'/'])
  except subprocess.CalledProcessError as exc:
   if checksum or exc.returncode != 24:raise
   print('Live source files vanished; transactional database exports follow.',flush=True)
# Rsync configurations first; replace all SQLite files with transactional snapshots.
stage()
started=datetime.datetime.now(datetime.timezone.utc).isoformat()
sqlite_results=[]
historical_sqlite=[]
def is_historical_plex_database(source):
 return source.parent==pathlib.Path(plex)/'Plug-in Support/Databases' and source.name.endswith('.db.bak')
for staged in [p for source_root,_ in sources for p in (STAGE/source_root.lstrip('/')).rglob('*')]:
 if not staged.is_file() or staged.is_symlink():continue
 with staged.open('rb') as f:header=f.read(16)
 if header!=b'SQLite format 3\x00':continue
 source=pathlib.Path('/')/staged.relative_to(STAGE)
 if not source.is_file():raise RuntimeError('Database disappeared: '+str(source))
 # Plex keeps pre-migration .db.bak files here. Preserve their original bytes,
 # but do not present historical artifacts as validated live restore databases.
 if is_historical_plex_database(source):
  check=subprocess.run(['/usr/lib/plexmediaserver/Plex SQLite','-readonly',str(staged),'PRAGMA quick_check;'],text=True,capture_output=True,check=True)
  checks=check.stdout.strip().splitlines()
  historical_sqlite.append({'path':str(source),'quickCheck':checks,'bytes':staged.stat().st_size,'role':'historical Plex database backup; not a live restore database'})
  print('Historical Plex database preserved:',source,'quick_check:',checks,flush=True)
  continue
 tmp=staged.with_name(staged.name+'.backup-tmp')
 if tmp.exists():tmp.unlink()
 before=source.stat();deadline=time.monotonic()+180
 def progress(status,remaining,total):
  if time.monotonic()>deadline:raise TimeoutError('SQLite backup timed out: '+str(source))
 src=sqlite3.connect(source.as_uri()+'?mode=ro',uri=True,timeout=30)
 dst=sqlite3.connect(str(tmp))
 try:
  src.backup(dst,pages=1024,progress=progress,sleep=0.05)
  try:
   checks=[row[0] for row in dst.execute('PRAGMA quick_check')]
  except sqlite3.DatabaseError:
   if not str(source).startswith(plex+'/'):raise
   checks=['deferred to Plex SQLite']
  repaired=[]
  if str(source)=='/mnt/storage/meta/appData/prowlarr/prowlarr.db' and checks==['wrong # of entries in index IX_History_DownloadId']:
   evidence=STAGE/'database-evidence';evidence.mkdir(exist_ok=True)
   original=evidence/'prowlarr-before-reindex.db'
   if original.exists():original.unlink()
   original_db=sqlite3.connect(str(original))
   try:dst.backup(original_db)
   finally:original_db.close()
   dst.execute('REINDEX');dst.commit()
   checks=[row[0] for row in dst.execute('PRAGMA integrity_check')]
   repaired=['all Prowlarr indexes']
   print('WARNING: rebuilt Prowlarr indexes in backup copy; original retained in database-evidence.',flush=True)
  if checks not in [['ok'],['deferred to Plex SQLite']]:raise RuntimeError('Database integrity failure: '+str(source))
 finally:dst.close();src.close()
 if checks==['deferred to Plex SQLite']:
  checks=output(['/usr/lib/plexmediaserver/Plex SQLite',str(tmp),'PRAGMA quick_check;']).splitlines()
  if checks!=['ok']:raise RuntimeError('Plex database integrity failure')
 os.chmod(tmp,stat.S_IMODE(before.st_mode));os.chown(tmp,before.st_uid,before.st_gid)
 os.replace(tmp,staged)
 for suffix in ['-wal','-shm','-journal']:
  sidecar=pathlib.Path(str(staged)+suffix)
  if sidecar.exists():sidecar.unlink()
 sqlite_results.append({'path':str(source),'quickCheck':'ok','rebuiltIndexesInBackupOnly':repaired,'bytes':staged.stat().st_size})
 print('SQLite snapshot verified:',source,flush=True)
# Root uses local socket authentication. Global read lock covers non-InnoDB tables too.
dumps=STAGE/'database-dumps';dumps.mkdir(exist_ok=True)
tmp=dumps/'mariadb.sql.tmp'
with tmp.open('wb') as f:
 run(['docker','exec','mariadb','mariadb-dump','-uroot','--all-databases','--lock-all-tables','--routines','--events','--triggers','--hex-blob'],stdout=f)
if tmp.stat().st_size<1000:raise RuntimeError('Unexpectedly empty MariaDB dump')
os.replace(tmp,dumps/'mariadb.sql')
containers=output(['docker','ps','--format','{{.Names}}']).splitlines()
save(STAGE/'backup-manifest.json',{'capturedAt':started,'sources':sources,'directSources':[str(LIBRARY)],'contentVerification':'Restic --force rereads all source bytes and deduplicates content chunks on every backup','runningContainers':containers,'consistency':'SQLite online backup API with quick_check; MariaDB locked logical dump; application configuration copied with rsync','sqlite':sqlite_results,'historicalSqlite':historical_sqlite,'mariadbDump':'database-dumps/mariadb.sql','mariadbRestore':'Import with compatible MariaDB; raw databases directory intentionally excluded'})
run(['restic','--retry-lock','15m','backup','--force',str(STAGE),str(LIBRARY),'--host','si','--tag','app-state'])
run(['restic','--retry-lock','15m','forget','--host','si','--tag','app-state','--group-by','host,tags','--keep-daily','7','--keep-weekly','4','--prune'])
run(['restic','--retry-lock','15m','check'])
save(STATE/'last-success.json',{'completedAt':datetime.datetime.now(datetime.timezone.utc).isoformat(),'repository':str(BASE/'repository')})
print('BACKUP SUCCESS',flush=True)
