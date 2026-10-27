#!/usr/bin/env python3
"""Record the four remaining tools through their native renderers and input paths."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import os_showcase as shared

ROOT=shared.ROOT
BUILD=Path('/tmp/tjikos-showcase-build')
STORIES={
 'runtjik':('Runtjik','Find it. Run it.','Find an app, choose a window, keep your hands on the keyboard.',[
  ('Your next app, a few letters away.','Fuzzy search narrows the grid as you type.'),
  ('Choose the right window.','See open instances before deciding where to go.'),
  ('Keep your flow on the keyboard.','Switch layouts and discover shortcuts without leaving the launcher.')]),
 'mailtjik':('Mailtjik','Your mail, worth keeping.','Find a conversation. Read the original. Choose what to preserve.',[
  ('Find that conversation again.','Combine a name, subject and years to narrow the archive.'),
  ('Read the message, with its context.','Sender, recipients, rich text and inline images stay together.'),
  ('Choose what you keep.','Select accounts and years; preview the scope before capture.')]),
 'tjikup':('Tjikup','One command. A clear outcome.','Run the workflow, follow the work, finish with a clear result.',[
  ('One command for the whole workflow.','Ordered stages keep preparation, checks and packaging together.'),
  ('Watch tasks progress together.','Independent checks run in parallel, with a result for every task.  ·  3×'),
  ('Finish with a clear result.','The real run succeeded: checks passed and the package was verified.')]),
 'greetjik':('Greetjik','Start every session with a quip','A tiny dose of pony-powered fun for your terminal.',[
  ('A little fun at the prompt.','Pipe a quip into Greetjik and let a pony deliver it.'),
  ('Pick your pony.','Choose a character to match today’s mood.'),
  ('Make the words your own.','Your message, your shell, a little more sparkle.')])}


def qs(path,target,method,*args):
    return shared.checked(['quickshell','ipc','--path',str(path),'call',target,method,*map(str,args)])


class Director(shared.Director):
    def __init__(self,directory,env,pointer,monitor,review,tool):
        self.directory,self.env,self.review,self.tool=directory,env,review,tool
        self.pointer=subprocess.Popen([str(pointer),monitor],env=env,stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True,bufsize=1)
        self.position=(1150,680);self.events=[];self.chapters=[];self.processes=[];self.speed=[]
        self.stage=directory/'stage';self.started=time.monotonic()
        self.move(*self.position,duration=.1)

    def scene(self,number):
        heading,caption=STORIES[self.tool][3][number-1]
        self.stage_scene(mode='feature',heading=heading,caption=caption,chapter=number,coverWindows=False)
        self.chapters.append({'chapter':number,'heading':heading,'rawStart':round(time.monotonic()-self.started,3)})
        print(f'{self.tool} {number}/3 {heading}',flush=True);time.sleep(.65)

    def stage_scene(self,**data):
        return json.loads(qs(self.stage,'showcase','request',json.dumps(data)))

    def evidence(self,label,**data):
        self.events.append({'time':round(time.monotonic()-self.started,3),'verified':label,**data})

    def type(self,text):
        shared.checked(['wtype','-d','42',text],env=self.env)
        self.events.append({'time':round(time.monotonic()-self.started,3),'input':'native keyboard','text':text})

    def key(self,key):shared.checked(['wtype','-k',key],env=self.env)
    def chord(self,modifier,key):shared.checked(['wtype','-M',modifier,'-k',key,'-m',modifier],env=self.env)

    def terminal(self,font=11.5):
        shared.checked(['hyprctl','eval','hl.window_rule({name="showcase-terminal",float=true,no_anim=true,no_shadow=true,no_blur=true,border_size=0,match={class="^ShowcaseTerminal$"}})'])
        self.socket=self.directory/'k.sock'
        self.terminal_process=subprocess.Popen(['kitty','--config','NONE','--listen-on','unix:'+str(self.socket),
            '--title','showcase-terminal','--class','ShowcaseTerminal','-o','allow_remote_control=yes',
            '-o','font_family=Monaspace Argon','-o','font_size='+str(font),'-o','background=#191724',
            '-o','foreground=#e5e5e5','-o','cursor=#ff53a6','-o','cursor_blink_interval=0',
            '-o','window_padding_width=10','-o','hide_window_decorations=yes','-o','confirm_os_window_close=0',
            'bash','--noprofile','--norc','-i'],env={**{k:v for k,v in self.env.items() if k!='NO_COLOR'},'TERM':'xterm-kitty','COLORTERM':'truecolor','CLICOLOR_FORCE':'1','PS1':r'\[\e[38;2;255;83;166m\]❯ \[\e[0m\]',
            'HISTFILE':str(self.directory/'history'),'PROMPT_COMMAND':'','PATH':str(BUILD/'bin')+':'+self.env['PATH']},
            cwd=self.directory,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
        self.processes.append(self.terminal_process)
        client=shared.wait_for(lambda:next((c for c in json.loads(shared.checked(['hyprctl','-j','clients'])) if c['class']=='ShowcaseTerminal'),None))
        address=json.dumps('address:'+client['address'])
        width,x=(650,10360) if self.tool=='greetjik' else (1104,10048)
        shared.checked(['hyprctl','eval',f'local w=hl.get_window({address});hl.dispatch(hl.dsp.window.float({{window=w}}));hl.dispatch(hl.dsp.window.resize({{x={width},y=468,relative=false,window=w}}));hl.dispatch(hl.dsp.window.move({{x={x},y=10182,relative=false,window=w}}))'])
        shared.wait_for(lambda:self.socket.exists());time.sleep(.6)
        self.evidence('native terminal',address=client['address'],font=font)

    def text(self):
        return shared.checked(['kitty','@','--to','unix:'+str(self.socket),'get-text','--extent','screen'])

    def wait_text(self,text,timeout=20):
        return shared.wait_for(lambda:text in self.text(),timeout=timeout)

    def screen(self,label):
        self.frame(label)
        (self.review/(label+'.txt')).write_text(self.text())

    def shell(self,command,wait=None):
        self.type(command);self.key('Return')
        if wait:self.wait_text(wait)

    def clear(self):self.chord('ctrl','l');time.sleep(.2)

    def runtjik(self):
        app=self.directory/'runtjik';shutil.copytree(ROOT/'apps/runtjik/qml',app/'qml')
        shutil.copy2(ROOT/'apps/runtjik/runtjik.py',app/'runtjik.py')
        qml=app/'qml/shell.qml';text=qml.read_text().replace('(panel.height-frame.height)/2','190').replace('y: (panel.height-height)/2','y: 190').replace('if(placed && root.centering)', 'if(placed && !root.transferring)').replace('cellHeight:root.listMode?70:122','cellHeight:root.listMode?70:104')
        qml.write_text(text)
        conf=self.directory/'runtjik.conf';conf.write_text('[ui]\nwidth=1040\nheight=450\ncolumns=5\nicon_size=42\nfont_size=14\n[theme]\nicon_theme="rose-punk-illustrated"\n')
        self.scene(1)
        with (self.directory/'runtjik.log').open('w') as log:
            p=subprocess.Popen(['quickshell','--path',str(app/'qml'),'--no-color'],env={**self.env,'RUNTJIK_HOME':str(app),'RUNTJIK_DEMO':'1','RUNTJIK_CONFIG':str(conf)},stdout=log,stderr=log)
        self.processes.append(p)
        def status():return json.loads(qs(app/'qml','runtjik','status'))
        def state():return json.loads(qs(app/'qml','runtjik','testState'))
        shared.wait_for(lambda:status()['ready']);time.sleep(1.5)
        if status()['geometry']['y']<10180:raise ValueError('Launcher obscures the caption')
        self.frame('01-app-grid');time.sleep(.8)
        self.type('fire');shared.wait_for(lambda:state()['resultQuery']=='fire' and len(state()['rows'])==1)
        time.sleep(2);self.frame('02-search');self.evidence('native fuzzy search',query='fire',rows=state()['rows'])
        self.scene(2);self.chord('shift','Return');shared.wait_for(lambda:status()['windowPicker'])
        time.sleep(1.3);self.key('Down');time.sleep(1.1);self.key('Up');time.sleep(1)
        self.frame('03-window-picker');self.evidence('native instance picker',rows=state()['rows'])
        self.key('Escape');time.sleep(.4)
        self.scene(3);self.chord('ctrl','l');shared.wait_for(lambda:status()['view']=='list')
        time.sleep(1.3);self.frame('04-list')
        self.chord('ctrl','a');self.type('/bindings');shared.wait_for(lambda:state()['resultQuery']=='/bindings')
        time.sleep(2);self.frame('05-shortcuts')
        for _ in range(4):self.key('Down');time.sleep(.3)
        time.sleep(1.2);self.frame('06-shortcut-actions');self.evidence('native shortcuts',count=len(state()['rows']))
        qs(app/'qml','runtjik','quit');p.wait(timeout=8)

    def mailtjik(self):
        self.scene(1);self.shell('mailtjik read --demo',wait='PARTICIPANT');time.sleep(.7)
        self.type('/morgan');self.key('Tab');self.type('atlas');self.key('Tab');self.type('2023');self.key('Tab');self.type('2025')
        self.key('Tab');self.key('space');self.key('Tab');self.key('space');self.key('Return')
        self.wait_text('matches');time.sleep(1.3);self.screen('01-search')
        self.evidence('native combined mail search',screen=self.text())
        self.scene(2);self.key('Down');self.key('Return');self.wait_text('Autumn field notes')
        time.sleep(1.4);self.screen('02-message');self.key('Down');time.sleep(.6);self.key('Down');time.sleep(.6)
        self.evidence('native message reader',screen=self.text());self.key('e');self.wait_text('Demo: EML')
        time.sleep(.9);self.screen('03-export-control');self.key('Escape')
        self.scene(3);self.chord('ctrl','a');self.wait_text('Preserve once')
        for _ in range(4):self.key('Down');time.sleep(.18)
        self.key('Return');self.wait_text('Select');time.sleep(.6)
        self.type('n');self.key('space');self.key('Down');self.key('Down');self.key('Right');self.key('Down');self.key('Right');self.key('Down');self.key('space')
        time.sleep(1);self.screen('04-selection');self.key('Return');self.wait_text('Enter: preview selected mail');time.sleep(.6);self.key('Return');self.wait_text('Capture preview')
        time.sleep(1.5);self.screen('05-preview');self.evidence('native archive selection and preview',screen=self.text())
        self.chord('ctrl','c');time.sleep(.3)

    def greetjik(self):
        self.scene(1);self.shell("printf 'Tiny tasks. Big sparkle.\\n' | greetjik -f twilight")
        self.wait_text('Tiny tasks. Big sparkle.');time.sleep(1.5);self.screen('01-quip')
        self.evidence('native piped quote',screen=self.text())
        self.scene(2);self.clear();self.shell("greetjik -f rainbow 'A fresh prompt. A fresh possibility.'")
        self.wait_text('A fresh prompt.');time.sleep(1.6);self.screen('02-pony')
        self.evidence('native selected pony',pony='rainbow',screen=self.text())
        self.scene(3);self.clear();self.shell("greetjik -f pinkie 'Ship something small. Celebrate anyway.'")
        self.wait_text('Celebrate anyway.');time.sleep(2);self.screen('03-own-words')
        self.evidence('native personal message',screen=self.text())

    def tjikup(self):
        prepare_workflow(self.directory)
        self.scene(1);self.type('tjikup --config workflow.json');time.sleep(.7);self.key('Return')
        self.wait_text('Checking applications',timeout=30);time.sleep(1)
        self.screen('01-workflow');self.scene(2)
        start=time.monotonic()-self.started
        self.wait_text('ALL SYSTEMS GO',timeout=240)
        end=time.monotonic()-self.started
        if end-start>1:self.speed.append({'start':round(start,3),'end':round(end,3),'rate':3})
        report=json.loads((self.directory/'workflow-result.json').read_text())
        if not report['success'] or any(row['returncode'] for row in report['checks']):raise ValueError('Workflow did not succeed')
        self.evidence('real successful workflow',report=report)
        self.scene(3);time.sleep(2.6);self.screen('02-success')
        shutil.copy2(self.directory/'workflow-result.json',self.review/'workflow-result.json')
        shutil.copy2(self.directory/'workflow.json',self.review/'workflow.json')

    def run(self):
        time.sleep(2.2);self.frame('00-intro')
        if self.tool!='runtjik':self.terminal(font=7.5 if self.tool=='greetjik' else 9 if self.tool=='mailtjik' else 11.5)
        getattr(self,self.tool)()
        self.stage_scene(mode='outro',chapter=3,coverWindows=True);time.sleep(1.2);self.frame('99-outro');time.sleep(2)


def prepare_workflow(directory):
    shutil.copy2(shared.HERE/'showcase_workflow.py',directory/'checks.py')
    cfg={'version':1,'title':'TjikOS release checks','env':{'TJIKUP_NOTIFY':'0','SHOWCASE_ROOT':str(ROOT),'SHOWCASE_WORK':str(directory),'GOCACHE':str(directory/'go-cache')},'steps':[
      {'id':'checks','label':'Application checks','section':'Checking applications','verb':'CHECK','command':['python3','checks.py'],'output':'stream','summary':False,'timeout':240},
      {'id':'verify','label':'Release package verified','section':'Package and verify','verb':'VERIFY','command':['python3','checks.py','--package'],'output':'stream','summary':False,'timeout':60}]}
    (directory/'workflow.json').write_text(json.dumps(cfg,indent=2))


def encode(raw,output,speed):
    if not speed:
        shared.checked(['ffmpeg','-v','error','-i',str(raw),'-an','-vf','fps=60','-c:v','libx264','-preset','slow','-crf','17','-pix_fmt','yuv420p','-movflags','+faststart',str(output)])
        return
    duration=float(shared.checked(['ffprobe','-v','error','-show_entries','format=duration','-of','default=nw=1:nk=1',str(raw)]))
    points=[];last=0
    for part in speed:
        points.extend([(last,part['start'],1),(part['start'],part['end'],part['rate'])]);last=part['end']
    points.append((last,duration,1));filters=[]
    for i,(start,end,rate) in enumerate(points):filters.append(f'[0:v]trim=start={start}:end={end},setpts=(PTS-STARTPTS)/{rate}[v{i}]')
    filters.append(''.join(f'[v{i}]' for i in range(len(points)))+f'concat=n={len(points)}:v=1:a=0,fps=60[v]')
    shared.checked(['ffmpeg','-v','error','-i',str(raw),'-filter_complex',';'.join(filters),'-map','[v]','-an','-c:v','libx264','-preset','slow','-crf','17','-pix_fmt','yuv420p','-movflags','+faststart',str(output)])


def record(tool,output):
    review=output.with_suffix('.review');review.mkdir(parents=True)
    with tempfile.TemporaryDirectory(prefix='sc-') as name:
        directory=Path(name);director=None
        stage=directory/'stage';stage.mkdir()
        for source in ['ShowcaseStage.qml','SuiteStage.qml']:shutil.copy2(shared.HERE/source,stage/source)
        (stage/'shell.qml').write_text('import Quickshell\nSuiteStage {}\n')
        pointer=shared.build_pointer(directory)
        with shared.native_session(directory) as (env,monitor):
            title,heading,caption,_=STORIES[tool]
            env.update(XDG_DATA_DIRS=str(Path.home()/'.local/share')+':/usr/local/share:/usr/share',SHOWCASE_MONITOR=monitor,SHOWCASE_TOOL=tool,SHOWCASE_NAME=title,SHOWCASE_HEADING=heading,SHOWCASE_CAPTION=caption,
                       SHOWCASE_ICONS='file://'+str(ROOT/'docs/icons'),TJIKUP_NOTIFY='0',MAILTJIK_BACKEND=str(ROOT/'apps/mailtjik/backend/mailtjik_backend.py'))
            previous=os.environ.copy();os.environ.clear();os.environ.update(env)
            recorder=stage_process=None
            try:
                with (directory/'stage.log').open('w') as log:
                    stage_process=subprocess.Popen(['quickshell','--path',str(stage),'--no-color'],env=env,stdout=log,stderr=log)
                shared.wait_for(lambda:json.loads(qs(stage,'showcase','request',json.dumps({'mode':'intro','chapterCount':3})))['ready'])
                director=Director(directory,env,pointer,monitor,review,tool)
                raw=directory/'raw.mp4'
                with (directory/'record.log').open('w') as log:
                    director.started=time.monotonic()
                    recorder=subprocess.Popen(['wf-recorder','-D','--no-dmabuf','-o',monitor,'-r','60','-c','libx264rgb','-p','preset=ultrafast','-p','crf=0','-x','bgr0','-f',str(raw)],env=env,stdout=log,stderr=log)
                    time.sleep(.3)
                    try:director.run()
                    except BaseException:
                        director.frame('failure')
                        if hasattr(director,'socket'):(review/'failure.txt').write_text(director.text())
                        raise
                    recorder.send_signal(signal.SIGINT);recorder.wait(timeout=15);recorder=None
                encode(raw,output,director.speed)
                probe=json.loads(shared.checked(['ffprobe','-v','error','-show_streams','-show_format','-of','json',str(output)]));video=probe['streams'][0]
                if (video['width'],video['height'],video['r_frame_rate'])!=(1200,720,'60/1'):raise ValueError('Unexpected film geometry')
                receipt={'tool':tool,'dimensions':[1200,720],'fps':60,'duration':probe['format']['duration'],'sha256':hashlib.sha256(output.read_bytes()).hexdigest(),
                    'sourceCommit':shared.checked(['git','-C',str(ROOT),'rev-parse','HEAD']),'input':'native Wayland pointer and keyboard; actual application rendering; synthetic data only',
                    'iconSha256':hashlib.sha256((ROOT/'docs/icons'/f'{tool}.svg').read_bytes()).hexdigest(),'speedSegments':director.speed,'chapters':director.chapters,'evidence':director.events}
                output.with_suffix('.json').write_text(json.dumps(receipt,indent=2)+'\n');print('Recording: '+str(output),flush=True)
            finally:
                if recorder and recorder.poll() is None:recorder.send_signal(signal.SIGINT);recorder.wait(timeout=15)
                if director:
                    (review/'input-evidence.json').write_text(json.dumps(director.events,indent=2))
                    director.pointer.stdin.close();director.pointer.wait(timeout=5)
                    for process in reversed(director.processes):
                        if process.poll() is None:process.terminate();process.wait(timeout=8)
                if stage_process and stage_process.poll() is None:stage_process.terminate();stage_process.wait(timeout=8)
                os.environ.clear();os.environ.update(previous)
                for file in directory.glob('*.log'):shutil.copy2(file,review/file.name)


def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('tool',choices=STORIES);parser.add_argument('output',type=Path,nargs='?');parser.add_argument('--plan',action='store_true');args=parser.parse_args()
    if args.plan:print(json.dumps(STORIES[args.tool],indent=2));return
    if not args.output:parser.error('recording requires OUTPUT.mp4')
    output=args.output.expanduser().absolute()
    if output.suffix!='.mp4' or any(path.exists() for path in [output,output.with_suffix('.json'),output.with_suffix('.review')]):parser.error('Choose an unused MP4, receipt and review path')
    output.parent.mkdir(parents=True,exist_ok=True);record(args.tool,output)

if __name__=='__main__':main()
