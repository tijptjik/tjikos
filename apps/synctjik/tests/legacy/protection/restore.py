#!/usr/bin/env python3
"""Explicit restore into fresh staging and empty destinations; never run by tjikup."""
from contextlib import closing
import argparse, fcntl, hashlib, json, os, pathlib, re, sqlite3, subprocess
from storage import CONFIG, check_mounts
P=pathlib.Path
DEFAULTS=['/mnt/storage/meta/appData','/mnt/storage/meta/appCache/audiobookshelf','/mnt/storage/meta/opt/plexmediaserver','/appData/mariadb/config','/home/m/.ssh','/home/m/.keys']
EXTRA=['/home/m/.config','/home/m/dockers','/home/m/.local/bin','/etc/nginx','/etc/pki/tls/private','/etc/pki/tls/certs','/etc/si-notifications']

def run(args,**kwargs):return subprocess.run(args,check=True,**kwargs)

def require_empty(path):
    if path.is_symlink():raise RuntimeError('Refusing destination symlink: '+str(path))
    if path.exists() and (not path.is_dir() or any(path.iterdir())):
        raise RuntimeError('Destination is not empty; refusing overwrite: '+str(path))

def root_for(stage):
    found=list(stage.rglob('backup-manifest.json'))
    if len(found)!=1:raise RuntimeError('Expected exactly one backup manifest in staging')
    return found[0].parent

def quiet_services(allow_mariadb=False):
    names=subprocess.check_output(['docker','ps','--format','{{.Names}}'],text=True).split() if subprocess.run(['systemctl','is-active','--quiet','docker']).returncode==0 else []
    if set(names)-({'mariadb'} if allow_mariadb else set()):raise RuntimeError('Stop containers explicitly before restoring')
    if subprocess.run(['systemctl','is-active','--quiet','plexmediaserver']).returncode==0:raise RuntimeError('Stop Plex explicitly before restoring')

def validate(root):
    manifest=json.loads((root/'backup-manifest.json').read_text())
    if not manifest.get('sqlite') or manifest.get('mariadbDump')!='database-dumps/mariadb.sql':raise RuntimeError('Unsupported backup manifest')
    for record in manifest['sqlite']:
        path=P(record['path'])
        if not path.is_absolute() or '..' in path.parts:raise RuntimeError('Invalid manifest path')
        db=root/str(path).lstrip('/')
        if not db.is_file() or db.is_symlink():raise RuntimeError('Missing database in restore')
        if '/plexmediaserver/' in str(path):
            result=subprocess.check_output(['/usr/lib/plexmediaserver/Plex SQLite',str(db),'PRAGMA quick_check;'],text=True).strip()
            if result!='ok':raise RuntimeError('Plex snapshot integrity failure')
        else:
            with closing(sqlite3.connect(db.as_uri()+'?mode=ro',uri=True)) as c:
                if c.execute('PRAGMA quick_check').fetchall()!=[('ok',)]:raise RuntimeError('SQLite snapshot integrity failure: '+str(path))
    dump=root/'database-dumps/mariadb.sql'
    if not dump.is_file() or dump.stat().st_size<1000:raise RuntimeError('Missing/empty MariaDB dump')

def stage_restore(args):
    if not re.fullmatch('[0-9a-f]{64}',args.snapshot or ''):raise RuntimeError('Supply an exact 64-character snapshot ID; latest is deliberately unsupported')
    if not args.repository or not args.password_file:raise RuntimeError('Repository and password-file required')
    if args.stage.exists():raise RuntimeError('Staging directory must not already exist')
    args.stage.mkdir(parents=True,mode=0o700)
    env={**os.environ,'RESTIC_REPOSITORY':args.repository,'RESTIC_PASSWORD_FILE':str(args.password_file)}
    run(['restic','restore',args.snapshot,'--target',str(args.stage),'--verify'],env=env)
    validate(root_for(args.stage))
    print('Staged and verified. No live configuration changed.')

def install(args):
    check_mounts(json.loads(CONFIG.read_text()));quiet_services()
    root=root_for(args.stage);validate(root)
    selected=args.include or DEFAULTS
    for target in selected:
        if target not in DEFAULTS+EXTRA:raise RuntimeError('Path is outside restore allowlist: '+target)
        source=root/target.lstrip('/')
        if not source.is_dir() or source.is_symlink():raise RuntimeError('Missing restore source: '+target)
        require_empty(P(target))
    # Complete all prechecks before copying any live data. No --delete, no overwrite.
    for target in selected:
        dest=P(target);dest.mkdir(parents=True,exist_ok=True)
        run(['rsync','-aHAX','--numeric-ids','--ignore-existing',str(root/target.lstrip('/'))+'/',target+'/'])
    print('Selected state restored. Services remain stopped. MariaDB logical import is separate.')

def import_mariadb(args):
    check_mounts(json.loads(CONFIG.read_text()));quiet_services(True)
    root=root_for(args.stage);validate(root)
    container=json.loads(subprocess.check_output(['docker','inspect','mariadb']))[0]
    if not any(m['Source']=='/appData/mariadb/config' and m['Destination']=='/config' for m in container['Mounts']):raise RuntimeError('Unexpected MariaDB target mount')
    query="SELECT COUNT(*) FROM information_schema.tables WHERE table_schema NOT IN ('mysql','information_schema','performance_schema','sys');"
    count=subprocess.check_output(['docker','exec','mariadb','mariadb','-uroot','-Nse',query],text=True).strip()
    if count!='0':raise RuntimeError('MariaDB contains application tables; refusing replacement')
    dump=root/'database-dumps/mariadb.sql'
    receipt=P('/var/lib/si-provision/mariadb-restored.json')
    if receipt.exists():raise RuntimeError('MariaDB restore receipt exists; refusing repeated import')
    # A failed/partial import also leaves the receipt, requiring manual diagnosis.
    receipt.parent.mkdir(parents=True,exist_ok=True)
    receipt.write_text(json.dumps({'sha256':hashlib.sha256(dump.read_bytes()).hexdigest(),'status':'started'}))
    with dump.open('rb') as f:run(['docker','exec','-i','mariadb','mariadb','-uroot'],stdin=f)
    receipt.write_text(json.dumps({'sha256':hashlib.sha256(dump.read_bytes()).hexdigest(),'status':'complete'}))
    print('MariaDB imported into checked fresh instance. Other services remain stopped.')

def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('action',choices=['stage','install','import-mariadb'])
    p.add_argument('--stage',type=P,required=True)
    p.add_argument('--repository');p.add_argument('--password-file',type=P);p.add_argument('--snapshot')
    p.add_argument('--include',action='append',help='Explicit allowlisted target; otherwise default app state and SSH/age secrets')
    args=p.parse_args()
    if os.geteuid()!=0:raise RuntimeError('Root required')
    if not args.stage.is_absolute():raise RuntimeError('Staging path must be absolute')
    os.umask(0o077)
    with open('/run/lock/si-app-backup.lock','w') as lock:
        fcntl.flock(lock,fcntl.LOCK_EX|fcntl.LOCK_NB)
        {'stage':stage_restore,'install':install,'import-mariadb':import_mariadb}[args.action](args)
if __name__=='__main__':main()
