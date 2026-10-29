"""Native protection failure paths. All state, credentials, processes and stores are synthetic.
The real Restic restore rehearsal uses only a temporary home and generated SQLite data.
"""
import ast
import contextlib
import datetime as dt
import fcntl
import hashlib
import http.server
import json
import os
from pathlib import Path
import shutil
import socket
import sqlite3
import subprocess
import tempfile
import threading
import time
import unittest

APP = Path(__file__).resolve().parents[1]
BINARY = APP / 'target/debug/synctjik'
FAKE = r'''#!/usr/bin/python3
import datetime, json, os, shutil, signal, subprocess, sys, time
from pathlib import Path
name=Path(sys.argv[0]).name; a=sys.argv[1:]; root=Path(os.environ['FIXTURE']); calls=root/'calls'
with calls.open('a') as f: f.write(json.dumps([name,*a])+'\n')
def fail(key):
    if os.environ.get(key): sys.exit(9)
if name=='findmnt':
    fail('FAIL_MOUNT')
    field=a[a.index('-o')+1] if '-o' in a else a[a.index('--output')+1]
    print({'UUID':'fixture-uuid','OPTIONS':'rw,relatime','FSTYPE':'fuse.rclone'}[field])
elif name=='mountpoint':
    target=Path(a[-1]); sys.exit(0 if target.name=='disk' or target.with_suffix('.mounted').exists() else 1)
elif name=='fusermount3':
    target=Path(a[-1]); target.with_suffix('.mounted').unlink(missing_ok=True)
    for p in target.iterdir():
        if p.is_file(): p.unlink()
elif name=='rclone':
    if 'lsf' in a:
        if not os.environ.get('EMPTY_SOURCE'): print('document.docx')
    elif 'mount' in a:
        target=Path(a[-1]); (target/'document.docx').write_bytes(b'fixture document export')
        target.with_suffix('.mounted').touch()
        while True: time.sleep(1)
elif name=='ssh': print('fixture-server')
elif name=='systemctl':
    if 'daemon-reload' in a: sys.exit(0)
    if 'is-active' in a:
        sys.exit(0 if os.environ.get('ACTIVE_WRITERS') else 3)
    unit=a[a.index('show')+1]
    status=json.loads((root/'unit-status.json').read_text()) if (root/'unit-status.json').exists() else {'ActiveState':'inactive','Result':'success','ExecMainStatus':'0'}
    if unit in status: status=status[unit]
    for k,v in status.items(): print(f'{k}={v}')
elif name=='docker':
    state=root/'paused';
    if a[:2]==['ps','-q']: print('fixture-writer')
    elif a[0]=='ps': print('mariadb\nwriter' if os.environ.get('ACTIVE_WRITERS') else 'mariadb')
    elif a[0]=='inspect' and '--format' in a:
        fail('FAIL_INSPECT'); print('true' if state.exists() else 'false')
    elif a[0]=='inspect' and a[1]=='mariadb':
        print(json.dumps([{'Mounts':[{'Source':str(root/'maria-config'),'Destination':'/config'}]}]))
    elif a[0]=='inspect': print(json.dumps([{'Id':'fixture-writer','Name':'/writer','State':{'Paused':state.exists()}}]))
    elif a[0]=='pause': state.touch(); fail('FAIL_PAUSE')
    elif a[0]=='unpause': fail('FAIL_RESUME'); state.unlink(missing_ok=True)
    elif 'mariadb-dump' in a: print('short' if os.environ.get('SHORT_DUMP') else '-- generated test dump\n'+'SELECT 1;\n'*150)
    elif '-Nse' in a: print('1' if os.environ.get('POPULATED_DB') else '0')
    elif '-i' in a: fail('FAIL_IMPORT'); sys.stdin.buffer.read()
elif name=='snapraid':
    if 'diff' in a:
        print('incomplete' if os.environ.get('BAD_DIFF') else os.environ.get('REMOVED','2')+' removed')
        sys.exit(2)
    if 'sync' in a or 'scrub' in a:
        if os.environ.get('WAIT_PARITY'):
            (root/'parity-running').touch()
            while True: time.sleep(1)
        fail('FAIL_PARITY')
    if 'status' in a: fail('FAIL_STATUS'); print('content readable')
elif name=='restic':
    for action,key in [('backup','FAIL_BACKUP'),('forget','FAIL_RETENTION'),('check','FAIL_CHECK'),('restore','FAIL_RESTORE')]:
        if action in a: fail(key)
    if 'backup' in a and os.environ.get('WAIT_BACKUP'):
        (root/'backup-running').touch()
        while True: time.sleep(1)
    if os.environ.get('REAL_RESTIC'):
        sys.exit(subprocess.call(['/usr/bin/restic',*a]))
    if 'copy' in a:
        n=int((root/'attempts').read_text())+1 if (root/'attempts').exists() else 1
        (root/'attempts').write_text(str(n))
        if n<=int(os.environ.get('COPY_FAILURES','0')): sys.exit(9)
        (root/'copied').touch()
    if 'snapshots' in a:
        stamp=(datetime.datetime.now(datetime.timezone.utc)-datetime.timedelta(hours=int(os.environ.get('SOURCE_AGE','0')))).isoformat()
        snapshot={'id':'a'*64,'time':os.environ.get('SOURCE_TIME',stamp),'hostname':'fixture-server','tags':['app-state']}
        if '-r' not in a and os.environ.get('INCOMPLETE_COPY'): print('[]')
        else: print(json.dumps([snapshot]))
    if 'init' in a:
        repo=Path(os.environ['RESTIC_REPOSITORY']); repo.mkdir(parents=True,exist_ok=True); (repo/'config').touch()
    if 'restore' in a:
        shutil.copytree(root/'stage',Path(a[a.index('--target')+1])/'saved-stage',dirs_exist_ok=True)
'''

