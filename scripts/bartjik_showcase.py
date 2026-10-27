#!/usr/bin/env python3
"""Native Bartjik film with public market/music snapshots and isolated controls."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import shlex
import subprocess
import sys
import tempfile
import time
import os_showcase as shared

ROOT = shared.ROOT
SOURCE = ROOT / "apps/bartjik"
SCALE = 1.2
FEATURES = (
    ("Real rates. Fluid tracking.", "CoinGecko prices and history; move the pointer to follow the panel."),
    ("The music has a pulse.", "Real album art moves to the bass of Karla Blum’s Acid Rain."),
    ("See your workspaces before switching.", "Hover for live window previews, then click to change workspace."),
    ("Connections, within reach.", "VPN locations, Tailscale devices and connection controls."),
    ("Your desktop, under control.", "Usage, audio, clocks and desktop/game mode in a second bar."),
    ("A softer landing.", "Drag a chart away; its release settles into an independent window."),
)
PUBLIC_DATA = Path.home()/"Videos/tjikos/os/bartjik-public-20261028/snapshot.json"



def prepare(app):
    qml = app / "qml"
    shutil.copy2(shared.HERE / "ShowcaseStage.qml", qml / "ShowcaseStage.qml")
    shutil.copy2(shared.HERE / "CapturePlayback.qml", qml / "CapturePlayback.qml")
    shutil.copy2(shared.HERE / "showcase_public.py", app / "bartjik_core/showcase_public.py")
    demo=app/"bartjik_core/demo.py"
    text=demo.read_text().replace('    options = options or {}', '    options = options or {}\n    from .showcase_public import items as captured_items\n    captured=captured_items(provider,options)\n    if captured is not None:return captured',1)
    demo.write_text(text)
    engine=app/"bartjik_core/engine.py"
    text=engine.read_text().replace('            if self.demo:\n                import math', '            if self.demo and os.environ.get("BARTJIK_PUBLIC_DATA"):\n                from .showcase_public import DATA\n                result=DATA["markets"][context["item"]]["chart"]\n            elif self.demo:\n                import math',1)
    text=text.replace('    async def panel_action(self, msg):\n        if self.demo:\n            return', '    async def panel_action(self, msg):\n        if self.demo:\n            from .showcase_public import control\n            if control(msg["context"]):self.changed()\n            return',1)
    if 'import os\n' not in text:text='import os\n'+text
    engine.write_text(text)
    # Use the actual private compositor windows for the workspace chapter.
    workspace=qml/"WorkspacePreview.qml"
    text=workspace.read_text().replace('popup.controller.demo', 'false').replace('controller.demo', 'false')
    workspace.write_text(text)

    shared.replace(qml / "shell.qml", "    id: root\n", '''    id: root
    readonly property bool ready: state.ready
    property var captureData:({})
    property bool captureGame:false
    property string captureWorkspace:'9'
    Process {
        id:captureSwitch
        command:['hyprctl','eval','hl.dispatch(hl.dsp.focus({workspace='+JSON.stringify(root.captureWorkspace)+'}))']
        onExited:{Hyprland.refreshMonitors();Hyprland.refreshToplevels();}
    }
    CapturePlayback {id:captureMusic;metadata:root.captureData.music || ({})}
    ShowcaseStage {
        id: showcaseStage; controller: root
        toolName: "Bartjik"; toolId: "bartjik"
        repository: "github.com/tijptjik/tjikos/tree/main/apps/bartjik"
        heading: "Panels at your fingertips"
        introHeading: "Panels at your fingertips"
        introCaption: "Real rates. Music that moves. Your desktop, connected."
        iconRoot: "file://" + Quickshell.env("SHOWCASE_ICONS")
    }
''')
    shared.replace(qml / "shell.qml", "        case 'status':break;", '''        case 'showcase': {
            if(req.scene)showcaseStage.scene(req.scene);
            if(req.publicData)root.captureData=req.publicData;
            if(req.play!==undefined){if(req.play)captureMusic.play();else captureMusic.pause();}
            function find(item,name){
                if(item.objectName===name && item.visible && item.enabled)return item;
                for(const child of (item.children || [])){const match=find(child,name);if(match)return match;}
                return null;
            }
            const cards=cryptoPanels.map(p=>({pair:p.pair,opened:p.panelVisible,detached:p.detached,
                dropping:p.dropping,dragging:p.dragging,points:p.points.length,placement:p.placement(),
                dropFrames:p.dropFrames,sourceFrames:p.sourceFrames}));
            if(!req.panel)return {ok:true,connection:detailPanel.panelData,ready:state.ready,targets:[].concat(...panels.instances.map(p=>p.showcaseTargets())),
                detail:detailPanel.inspect(),crypto:cards,pointer:pointer,workspace:workspacePreview.item?.inspect() || {},
                music:{playing:captureMusic.playing,position:captureMusic.position,energy:captureMusic.energy,pulse:detailPanel.showcasePulse},
                mode:captureGame?'Game Mode':'Desktop Mode',workspaceId:Hyprland.focusedMonitor?.activeWorkspace?.id,
                toplevels:Hyprland.toplevels.values.map(t=>({address:t.address,workspace:t.workspace?.name,monitor:t.monitor?.name,mapped:t.lastIpcObject.mapped,hidden:t.lastIpcObject.hidden,wayland:!!t.wayland}))};
            const host=req.panel==='workspace'?workspacePreview.item:req.panel==='crypto'?(req.pair?cryptoPanels.find(p=>p.pair===req.pair && p.panelVisible):cryptoPanel):detailPanel;
            if(!host || !host.panelVisible)return {ok:true,present:false};
            const item=req.target?find(host.panelBody,req.target):host.panelBody;
            if(!item)return {ok:true,present:true,targetReady:false};
            const pos=host.detached?host.placement():host.attachedPosition();
            const point=item.mapToItem(host.panelBody, item.width*(req.fraction ?? .5),req.header?24:item.height/2);
            return {ok:true,present:true,targetReady:true,x:pos.x+point.x,y:pos.y+point.y,
                left:pos.x,top:pos.y,width:item.width,height:item.height,bodyWidth:host.panelBody.width,
                bodyHeight:host.panelBody.height,value:item.value ?? null,scroll:item.contentY ?? null,
                opened:host.panelVisible,detached:host.detached,dropping:host.dropping,
                dropFrames:host.dropFrames,sourceFrames:host.sourceFrames,
                placement:host.placement(),points:host.points?.length || 0,
                audioNodes:req.panel==='detail'?detailPanel.showcaseAudioNodes():[],
                nativePlaced:host.showcasePlaced,nativeAddress:host.showcaseAddress,releaseProgress:host.showcaseRelease,
                trace:req.trace?host.showcaseEvents:[]};
        }
        case 'status':break;''')
    shared.replace(qml/"shell.qml", "import Quickshell.Io", "import Quickshell.Io\nimport Quickshell.Hyprland")
    shared.replace(qml/"shell.qml", "        if(demo)return;", '''        if(demo && action==='player.toggle'){captureMusic.toggle();return;}
        if(demo && action==='workspace.activate'){root.captureWorkspace=row.id;captureSwitch.running=true;return;}
        if(demo && action==='monitor.toggle'){captureGame=!captureGame;return;}
        if(demo)return;''')
    shared.replace(qml/"shell.qml", "        return (state.providers[bar.sources[id]] || {}).items || [];", '''        const rows=(state.providers[bar.sources[id]] || {}).items || [];
        if(demo && m.provider==='playerctl')return rows.map(r=>Object.assign({},r,{icon:Object.assign({},r.icon,{value:captureMusic.playing?'pause':'play_arrow'}),media:Object.assign({},r.media,{playing:captureMusic.playing,energy:captureMusic.energy})}));
        if(demo && m.provider==='workspaces')return rows.map(r=>Object.assign({},r,{state:Object.assign({},r.state,{active:String(Hyprland.focusedMonitor?.activeWorkspace?.id)===r.id,focused:String(Hyprland.focusedMonitor?.activeWorkspace?.id)===r.id})}));
        if(demo && m.provider==='monitor-layout')return rows.map(r=>Object.assign({},r,{type:'icon_text',text:Object.assign({},r.text,{value:root.captureGame?'Game':'Desktop'}),tooltip:root.captureGame?'Game Mode':'Desktop Mode',state:{active:root.captureGame}}));
        return rows;''')
    shared.replace(qml / "Bar.qml", "    function capture(path)", '''    function showcaseTargets(){
        return [].concat(...groups.children.filter(c=>c.islandId && c.visible).map(island=>island.previewTargets().map(t=>{
            const p=island.mapToItem(panel.contentItem,t.x+t.width/2,t.y+t.height/2);
            return {module:t.context.module,item:t.row.id,part:t.context.part || '',x:p.x,y:p.y,width:t.width,height:t.height};
        })));
    }
    function capture(path)''')
    shared.replace(qml / "DetachablePopup.qml", "    id:host\n", '''    id:host
    readonly property real showcaseRelease:nativeWindow.releaseProgress
    readonly property bool showcasePlaced:nativeWindow.placed
    readonly property string showcaseAddress:nativeWindow.address
    property var showcaseEvents:[]
    function showcaseMove(point){
        if(showcaseEvents.length<150)showcaseEvents=showcaseEvents.concat([{time:Date.now(),x:point.x,y:point.y,
            originX:dragOrigin.x,originY:dragOrigin.y,grabX:grabOffset.x,grabY:grabOffset.y,
            dropX:dropPosition.x,dropY:dropPosition.y,attachedX:attachedPosition().x,attachedY:attachedPosition().y}]);
    }
''')
    shared.replace(qml / "DetachablePopup.qml", "            host.moveDrag(point);", "            host.moveDrag(point);host.showcaseMove(point);")
    shared.replace(qml / "MediaDetails.qml", "                    id:playbackButton", '                    id:playbackButton\n                    objectName:"showcase-playback-"+modelData.action')
    shared.replace(qml / "MediaDetails.qml", "            id:lyricsButton", '            id:lyricsButton\n            objectName:"showcase-lyrics"')
    shared.replace(qml / "AudioGroup.qml", "            id: slider\n", '            id: slider\n            objectName:"showcase-volume"\n')
    shared.replace(qml / "PanelHandle.qml", "PanelButton {", 'PanelButton {\n    objectName:"showcase-close"')
    shared.replace(qml / "DetailPanel.qml", "    id:popup\n", '    id:popup\n    readonly property real showcasePulse:mediaDetails.musicEnergy\n    function showcaseAudioNodes(){return audioDetails.demoNodes.map(n=>({name:n.name,volume:n.audio.volume}));}\n')
    # Native panel viewports fit below the film captions at this compositor scale.
    shared.replace(qml / "DetailPanel.qml", "    readonly property real targetHeight:", "    readonly property real targetHeight:Math.min(330, ")
    p=qml / "DetailPanel.qml"
    lines=p.read_text().splitlines()
    lines=[line+")" if line.startswith("    readonly property real targetHeight:") else line for line in lines]
    p.write_text("\n".join(lines)+"\n")


def configuration(monitor):
    cfg=json.loads((SOURCE / "examples/comparison.json").read_text())
    cfg["style"].update(previewHeight=300,fontSize=15)
    cfg["layout"]["centre_width"]="100%"
    cfg["perMonitor"]={}
    common={"monitors":[monitor],"side":"b","position":"c","alignment":"c","autoHide":False,"thickness":44,"islandGap":9,"layer":"overlay"}
    cfg["bars"]=[dict(common,id="workspace-bar",offsetY="-22%",islands=[
        {"id":"markets","modules":["prices"]},{"id":"media","modules":["music"]},{"id":"workspaces","modules":["spaces"]}]),
        dict(common,id="status-bar",offsetY="-1%",islands=[
        {"id":"usage","modules":["quota"]},{"id":"network","modules":["net"]},
        {"id":"vpn","modules":["vpn"]},{"id":"tailnet","modules":["tailnet"]},
        {"id":"audio-clock","modules":["sound","time"]},{"id":"mode","modules":["display"]}])]
    cfg["style"]["previewHeight"]=260
    cfg["modules"]["spaces"].update(appearance="ring",labels={"10":"X"},preview={"enabled":True,"delay":260,"closeDelay":500})
    for module in cfg["modules"].values():
        module["demoPresentation"]=True
        module["panel"]={"hover":True,"delay":260,"closeDelay":500}
    for field in ("artist","track"):
        cfg["modules"]["music"].setdefault(field,{}).setdefault("style",{}).update(maxTextWidth=100,marquee=False)
    return cfg


class Director(shared.Director):
    def __init__(self,app,env,pointer,monitor,review):
        self.app,self.env,self.review=app,env,review
        self.pointer=subprocess.Popen([str(pointer),monitor],env=env,stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True,bufsize=1)
        self.position=(950,560)
        self.events,self.chapters=[],[]
        self.started=time.monotonic()
        self.move(*self.position,duration=.1)

    def ipc(self,**request):
        from bartjik import ipc
        return ipc(request)

    def command(self,command):
        if command.startswith("M "):
            _,x,y=command.split()
            command=f"M {round(int(x)*SCALE)} {round(int(y)*SCALE)}"
        super().command(command)

    def scene(self,number):
        heading,caption=FEATURES[number-1]
        self.observe(scene={"mode":"feature","heading":heading,"caption":caption,"chapter":number,"chapterCount":len(FEATURES)})
        self.chapters.append({"feature":heading,"start":round(time.monotonic()-self.started,3)})
        print(f"{number}/{len(FEATURES)} {heading}",flush=True)
        time.sleep(.65)

    def aim(self,module,item=None):
        targets=self.observe()["targets"]
        target=next(t for t in targets if t["module"]==module and (item is None or t["item"]==item))
        self.move(target["x"],target["y"],duration=.8)
        shared.wait_for(lambda:(lambda p:p.get("valid") and abs(p.get("x",0)-target["x"])<2 and abs(p.get("y",0)-target["y"])<2)(self.observe()["pointer"]))
        self.events.append({"time":round(time.monotonic()-self.started,3),"input":"native hover","module":module,"item":target["item"]})
        return target

    def target(self,**kwargs):
        state=shared.wait_for(lambda:(lambda v:v if v.get("present") and v.get("targetReady") else None)(self.observe(**kwargs)))
        if state.get("detached") and state.get("nativePlaced") and state.get("nativeAddress"):
            client=next((c for c in json.loads(shared.checked(["hyprctl","-j","clients"])) if c["address"]==state["nativeAddress"]),None)
            monitor=next(m for m in json.loads(shared.checked(["hyprctl","-j","monitors"])) if m["name"]==self.env["SHOWCASE_MONITOR"])
            if client:
                left,top=client["at"][0]-monitor["x"],client["at"][1]-monitor["y"]
                state["x"]+=left-state["left"];state["y"]+=top-state["top"]
                state["left"],state["top"]=left,top
                state["placement"]={"monitor":monitor["name"],"x":left,"y":top}
                state["nativeClient"]={"floating":client["floating"],"size":client["size"],"at":client["at"]}
        return state

    def click(self,**kwargs):
        target=self.target(**kwargs)
        for _ in range(5 if kwargs.get('panel')=='detail' else 0):
            if target['top']+75<target['y']<target['top']+target['bodyHeight']-24:break
            self.move(target['left']+target['bodyWidth']*.75,target['top']+target['bodyHeight']*.65,duration=.4)
            direction=1 if target['y']>target['top']+target['bodyHeight']-24 else -1
            for _ in range(12):self.command(f'S 0 {direction*8}');time.sleep(1/60)
            self.command('S 0 0');time.sleep(.25)
            target=self.target(**kwargs)
        self.move(target["x"],target["y"])
        time.sleep(.2)
        target=self.target(**kwargs)
        self.move(target["x"],target["y"],duration=.15)
        def aligned():
            pointer=self.observe()['pointer']
            return pointer.get('valid') and abs(pointer['x']-target['x'])<2 and abs(pointer['y']-target['y'])<2
        if not aligned():self.move(target['x'],target['y'],duration=.2)
        shared.wait_for(aligned)
        time.sleep(.08)
        self.command("B 272 1");time.sleep(.09);self.command("B 272 0")
        self.events.append({"time":round(time.monotonic()-self.started,3),"input":"native click","target":kwargs,"geometry":target,"pointer":self.observe()["pointer"]})
        if kwargs.get('panel')=='detail':
            self.move(target['left']+target['bodyWidth']*.6,target['top']+110,duration=.12)

    def detail(self,provider):
        return shared.wait_for(lambda:(lambda s:s if s.get("opened") and s.get("provider")==provider and s.get("crossfade")==1 else None)(self.observe()["detail"]))

    def graph(self,pair):
        return shared.wait_for(lambda:next((s for s in self.observe()["crypto"] if s["pair"]==pair and s["opened"] and s["points"]>=20),None))

    def evidence(self,label,**values):
        self.events.append({"time":round(time.monotonic()-self.started,3),"verified":label,**values})

    def launch_workspaces(self,directory):
        self.windows=[]
        from_text=[('1','Editor',SOURCE/'bartjik_core/history.py'),('1','Panels',SOURCE/'qml/DetailPanel.qml'),('2','Workspaces',SOURCE/'qml/WorkspacePreview.qml'),('3','Configuration',SOURCE/'examples/demo.json')]
        for workspace,title,path in from_text:
            shared.checked(['hyprctl','eval','hl.dispatch(hl.dsp.focus({workspace='+json.dumps(workspace)+'}))'])
            process=subprocess.Popen(['kitty','--config','NONE','--title',title,'--class','BartjikCapture',
                '-o','font_family=Monaspace Argon','-o','font_size=12','-o','background=#191724','-o','foreground=#e5e5e5',
                'vim','-u','NONE','-i','NONE','-n','-R',str(path),'-c','set number','-c','syntax on'],env=self.env,cwd=directory,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
            self.windows.append(process)
            shared.wait_for(lambda:any(c['title']==title for c in json.loads(shared.checked(['hyprctl','-j','clients']))))
            time.sleep(.7)
        self.focus_workspace('9')

    def focus_workspace(self,number):
        shared.checked(['hyprctl','eval','hl.dispatch(hl.dsp.focus({workspace='+json.dumps(str(number))+'}))'])
        time.sleep(.25)

    def describe(self,text):
        self.observe(scene={'caption':text,'animate':False})

    def click_bar(self,module,item=None):
        target=self.aim(module,item)
        self.command('B 272 1');time.sleep(.09);self.command('B 272 0')
        self.events.append({'time':round(time.monotonic()-self.started,3),'input':'native bar click','module':module,'item':target['item']})

    def run(self,source):
        self.observe(scene={'mode':'intro','heading':'Panels at your fingertips','caption':'Real rates. Music that moves. Your desktop, connected.','chapterCount':len(FEATURES)})
        time.sleep(2.3);self.frame('00-intro')
        self.scene(1);self.ipc(op='show',bar='all');time.sleep(.65)
        self.evidence('two native bars',bars=self.ipc(op='bars')['bars'])
        for pair in ('BTC/USD','SOL/USD','AR/USD'):
            target=self.aim('prices',pair);state=self.graph(pair)
            self.evidence('public market panel',pair=pair,points=state['points'])
            time.sleep(.6)
        self.describe('Move across the rate: the native panel follows the pointer.')
        target=self.aim('prices','AR/USD')
        distance=min(25,target['width']*.3)
        self.move(target['x']-distance,target['y'],duration=1.0);time.sleep(.2)
        left=self.target(panel='crypto',pair='AR/USD')['left']
        self.move(target['x']+distance,target['y'],duration=1.5);time.sleep(.2)
        right=self.target(panel='crypto',pair='AR/USD')['left']
        if right-left<12:raise ValueError('Native attached panel did not track the pointer: '+str((left,right)))
        self.evidence('fluid native pointer tracking',left=left,right=right,distance=right-left)
        self.frame('01-real-rates-tracking')
        self.scene(2);self.observe(play=True);self.music_started=time.monotonic()-self.started
        self.aim('music');self.detail('playerctl')
        shared.wait_for(lambda:self.observe()['detail'].get('artworkReady'))
        self.describe('Acid Rain — real cover art, with bass-driven movement from the preview audio.')
        samples=[]
        for index in range(16):
            state=self.observe()['music'];samples.append(state['energy']);time.sleep(.22)
            if index==4:self.frame('02-album-art-pulse')
        if max(samples)-min(samples)<.15 or self.observe()['music']['position']<1000:
            raise ValueError('Public music preview is not driving the native artwork')
        self.evidence('real music pulse',position=self.observe()['music']['position'],energyRange=[min(samples),max(samples)])
        self.observe(play=False);self.music_ended=time.monotonic()-self.started
        self.scene(3);self.observe(scene={'coverWindows':True,'animate':False})
        for number in ('1','2','3'):
            self.aim('spaces',number)
            state=shared.wait_for(lambda:(lambda s:s if s.get('opened') and s.get('windows',0)>0 and s.get('captured',0)>0 else None)(self.observe()['workspace']))
            self.evidence('real workspace window thumbnails',workspace=number,windows=state['windows'],captured=state['captured'])
            time.sleep(.65)
        self.describe('These are live application windows; click a workspace to switch.')
        self.click_bar('spaces','2')
        shared.wait_for(lambda:any(m.get('focused') and m['activeWorkspace']['id']==2 for m in json.loads(shared.checked(['hyprctl','-j','monitors']))))
        time.sleep(.9);self.frame('03-workspace-switcher')
        self.evidence('native workspace activation',workspace=2)
        self.focus_workspace('9');self.observe(scene={'coverWindows':False,'animate':False})
        self.ipc(op='hide',bar='workspace-bar')
        self.scene(4)
        self.aim('vpn');self.detail('mullvad');time.sleep(.7)
        self.describe('Select a VPN country from the panel’s quick locations.')
        self.click(panel='detail',target='mullvad-country-nl')
        shared.wait_for(lambda:self.observe().get('connection',{}).get('countryCode')=='nl')
        self.frame('04-vpn-locations');self.evidence('native VPN country control',country='nl')
        self.aim('tailnet');self.detail('tailscale');time.sleep(.6)
        self.describe('Tailscale brings device status and connect/disconnect into the same workflow.')
        self.click(panel='detail',target='tailscale-toggle')
        shared.wait_for(lambda:self.observe().get('connection',{}).get('connection')=='Stopped')
        self.click(panel='detail',target='tailscale-toggle')
        shared.wait_for(lambda:self.observe().get('connection',{}).get('connection')=='Running')
        self.frame('05-tailscale-controls');self.evidence('native Tailscale controls',connected=True)
        self.scene(5)
        self.aim('quota');state=self.detail('codex');time.sleep(1.0)
        self.describe('Capacity and reset times stay readable; audio and clocks are one hover away.')
        self.frame('06-usage')
        self.aim('sound');self.detail('audio');time.sleep(.6)
        slider=self.target(panel='detail',target='showcase-volume');left=slider['x']-slider['width']/2
        self.move(left+slider['width']*.6,slider['y']);self.command('B 272 1')
        self.move(left+slider['width']*.32,slider['y'],duration=.8);self.command('B 272 0')
        self.evidence('native audio slider',value=self.target(panel='detail',target='showcase-volume')['value'])
        self.aim('time');self.detail('datetime');time.sleep(.6)
        self.describe('Desktop / Game mode is part of the second bar’s quick controls.')
        self.click_bar('display');shared.wait_for(lambda:self.observe()['mode']=='Game Mode')
        time.sleep(.6);self.frame('07-mode-control');self.click_bar('display')
        self.evidence('native mode button',mode=self.observe()['mode'])
        self.detach()

    def detach(self):
        self.ipc(op='hide',bar='status-bar');self.ipc(op='show',bar='workspace-bar')
        self.scene(6)
        self.aim("prices","BTC/USD");self.graph("BTC/USD")
        time.sleep(.5)
        card=self.target(panel="crypto",pair="BTC/USD",header=True,fraction=.08)
        self.move(card["x"],card["y"])
        time.sleep(.3)
        card=self.target(panel="crypto",pair="BTC/USD",header=True,fraction=.08)
        self.move(card["x"],card["y"],duration=.25)
        time.sleep(.25)
        self.command("B 272 1");time.sleep(.15)
        card=self.target(panel="crypto",pair="BTC/USD",header=True,fraction=.08)
        self.move(self.position[0]+200,self.position[1],duration=.9)
        self.command("B 272 0")
        release_started=time.monotonic();release_samples=[]
        def settled():
            state=self.target(panel="crypto",pair="BTC/USD")
            release_samples.append({'ms':round((time.monotonic()-release_started)*1000),'progress':state.get('releaseProgress',0)})
            return state if state.get('detached') and not state.get('dropping') and state.get('nativePlaced') and state.get('dropFrames',0)>=2 else None
        dropped=shared.wait_for(settled)
        self.evidence('native exponential release',samples=release_samples)
        time.sleep(.4)
        # Reposition the detached xdg-toplevel through its native system move.
        dropped=self.target(panel="crypto",pair="BTC/USD",header=True,fraction=.08)
        self.evidence("native chart detached",placement=dropped["placement"],dropFrames=dropped["dropFrames"],sourceFrames=dropped["sourceFrames"])
        self.move(dropped["x"],dropped["y"])
        self.command("B 272 1");time.sleep(.15)
        self.move(self.position[0]+12,self.position[1],duration=.15)
        time.sleep(.15)
        moving=self.target(panel="crypto",pair="BTC/USD",header=True,fraction=.08)
        self.move(self.position[0]+528-moving["left"],self.position[1]+156-moving["top"],duration=1.1)
        self.command("B 272 0");time.sleep(.6)
        dropped=self.target(panel="crypto",pair="BTC/USD")
        (self.review/"drag-state.json").write_text(json.dumps(dropped,indent=2))
        if not dropped.get("nativeClient",{}).get("floating") or dropped["left"]<480 or dropped["left"]+dropped["bodyWidth"]>1000:
            raise ValueError("Native floating graph is not placed beside the attached panel: "+str(dropped))
        self.evidence("native compositor window move",placement=dropped["placement"],client=dropped["nativeClient"])
        self.frame("08-detached")
        self.aim("prices","SOL/USD");self.graph("SOL/USD")
        old=self.target(panel="crypto",pair="BTC/USD")
        if not old["detached"] or old["placement"]!=dropped["placement"]:
            raise ValueError("Detached chart moved while opening another panel")
        self.evidence("independent native panels",pairs=["BTC/USD","SOL/USD"],detachedPlacement=old["placement"])
        time.sleep(2.2);self.frame("09-two-panels")
        self.click(panel="crypto",pair="BTC/USD",target="showcase-close")
        shared.wait_for(lambda:not any(c["opened"] and c["pair"]=="BTC/USD" for c in self.observe()["crypto"]))
        self.move(950,560);time.sleep(.85)
        self.ipc(op="hide",bar="all")
        self.observe(scene={"mode":"outro","chapter":len(FEATURES)})
        time.sleep(1.2);self.frame("10-outro");time.sleep(2)


def record(output):
    sys.path.insert(0,str(SOURCE))
    snapshot=json.loads(PUBLIC_DATA.read_text())
    review=output.with_suffix(".review");review.mkdir(parents=True)
    with tempfile.TemporaryDirectory(prefix="bc-") as name:
        directory=Path(name);app=directory/"app"
        for part in ("qml","themes","bartjik_core"):
            shutil.copytree(SOURCE/part,app/part)
        shutil.copy2(SOURCE/"bartjik.py",app/"bartjik.py")
        prepare(app)
        pointer=shared.build_pointer(directory)
        try:
            with shared.native_session(directory,scale=SCALE) as (env,monitor):
                env.update(BARTJIK_HOME=str(app),BARTJIK_QML=str(app/"qml"),BARTJIK_CONFIG=str(directory/"config.json"),
                           BARTJIK_DEMO="1",BARTJIK_PUBLIC_DATA=str(PUBLIC_DATA),BARTJIK_CAPTURE_AUDIO=Path(snapshot["music"]["audio"]).as_uri(),SHOWCASE_ICONS=str(ROOT/"docs/icons"),SHOWCASE_MONITOR=monitor)
                Path(env["BARTJIK_CONFIG"]).write_text(json.dumps(configuration(monitor)))
                previous=os.environ.copy();os.environ.clear();os.environ.update(env)
                child=recorder=director=None
                try:
                    with (directory/"app.log").open("w") as log,(directory/"record.log").open("w") as capture_log:
                        child=subprocess.Popen(["quickshell","--path",str(app/"qml"),"--no-color"],env=env,stdout=log,stderr=log)
                        from bartjik import ipc
                        shared.wait_for(lambda:ipc({"op":"status"}).get("ready"))
                        ipc({"op":"hide","bar":"all"})
                        director=Director(app,env,pointer,monitor,review)
                        director.observe(publicData={"music":snapshot["music"]})
                        director.launch_workspaces(directory)
                        time.sleep(2)
                        raw=directory/"capture.mp4"
                        director.started=time.monotonic()
                        recorder=subprocess.Popen(["wf-recorder","-D","--no-dmabuf","-o",monitor,"-r","60","-c","libx264rgb","-p","preset=ultrafast","-p","crf=0","-x","bgr0","-f",str(raw)],env=env,stdout=capture_log,stderr=capture_log)
                        time.sleep(.3)
                        if recorder.poll() is not None:raise ValueError("Recorder failed to start")
                        try:director.run(SOURCE)
                        except BaseException:
                            director.frame('failure')
                            (review/'failure-state.json').write_text(json.dumps(director.observe(),indent=2))
                            raise
                        recorder.send_signal(signal.SIGINT);recorder.wait(timeout=15);recorder=None
                        shared.checked(["ffmpeg","-v","error","-i",str(raw),"-an","-vf","fps=60","-c:v","libx264","-preset","slow","-crf","17","-pix_fmt","yuv420p","-movflags","+faststart",str(output)])
                        probe=json.loads(shared.checked(["ffprobe","-v","error","-show_streams","-show_format","-of","json",str(output)]))
                        stream=probe["streams"][0]
                        if (stream["width"],stream["height"],stream["r_frame_rate"])!=(1200,720,"60/1"):
                            raise ValueError("Unexpected capture dimensions")
                        receipt={"tool":"bartjik","dimensions":[1200,720],"fps":60,"dataMode":"real public market/music snapshots; isolated desktop controls",
                            "publicDataSha256":hashlib.sha256(PUBLIC_DATA.read_bytes()).hexdigest(),"fetchedAt":snapshot["fetchedAt"],"publicAssets":snapshot["assets"],
                            "input":"private Wayland compositor seat; no QtTest or comparison renderer", "scale":SCALE,
                            "audio":"none", "duration":probe["format"]["duration"],"sha256":hashlib.sha256(output.read_bytes()).hexdigest(),
                            "sourceCommit":shared.checked(["git","-C",str(ROOT),"rev-parse","HEAD"]),
                            "directorSha256":hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
                            "stageSha256":hashlib.sha256((shared.HERE/"ShowcaseStage.qml").read_bytes()).hexdigest(),
                            "chapters":director.chapters,"evidence":director.events}
                        output.with_suffix(".json").write_text(json.dumps(receipt,indent=2)+"\n")
                        print("Recording: "+str(output),flush=True)
                finally:
                    if recorder and recorder.poll() is None:recorder.send_signal(signal.SIGINT);recorder.wait(timeout=10)
                    if director:director.pointer.stdin.close();director.pointer.wait(timeout=5)
                    if director:
                        for window in getattr(director,"windows",[]):
                            if window.poll() is None:window.terminate();window.wait(timeout=5)
                    if child and child.poll() is None:child.terminate();child.wait(timeout=8)
                    os.environ.clear();os.environ.update(previous)
        except BaseException:
            if director:
                (review/"partial-evidence.json").write_text(json.dumps(director.events,indent=2))
            raise
        finally:
            for log in directory.glob("*.log"):shutil.copy2(log,review/log.name)


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output",type=Path,nargs="?")
    parser.add_argument("--plan",action="store_true")
    parser.add_argument("--data",type=Path,help="verified public rates, artwork and audio snapshot JSON")
    args=parser.parse_args()
    if args.plan:
        print(json.dumps({"dimensions":[1200,720],"fps":60,"features":FEATURES,"comparison":False},indent=2));return
    if not args.output:parser.error("recording requires OUTPUT.mp4")
    global PUBLIC_DATA
    if args.data:PUBLIC_DATA=args.data.expanduser().absolute()
    output=args.output.expanduser().absolute()
    if output.suffix!=".mp4" or any(p.exists() for p in (output,output.with_suffix(".json"),output.with_suffix(".review"))):
        parser.error("choose a new .mp4 path; existing media and evidence are never overwritten")
    output.parent.mkdir(parents=True,exist_ok=True)
    record(output)


if __name__=="__main__":main()
