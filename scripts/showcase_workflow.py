"""Real release checks with the fleet's native rich report renderer."""
from concurrent.futures import ThreadPoolExecutor, as_completed
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tarfile
import time
from showcase_report.icons import HookProgress as Progress
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
    report.write_text(json.dumps(result,indent=2))
    print('\033[2J\033[H\033[1;38;2;255;83;166mRich reporting\033[0m  ·  TjikOS release checks\n',end='',flush=True)
    with Progress() as rich:
        if not rich.graphics:raise ValueError('Native Kitty icon renderer is required for this capture')
        for name in ['notjik','bartjik','runtjik','tjikup','mailtjik','greetjik']:
            row=next(row for row in result['checks'] if row['id']==name)
            rich.finish(name,row['label'],verb='CHECK',icon='✓',note=f"{row['summary']} · {row['seconds']:.1f}s",record=False)
    total=sum(row['testsPassed'] for row in result['checks'])
    print(f'\n{total} checks passed · 6 apps · package verified',flush=True)
    raise SystemExit(0)

checks=[
 ('notjik','Notjik (notification handlers)',['python3','-B','-m','unittest','discover','-s','tests','-p','test_handlers.py','-v'],root/'apps/notjik',{}),
 ('bartjik','Bartjik (panel core)',['python3','-B','-m','unittest','discover','-s','tests','-p','test_core.py','-v'],root/'apps/bartjik',{'PYTHONPATH':str(root/'apps/bartjik')}),
 ('runtjik','Runtjik (launcher checks)',['python3','-B','-m','unittest','discover','-s','tests','-v'],root/'apps/runtjik',{}),
 ('tjikup','Tjikup (workflow checks)',['python3','-B','-m','unittest','discover','-s','tests','-v'],root/'apps/tjikup',{'PYTHONPATH':str(root/'apps/tjikup/src')}),
 ('mailtjik','Mailtjik (archive checks)',['go','test','-v','./...'],root/'apps/mailtjik',{}),
 ('greetjik','Greetjik (render checks)',['cargo','test','--manifest-path',str(root/'apps/greetjik/Cargo.toml'),'--target-dir',str(work/'cargo-target')],root,{})]

def run(check):
    ident,label,command,cwd,extra=check
    progress.start(ident,label,verb='CHECK')
    start=time.monotonic()
    child_env={key:value for key,value in os.environ.items() if key not in {'TJIKUP_REPORT_FILE','TJIKUP_PROGRESS','TJIKUP_COLOR','TJIKUP_NO_SPIN'}}
    process=subprocess.run(command,cwd=cwd,env={**child_env,**extra},stdout=subprocess.PIPE,stderr=subprocess.STDOUT,text=True)
    text=process.stdout
    (work/(ident+'.log')).write_text(text)
    python=re.findall(r'Ran (\d+) tests?',text)
    skipped=re.findall(r'OK \(skipped=(\d+)\)',text)
    skip=int(skipped[-1]) if skipped else 0
    passed=(int(python[-1])-skip if python else
            sum(map(int,re.findall(r'test result: ok\. (\d+) passed;',text))) if 'cargo'==command[0] else
            len(re.findall(r'^--- PASS:',text,re.MULTILINE)))
    if process.returncode==0 and passed<1:raise ValueError('No verified test counts for '+ident)
    summary=f'{passed} passed'+(f', {skip} skipped' if skip else '')
    return {'id':ident,'label':label,'command':command,'returncode':process.returncode,'seconds':round(time.monotonic()-start,2),'testsPassed':passed,'testsSkipped':skip,'summary':summary,'logSha256':hashlib.sha256(text.encode()).hexdigest()}

results=[]
with Progress() as progress:
    if not progress.graphics:raise ValueError('Native Kitty icon renderer is required for this capture')
    with ThreadPoolExecutor(max_workers=4) as pool:
        futures={pool.submit(run,check):check for check in checks}
        for future in as_completed(futures):
            row=future.result();results.append(row)
            progress.finish(row['id'],row['label'],verb='CHECK',icon='✓' if row['returncode']==0 else '✗',note=f"{row['summary']} · {row['seconds']:.1f}s")
            if row['returncode']:
                with progress.output():print((work/(row['id']+'.log')).read_text())
success=all(row['returncode']==0 for row in results)
report.write_text(json.dumps({'success':success,'checks':results},indent=2))
raise SystemExit(0 if success else 1)