class Fixtures(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory(prefix='synctjik-protection-'); self.addCleanup(self.temp.cleanup)
        self.root=Path(self.temp.name); (self.root/'bin').mkdir(); (self.root/'disk').mkdir()
        for name in ['rclone','restic','findmnt','mountpoint','fusermount3','docker','snapraid','ssh','systemctl']:
            p=self.root/'bin'/name; p.write_text(FAKE); p.chmod(0o700)
        for name in ['password','source-password','rclone.conf']:
            p=self.root/name; p.write_text('synthetic-credential'); p.chmod(0o600)
        self.env={**os.environ,'HOME':str(self.root),'FIXTURE':str(self.root),'XDG_CACHE_HOME':str(self.root/'xdg-cache'),'RESTIC_CACHE_DIR':str(self.root/'cache'),'PATH':str(self.root/'bin')+':/usr/bin:/bin'}
        self.restic=dict(repository=str(self.root/'repo'),password=str(self.root/'password'),cache=str(self.root/'cache'),host='fixture-server',tag='app-state',keep_daily=7,keep_weekly=4)
        self.mounts={str(self.root/'disk'):'fixture-uuid'}
        self.snapconf=self.root/'snapraid.conf'; self.snapconf.write_text('data d1 '+str(self.root/'disk')+'\n')
        self.source=self.root/'source'; self.source.mkdir()
        with contextlib.closing(sqlite3.connect(self.source/'app.db')) as c, c:
            c.execute('create table fixture (value text)'); c.execute("insert into fixture values ('retained data')")
        (self.source/'settings').write_text('synthetic configuration')
        self.library=self.root/'library'; self.library.mkdir(); (self.library/'book').write_bytes(b'book fixture\0contents')
        self.app=dict(host=socket.gethostname().split('.')[0],required_uid=os.geteuid(),state=str(self.root/'app-state'),lock=str(self.root/'backup.lock'),stage=str(self.root/'stage'),restic=self.restic,snapraid_config=str(self.snapconf),sources=[[str(self.source),[]]],direct_sources=[str(self.library)],plex_root=str(self.root/'plex'),plex_sqlite='/usr/bin/false',repairs=[],mariadb=dict(container='mariadb',config_source=str(self.root/'maria-config'),config_target='/config',receipt=str(self.root/'import/receipt.json')),restore_defaults=[str(self.source)],restore_extra=[],quiet_services=['plex'],restore_mounts={'mounts':self.mounts},mounts=self.mounts)
        self.drive=dict(host=self.app['host'],state=str(self.root/'drive-state'),mount_root=str(self.root/'mounts'),rclone_config=str(self.root/'rclone.conf'),export_formats='docx,xlsx,pptx,svg',sources=[dict(tag='documents',source='fixture:documents',name='documents')],restic={**self.restic,'tag':'google-drive','keep_daily':1,'keep_weekly':1,'keep_monthly':1},mounts=self.mounts)
        self.snap=dict(state=str(self.root/'snap-state'),lock=str(self.root/'snap.lock'),backup_lock=str(self.root/'backup.lock'),log_dir=str(self.root/'logs'),snapraid_config=str(self.snapconf),layout=[['data','d1',str(self.root/'disk')]],writers=['writer'],max_removed=200,scrub_percent=25,scrub_age_days=7,mounts=self.mounts)
        self.pull=dict(state=str(self.root/'pull-state'),restic=self.restic,source_repository='sftp:fixture:/repository',source_password=str(self.root/'source-password'),ssh=['ssh'],endpoints=['fixture'],remote_host='fixture-server',sftp_server=['sudo','-n','sftp-server'],limit_download=2048,copy_attempts=3,retry_seconds=0,copy_max_hours=36,health_max_hours=24,weekly_offset_seconds=28800,weekly_weekday=0,weekly_hour=6)
        self.policies={}
        for name,p in [('app-backup',self.app),('app-restore',self.app),('drive',self.drive),('snapraid',self.snap),('app-pull',self.pull)]: self.save(name,p)

    def save(self,name,p):
        path=self.root/(name+'.json'); path.write_text(json.dumps(p)); self.policies[name]=path

    def run_native(self,name,action,*args,ok=True,env=None):
        result=subprocess.run([str(BINARY),name,action,'--policy',str(self.policies[name]),*map(str,args)],env={**self.env,**(env or {})},capture_output=True,text=True,timeout=45)
        self.assertEqual(result.returncode==0,ok,result.stderr)
        return result

    def commands(self,name=None):
        p=self.root/'calls'; calls=list(map(json.loads,p.read_text().splitlines())) if p.exists() else []
        return [c for c in calls if name is None or c[0]==name]

class Protection(Fixtures):
    def test_help_and_check_do_not_execute_dependencies(self):
        for engine in self.policies:
            if engine!='app-restore': self.run_native(engine,'check')
            out=subprocess.run([str(BINARY),engine,'--help'],env=self.env,capture_output=True)
            self.assertEqual(out.returncode,0,out.stderr)
        self.assertEqual(self.commands(),[])

    def test_drive_success_retention_receipt_and_cleanup(self):
        self.run_native('drive','backup')
        receipt=json.loads((self.root/'drive-state/last-success.json').read_text())
        self.assertEqual(receipt['sources'][0]['google_native_export_formats'],'docx,xlsx,pptx,svg')
        actions=self.commands('restic'); self.assertLess(next(i for i,c in enumerate(actions) if 'check' in c),next(i for i,c in enumerate(actions) if 'forget' in c))
        self.assertFalse((self.root/'mounts/documents').exists())

    def test_drive_failure_never_publishes_success_or_retains_failed_backup(self):
        for flag in ['FAIL_BACKUP','FAIL_CHECK','FAIL_RETENTION','EMPTY_SOURCE','FAIL_MOUNT']:
            with self.subTest(flag=flag):
                self.run_native('drive','backup',ok=False,env={flag:'1'})
                self.assertFalse((self.root/'drive-state/last-success.json').exists())
                self.assertFalse((self.root/'mounts/documents').exists())
        self.assertTrue((self.root/'drive-state/last-error.json').exists())

    def test_drive_and_status_busy_fail_without_commands(self):
        (self.root/'drive-state').mkdir()
        for engine,action,lock in [('drive','backup',self.root/'drive-state/operation.lock'),('snapraid','status',self.root/'snap.lock')]:
            with lock.open('w') as f:
                fcntl.flock(f,fcntl.LOCK_EX)
                result=self.run_native(engine,action,ok=False)
                self.assertEqual(result.returncode,75)
        self.assertEqual(self.commands(),[])

    def test_application_backup_preserves_database_and_manifest(self):
        self.run_native('app-backup','backup')
        stage=self.root/'stage'; record=json.loads((stage/'backup-manifest.json').read_text())
        self.assertEqual(record['sqlite'][0]['quickCheck'],'ok')
        db=stage/str(self.source/'app.db').lstrip('/')
        with contextlib.closing(sqlite3.connect(db)) as c: self.assertEqual(c.execute('select value from fixture').fetchone()[0],'retained data')
        self.assertEqual(record['mariadbDump'],'database-dumps/mariadb.sql')
        self.assertTrue((self.root/'app-state/last-success.json').exists())
        self.assertIn('--force',next(c for c in self.commands('restic') if 'backup' in c))

    def test_sqlite_wal_is_snapshotted_without_mutating_live_database(self):
        db=self.source/'wal.db'
        with contextlib.closing(sqlite3.connect(db)) as c:
            c.execute('PRAGMA journal_mode=WAL');c.execute('PRAGMA wal_autocheckpoint=0')
            c.execute('CREATE TABLE live(value TEXT)');c.execute("INSERT INTO live VALUES ('committed WAL data')");c.commit()
            self.assertTrue(Path(str(db)+'-wal').is_file())
            before=hashlib.sha256(db.read_bytes()).hexdigest()
            self.run_native('app-backup','backup')
            self.assertEqual(hashlib.sha256(db.read_bytes()).hexdigest(),before)
            restored=self.root/'stage'/str(db).lstrip('/')
            with contextlib.closing(sqlite3.connect(restored)) as snapshot:
                self.assertEqual(snapshot.execute('SELECT value FROM live').fetchone()[0],'committed WAL data')
            self.assertFalse(Path(str(restored)+'-wal').exists())

    def test_application_failures_preserve_last_success(self):
        self.run_native('app-backup','backup'); receipt=self.root/'app-state/last-success.json'; old=receipt.read_bytes()
        for flag in ['SHORT_DUMP','FAIL_BACKUP','FAIL_RETENTION','FAIL_CHECK','FAIL_MOUNT']:
            self.run_native('app-backup','backup',ok=False,env={flag:'1'})
            self.assertEqual(receipt.read_bytes(),old)

    def test_restore_requires_exact_id_and_complete_snapshot(self):
        self.run_native('app-restore','stage','--stage',self.root/'restore','--snapshot','latest',ok=False)
        self.assertFalse((self.root/'restore').exists())
        self.run_native('app-backup','backup')
        self.run_native('app-restore','stage','--stage',self.root/'restore','--snapshot','a'*64,'--repository',self.root/'repo','--password-file',self.root/'password')
        dump=self.root/'restore/saved-stage/database-dumps/mariadb.sql'; dump.write_text('bad')
        self.run_native('app-restore','verify','--stage',self.root/'restore',ok=False)

    def test_restore_staging_works_on_a_replacement_host(self):
        self.run_native('app-backup','backup')
        self.save('app-restore',{**self.app,'host':'original-host-no-longer-available'})
        self.run_native('app-restore','stage','--stage',self.root/'replacement-restore','--snapshot','a'*64,'--repository',self.root/'repo','--password-file',self.root/'password')

    def test_restore_refuses_populated_destinations_and_writers(self):
        self.run_native('app-backup','backup')
        self.run_native('app-restore','install','--stage',self.root/'stage',ok=False)
        before=len(self.commands('rsync'))
        self.run_native('app-restore','install','--stage',self.root/'stage',ok=False,env={'ACTIVE_WRITERS':'1'})
        self.assertEqual(len(self.commands('rsync')),before)
        self.assertEqual((self.source/'settings').read_text(),'synthetic configuration')

    def test_partial_mariadb_import_cannot_retry(self):
        self.run_native('app-backup','backup')
        self.run_native('app-restore','import-mariadb','--stage',self.root/'stage',ok=False,env={'FAIL_IMPORT':'1'})
        receipt=json.loads((self.root/'import/receipt.json').read_text()); self.assertEqual(receipt['status'],'started')
        before=len([c for c in self.commands('docker') if '-i' in c])
        self.run_native('app-restore','import-mariadb','--stage',self.root/'stage',ok=False)
        self.assertEqual(before,len([c for c in self.commands('docker') if '-i' in c]))
        dump=self.root/'stage/database-dumps/mariadb.sql'; self.assertEqual(receipt['sha256'],hashlib.sha256(dump.read_bytes()).hexdigest())

    def test_copy_retry_full_verification_and_receipt(self):
        self.run_native('app-pull','copy',env={'COPY_FAILURES':'2'})
        self.assertEqual((self.root/'attempts').read_text(),'3')
        receipt=json.loads((self.root/'pull-state/last-success.json').read_text()); self.assertEqual(receipt['dataCheck'],'passed')
        self.assertTrue(any('--read-data' in c for c in self.commands('restic')))

    def test_copy_failures_never_publish(self):
        for env in [{'COPY_FAILURES':'99'},{'SOURCE_AGE':'37'},{'SOURCE_AGE':'-1'},{'INCOMPLETE_COPY':'1'},{'FAIL_RETENTION':'1'},{'FAIL_CHECK':'1'}]:
            self.run_native('app-pull','copy',ok=False,env=env)
            self.assertFalse((self.root/'pull-state/last-success.json').exists())

    def test_copy_health_uses_matching_weekly_evidence(self):
        stamp=dt.datetime.now(dt.timezone.utc).isoformat(); env={'SOURCE_TIME':stamp}
        self.run_native('app-pull','copy',env=env)
        copies=len([c for c in self.commands('restic') if 'copy' in c])
        self.run_native('app-pull','health',env=env)
        self.assertEqual(copies,len([c for c in self.commands('restic') if 'copy' in c]))
        self.run_native('app-pull','health',ok=False,env={'SOURCE_AGE':'25'})

    def test_snapraid_success_gate_and_owned_resume(self):
        self.run_native('snapraid','sync')
        self.assertFalse((self.root/'paused').exists())
        self.assertFalse((self.root/'snap-state/paused.json').exists())
        receipt=json.loads((self.root/'snap-state/last-sync-success.json').read_text()); self.assertEqual(receipt['mode'],'sync')
        self.assertFalse(any('--force' in c or '-f' in c for c in self.commands('snapraid')))

    def test_snapraid_threshold_truncation_mount_and_scrub_failure(self):
        for env in [{'REMOVED':'201'},{'BAD_DIFF':'1'},{'FAIL_MOUNT':'1'}]: self.run_native('snapraid','sync',ok=False,env=env)
        self.assertFalse(any('sync' in c for c in self.commands('snapraid')))
        self.run_native('snapraid','scrub',ok=False,env={'FAIL_PARITY':'1'})
        self.run_native('snapraid','sync',ok=False)
        self.run_native('snapraid','scrub')
        self.run_native('snapraid','sync')

    def test_resume_failure_retains_ownership_and_blocks_success(self):
        self.run_native('snapraid','sync',ok=False,env={'FAIL_RESUME':'1'})
        self.assertTrue((self.root/'snap-state/paused.json').exists())
        self.assertFalse((self.root/'snap-state/last-sync-success.json').exists())
        self.run_native('snapraid','resume')
        self.assertFalse((self.root/'paused').exists())

    def test_interrupted_parity_resumes_writers_and_does_not_publish(self):
        proc=subprocess.Popen([str(BINARY),'snapraid','sync','--policy',str(self.policies['snapraid'])],env={**self.env,'WAIT_PARITY':'1'},stdout=subprocess.PIPE,stderr=subprocess.PIPE)
        self.addCleanup(lambda: proc.poll() is None and proc.kill())
        deadline=time.monotonic()+10
        while not (self.root/'parity-running').exists() and time.monotonic()<deadline: time.sleep(.05)
        self.assertTrue((self.root/'parity-running').exists()); proc.terminate(); proc.communicate(timeout=15)
        self.assertNotEqual(proc.returncode,0)
        self.assertFalse((self.root/'paused').exists())
        self.assertFalse((self.root/'snap-state/last-sync-success.json').exists())

    def test_interrupted_drive_unmounts_and_does_not_publish(self):
        proc=subprocess.Popen([str(BINARY),'drive','backup','--policy',str(self.policies['drive'])],env={**self.env,'WAIT_BACKUP':'1'},stdout=subprocess.PIPE,stderr=subprocess.PIPE)
        self.addCleanup(lambda: proc.poll() is None and proc.kill())
        deadline=time.monotonic()+10
        while not (self.root/'backup-running').exists() and time.monotonic()<deadline: time.sleep(.05)
        self.assertTrue((self.root/'backup-running').exists()); proc.terminate(); proc.communicate(timeout=15)
        self.assertFalse((self.root/'mounts/documents').exists())
        self.assertFalse((self.root/'drive-state/last-success.json').exists())

    @unittest.skipUnless(shutil.which('restic') and shutil.which('rsync'),'real Restic and rsync required')
    def test_real_encrypted_repository_restore_and_hashes(self):
        self.run_native('app-backup','backup',env={'REAL_RESTIC':'1'})
        env={**self.env,'RESTIC_REPOSITORY':self.restic['repository'],'RESTIC_PASSWORD_FILE':self.restic['password']}
        snapshots=json.loads(subprocess.check_output(['/usr/bin/restic','snapshots','--json'],env=env)); snapshot=snapshots[-1]['id']
        target=self.root/'verified-restore'
        self.run_native('app-restore','stage','--stage',target,'--snapshot',snapshot,'--repository',self.root/'repo','--password-file',self.root/'password',env={'REAL_RESTIC':'1'})
        for source in [self.library/'book',self.source/'settings']:
            restored=target/str(source).lstrip('/') if source.parent==self.library else target/str(self.root/'stage').lstrip('/')/str(source).lstrip('/')
            self.assertEqual(hashlib.sha256(source.read_bytes()).digest(),hashlib.sha256(restored.read_bytes()).digest())
        subprocess.run(['/usr/bin/restic','check','--read-data'],env=env,check=True,capture_output=True)
        shutil.rmtree(self.source)
        self.run_native('app-restore','install','--stage',target)
        with contextlib.closing(sqlite3.connect(self.source/'app.db')) as c, c: self.assertEqual(c.execute('select value from fixture').fetchone()[0],'retained data')

    def test_cutover_refuses_busy_job_and_holds_legacy_lock_during_apply(self):
        policy=self.root/'migration.json'; policy.write_text(json.dumps({'locks':[str(self.root/'backup.lock')],'units':[{'name':'fixture.service'}],'reload_managers':['user']}))
        (self.root/'unit-status.json').write_text(json.dumps({'LoadState':'loaded','ActiveState':'active'}))
        args=[str(BINARY),'protection-migration','--policy',str(policy),'--','touch',str(self.root/'applied')]
        result=subprocess.run(args,env=self.env,capture_output=True);self.assertEqual(result.returncode,75)
        self.assertFalse((self.root/'applied').exists())
        (self.root/'unit-status.json').write_text(json.dumps({'LoadState':'loaded','ActiveState':'inactive'}))
        script=self.root/'apply.py'; script.write_text("import fcntl,sys\nwith open(sys.argv[1],'a') as f:\n try: fcntl.flock(f,fcntl.LOCK_EX|fcntl.LOCK_NB)\n except BlockingIOError: sys.exit(0)\n sys.exit(9)\n")
        args=args[:args.index('--')+1]+['/usr/bin/python3',str(script),str(self.root/'backup.lock')]
        result=subprocess.run(args,env=self.env,capture_output=True);self.assertEqual(result.returncode,0,result.stderr)
        self.assertFalse(any('enable' in c or 'restart' in c or 'stop' in c for c in self.commands('systemctl')))

    def test_receipt_publication_failure_is_not_success(self):
        state=self.root/'app-state';state.mkdir();(state/'last-success.json').mkdir()
        self.run_native('app-backup','backup',ok=False)
        self.assertTrue((state/'last-error.json').is_file())
        self.assertTrue((state/'last-success.json').is_dir())

    @unittest.skipUnless(shutil.which('restic'),'real Restic required')
    def test_real_drive_restore_and_hash(self):
        env={'REAL_RESTIC':'1'}
        self.run_native('drive','backup',env=env)
        self.save('drive',{**self.drive,'host':'original-host-no-longer-available'})
        result=self.run_native('drive','snapshots','--tag','documents',env=env)
        snapshot=json.loads(result.stdout)[-1]['id']; target=self.root/'drive-restore'
        self.run_native('drive','restore','--tag','documents','--snapshot',snapshot,'--to',target,env=env)
        files=list(target.rglob('document.docx'));self.assertEqual(len(files),1)
        self.assertEqual(hashlib.sha256(files[0].read_bytes()).digest(),hashlib.sha256(b'fixture document export').digest())

    @unittest.skipUnless(shutil.which('restic'),'real Restic required')
    def test_independent_copy_restores_after_source_repository_is_unavailable(self):
        env={'REAL_RESTIC':'1'}
        self.run_native('app-backup','backup',env=env)
        password=self.root/'independent-password';password.write_text('different synthetic password');password.chmod(0o600)
        self.pull['source_repository']=str(self.root/'repo');self.pull['source_password']=str(self.root/'password')
        self.pull['restic']={**self.restic,'repository':str(self.root/'independent'),'password':str(password)}
        self.save('app-pull',self.pull);self.run_native('app-pull','copy',env=env)
        (self.root/'repo').rename(self.root/'source-unavailable')
        actual={**self.env,'RESTIC_REPOSITORY':str(self.root/'independent'),'RESTIC_PASSWORD_FILE':str(password)}
        snapshots=json.loads(subprocess.check_output(['/usr/bin/restic','snapshots','--json'],env=actual));snapshot=snapshots[-1]['id']
        target=self.root/'independent-restore'
        self.run_native('app-restore','stage','--stage',target,'--snapshot',snapshot,'--repository',self.root/'independent','--password-file',password,env=env)
        book=target/str(self.library/'book').lstrip('/')
        self.assertEqual(hashlib.sha256(book.read_bytes()).digest(),hashlib.sha256((self.library/'book').read_bytes()).digest())

class Notifications(Fixtures):
    # Reuse only fixture construction, not the storage test methods.
    def setUp(self):
        super().setUp()
        self.messages=[]; self.reject=False
        owner=self
        class Handler(http.server.BaseHTTPRequestHandler):
            def do_POST(self):
                value=json.loads(self.rfile.read(int(self.headers['Content-Length'])))
                owner.messages.append(value)
                self.send_response(503 if owner.reject else 200); self.end_headers()
                self.wfile.write(json.dumps({'ok':True,'result':{'message_id':len(owner.messages)}}).encode())
            def log_message(self,*args): pass
        self.server=http.server.ThreadingHTTPServer(('127.0.0.1',0),Handler)
        self.thread=threading.Thread(target=self.server.serve_forever,daemon=True);self.thread.start()
        self.addCleanup(self.server.server_close); self.addCleanup(self.server.shutdown)
        credentials=self.root/'telegram.json'; credentials.write_text(json.dumps({'bot_token':'SYNTHETIC-TOKEN','chat_id':123}));credentials.chmod(0o600)
        self.notify=dict(host='fixture',state=str(self.root/'notify-state'),receipts=str(self.root/'snap-state'),credentials=str(credentials),reminder_seconds=86400,telegram_base_url='http://127.0.0.1:'+str(self.server.server_port),jobs={'fixture-sync.service':dict(task='Sync',mode='sync',max_age=129600),'fixture-resume.service':dict(task='Writer recovery',mode=None,max_age=None,remain_after_exit=True)})
        self.save('snapraid-notify',self.notify)

    def test_notification_pending_retry_reminder_and_recovery(self):
        (self.root/'unit-status.json').write_text(json.dumps({'ActiveState':'failed','Result':'exit-code','ExecMainStatus':'1'}))
        self.reject=True
        result=self.run_native('snapraid-notify','failure','fixture-sync.service',ok=False)
        self.assertNotIn('SYNTHETIC-TOKEN',result.stderr)
        receipt=self.root/'notify-state/fixture-sync.service.json'; self.assertIsNone(json.loads(receipt.read_text())['last_sent_at'])
        self.reject=False
        self.run_native('snapraid-notify','failure','fixture-sync.service')
        self.run_native('snapraid-notify','failure','fixture-sync.service'); self.assertEqual(len(self.messages),2)
        state=json.loads(receipt.read_text());state['last_sent_at']=(dt.datetime.now(dt.timezone.utc)-dt.timedelta(days=2)).isoformat();receipt.write_text(json.dumps(state))
        self.run_native('snapraid-notify','failure','fixture-sync.service'); self.assertEqual(len(self.messages),3)
        (self.root/'unit-status.json').write_text(json.dumps({'ActiveState':'inactive','Result':'success'}))
        self.run_native('snapraid-notify','failure','fixture-sync.service')
        self.run_native('snapraid-notify','recovery','fixture-sync.service'); self.assertEqual(len(self.messages),4)
        self.assertFalse(json.loads(receipt.read_text())['active']); self.assertIn('Recovered',self.messages[-1]['text'])
        self.assertEqual(self.commands('snapraid'),[])

    def test_health_corrupt_receipt_running_job_and_quiet_success(self):
        state=self.root/'snap-state';state.mkdir();(state/'last-sync-success.json').write_text(json.dumps({'completed_at':dt.datetime.now(dt.timezone.utc).isoformat()}))
        self.run_native('snapraid-notify','health'); self.assertEqual(self.messages,[])
        (state/'last-sync-success.json').write_text('broken')
        (self.root/'unit-status.json').write_text(json.dumps({'ActiveState':'activating','Result':'success','ExecMainStartTimestampMonotonic':str(int(time.monotonic()*1e6))}))
        self.run_native('snapraid-notify','health'); self.assertEqual(self.messages,[])
        (self.root/'unit-status.json').write_text(json.dumps({'ActiveState':'inactive','Result':'success'}))
        self.run_native('snapraid-notify','health'); self.assertEqual(len(self.messages),1)
        self.assertIn('No verified successful sync',self.messages[-1]['text'])

    def test_health_uses_current_failure_and_escapes_html(self):
        state=self.root/'snap-state';state.mkdir();(state/'sync-failure.json').write_text(json.dumps({'failed_at':dt.datetime.now(dt.timezone.utc).isoformat(),'error':'<unsafe & text>'}))
        (self.root/'unit-status.json').write_text(json.dumps({'ActiveState':'failed','Result':'exit-code'}))
        self.run_native('snapraid-notify','health')
        self.assertIn('&lt;unsafe &amp; text&gt;',self.messages[-1]['text'])

class Oracles(unittest.TestCase):
    def test_weekly_contract_boundary_and_historical_role(self):
        # Independent frozen Python algorithms remain the compatibility oracle.
        tree=ast.parse((APP/'tests/legacy/protection/pull.py').read_text())
        fn=next(n for n in tree.body if isinstance(n,ast.FunctionDef) and n.name=='weekly_copy_current')
        ns={'datetime':dt,'ZoneInfo':__import__('zoneinfo').ZoneInfo}
        exec(compile(ast.Module(body=[fn],type_ignores=[]),'<oracle>','exec'),ns)
        now=dt.datetime(2026,10,25,0,tzinfo=dt.timezone.utc)
        self.assertTrue(ns['weekly_copy_current']({'copiedAt':'2026-10-24T22:00:00+00:00','sourceTime':'x','dataCheck':'passed'},now))
        self.assertFalse(ns['weekly_copy_current']({'copiedAt':'2026-10-24T21:59:59+00:00','sourceTime':'x','dataCheck':'passed'},now))

if __name__=='__main__': unittest.main()
