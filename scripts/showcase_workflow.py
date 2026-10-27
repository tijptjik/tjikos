"""Private, reproducible release-check workflow for the Tjikup film."""
from concurrent.futures import ThreadPoolExecutor, as_completed
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import time
from tjikup.progress import Progress
from tjikup import runtime

root=Path(os.environ['SHOWCASE_ROOT'])
work=Path(os.environ['SHOWCASE_WORK'])
report=work/'workflow-result.json'

if '--package' in sys.argv:
    result=json.loads(report.read_text())
    package=work/'release-checks.tar.gz'
    files=[work/(row['id']+'.log') for row in result['checks']]+[report]
    with tarfile.open(package,'w:gz') as archive:
        for file in files:archive.add(file,arcname=file.name)
    with tarfile.open(package) as archive:
        for file in files:
            if archive.extractfile(file.name).read()!=file.read_bytes():raise ValueError('Package verification failed')
    runtime.stage_result('PACK','Release check reports',f'{len(files)} files')
    runtime.stage_result('VERIFY','Package contents','all hashes match')
    result['package']={'bytes':package.stat().st_size,'sha256':hashlib.sha256(package.read_bytes()).hexdigest(),'verifiedFiles':len(files)}
    report.write_text(json.dumps(result,indent=2));raise SystemExit(0)

checks=[
 ('runtjik','Runtjik · launcher checks',['python3','-B','-m','unittest','discover','-s','tests','-v'],root/'apps/runtjik',{}),
 ('tjikup','Tjikup · workflow checks',['python3','-B','-m','unittest','discover','-s','tests','-v'],root/'apps/tjikup',{'PYTHONPATH':str(root/'apps/tjikup/src')}),
 ('mailtjik','Mailtjik · archive checks',['go','test','./...'],root/'apps/mailtjik',{}),
 ('greetjik','Greetjik · render checks',['cargo','test','--manifest-path',str(root/'apps/greetjik/Cargo.toml'),'--target-dir',str(work/'cargo-target')],root,{})]

def run(check):
    ident,label,command,cwd,extra=check
    start=time.monotonic()
    child_env={key:value for key,value in os.environ.items() if key not in {'TJIKUP_REPORT_FILE','TJIKUP_PROGRESS','TJIKUP_COLOR','TJIKUP_NO_SPIN'}}
    process=subprocess.run(command,cwd=cwd,env={**child_env,**extra},stdout=subprocess.PIPE,stderr=subprocess.STDOUT,text=True)
    (work/(ident+'.log')).write_text(process.stdout)
    return {'id':ident,'label':label,'command':command,'returncode':process.returncode,'seconds':round(time.monotonic()-start,2),'logSha256':hashlib.sha256(process.stdout.encode()).hexdigest()}

results=[]
with Progress() as progress:
    for ident,label,*_ in checks:progress.start(ident,label,verb='CHECK')
    with ThreadPoolExecutor(max_workers=4) as pool:
        futures={pool.submit(run,check):check for check in checks}
        for future in as_completed(futures):
            row=future.result();results.append(row)
            progress.finish(row['id'],row['label'],verb='CHECK',icon='✓' if row['returncode']==0 else '✗',note=f"{row['seconds']:.1f}s")
            if row['returncode']:
                with progress.output():print((work/(row['id']+'.log')).read_text())
success=all(row['returncode']==0 for row in results)
report.write_text(json.dumps({'success':success,'checks':results},indent=2))
raise SystemExit(0 if success else 1)
