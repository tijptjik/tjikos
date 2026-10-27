#!/usr/bin/env python3
"""Native Bartjik panels in the shared Rose Punk film stage; demo providers only."""
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

ROOT = shared.ROOT
SOURCE = ROOT / "apps/bartjik"
SCALE = 1.2
FEATURES = (
    ("Live charts, one hover away.", "Explore market details without leaving the bar."),
    ("One panel. Your whole desktop.", "Music, audio and everyday controls flow together."),
    ("Pull a panel into your workspace.", "Detach a chart, then open another beside it."),
)


def prepare(app):
    qml = app / "qml"
    shutil.copy2(shared.HERE / "ShowcaseStage.qml", qml / "ShowcaseStage.qml")
    shared.replace(qml / "shell.qml", "    id: root\n", '''    id: root
    readonly property bool ready: state.ready
    ShowcaseStage {
        id: showcaseStage; controller: root
        toolName: "Bartjik"; toolId: "bartjik"
        repository: "github.com/tijptjik/tjikos/tree/main/apps/bartjik"
        heading: "Panels at your fingertips"
        introHeading: "Panels at your fingertips"
        introCaption: "Live details. Fluid transitions. Detachable panels."
        iconRoot: "file://" + Quickshell.env("SHOWCASE_ICONS")
    }
''')
    shared.replace(qml / "shell.qml", "        case 'status':break;", '''        case 'showcase': {
            if(req.scene)showcaseStage.scene(req.scene);
            function find(item,name){
                if(item.objectName===name && item.visible && item.enabled)return item;
                for(const child of (item.children || [])){const match=find(child,name);if(match)return match;}
                return null;
            }
            const cards=cryptoPanels.map(p=>({pair:p.pair,opened:p.panelVisible,detached:p.detached,
                dropping:p.dropping,dragging:p.dragging,points:p.points.length,placement:p.placement(),
                dropFrames:p.dropFrames,sourceFrames:p.sourceFrames}));
            if(!req.panel)return {ok:true,ready:state.ready,targets:[].concat(...panels.instances.map(p=>p.showcaseTargets())),
                detail:detailPanel.inspect(),crypto:cards,pointer:pointer};
            const host=req.panel==='crypto'?(req.pair?cryptoPanels.find(p=>p.pair===req.pair && p.panelVisible):cryptoPanel):detailPanel;
            if(!host || !host.panelVisible)return {ok:true,present:false};
            const item=req.target?find(host.panelBody,req.target):host.panelBody;
            if(!item)return {ok:true,present:true,targetReady:false};
            const pos=host.detached?host.placement():host.attachedPosition();
            const point=item.mapToItem(host.panelBody, item.width*(req.fraction ?? .5),req.header?24:item.height/2);
            return {ok:true,present:true,targetReady:true,x:pos.x+point.x,y:pos.y+point.y,
                left:pos.x,top:pos.y,width:item.width,height:item.height,bodyWidth:host.panelBody.width,
                bodyHeight:host.panelBody.height,value:item.value ?? null,
                opened:host.panelVisible,detached:host.detached,dropping:host.dropping,
                dropFrames:host.dropFrames,sourceFrames:host.sourceFrames,
                placement:host.placement(),points:host.points?.length || 0,
                audioNodes:req.panel==='detail'?detailPanel.showcaseAudioNodes():[],
                nativePlaced:host.showcasePlaced,nativeAddress:host.showcaseAddress,
                trace:req.trace?host.showcaseEvents:[]};
        }
        case 'status':break;''')
    shared.replace(qml / "Bar.qml", "    function capture(path)", '''    function showcaseTargets(){
        return [].concat(...groups.children.filter(c=>c.islandId && c.visible).map(island=>island.previewTargets().map(t=>{
            const p=island.mapToItem(panel.contentItem,t.x+t.width/2,t.y+t.height/2);
            return {module:t.context.module,item:t.row.id,part:t.context.part || '',x:p.x,y:p.y,width:t.width,height:t.height};
        })));
    }
    function capture(path)''')
    shared.replace(qml / "DetachablePopup.qml", "    id:host\n", '''    id:host
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
    shared.replace(qml / "MediaDetails.qml", "            id:lyricsButton", '            id:lyricsButton\n            objectName:"showcase-lyrics"')
    shared.replace(qml / "AudioGroup.qml", "            id: slider\n", '            id: slider\n            objectName:"showcase-volume"\n')
    shared.replace(qml / "PanelHandle.qml", "PanelButton {", 'PanelButton {\n    objectName:"showcase-close"')
    shared.replace(qml / "DetailPanel.qml", "    id:popup\n", '    id:popup\n    function showcaseAudioNodes(){return audioDetails.demoNodes.map(n=>({name:n.name,volume:n.audio.volume}));}\n')
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
    cfg["bars"]=[{"id":"showcase","monitors":[monitor],"side":"b","position":"c","alignment":"c",
        "offsetY":"-11%","autoHide":False,"thickness":44,"islandGap":9,
        "islands":[{"id":"markets","modules":["prices"]},{"id":"media","modules":["music"]},
            {"id":"usage","modules":["quota"]},{"id":"desktop","modules":["sound","net","time"]}]}]
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
        self.move(target["x"],target["y"])
        time.sleep(.2)
        target=self.target(**kwargs)
        self.move(target["x"],target["y"],duration=.15)
        self.command("B 272 1");time.sleep(.09);self.command("B 272 0")
        self.events.append({"time":round(time.monotonic()-self.started,3),"input":"native click","target":kwargs})

    def detail(self,provider):
        return shared.wait_for(lambda:(lambda s:s if s.get("opened") and s.get("provider")==provider and s.get("crossfade")==1 else None)(self.observe()["detail"]))

    def graph(self,pair):
        return shared.wait_for(lambda:next((s for s in self.observe()["crypto"] if s["pair"]==pair and s["opened"] and s["points"]==288),None))

    def evidence(self,label,**values):
        self.events.append({"time":round(time.monotonic()-self.started,3),"verified":label,**values})

    def run(self,source):
        self.observe(scene={"mode":"intro","heading":"Panels at your fingertips","caption":"Live details. Fluid transitions. Detachable panels.","chapterCount":len(FEATURES)})
        time.sleep(2.3);self.frame("00-intro")
        self.scene(1)
        self.ipc(op="show",bar="all")
        time.sleep(.65)
        targets=self.observe()["targets"]
        self.evidence("native bar layout",bars=self.ipc(op="bars")["bars"])
        if not targets or any(t["x"]<0 or t["x"]>1000 for t in targets):
            raise ValueError("Bar targets do not fit the native stage: "+str(targets))
        for pair in ("BTC/USD","SOL/USD","AR/USD"):
            self.aim("prices",pair)
            state=self.graph(pair)
            self.evidence("native market panel",pair=pair,points=state["points"])
            time.sleep(1.15)
            self.frame("01-market-"+pair.split("/")[0].lower())
        self.scene(2)
        self.aim("music")
        state=self.detail("playerctl")
        shared.wait_for(lambda:self.observe()["detail"].get("artworkReady"))
        time.sleep(.9);self.frame("02-media")
        self.click(panel="detail",target="showcase-lyrics")
        shared.wait_for(lambda:self.observe()["detail"].get("lyricsShown"))
        time.sleep(1.2);self.frame("03-lyrics")
        self.evidence("native lyrics toggle",shown=True)
        self.aim("quota");state=self.detail("codex")
        time.sleep(1.7);self.frame("04-quota")
        self.evidence("native usage panel",provider=state["provider"],rows=state["count"])
        self.aim("sound");self.detail("audio")
        time.sleep(.8)
        slider=self.target(panel="detail",target="showcase-volume")
        left=slider["x"]-slider["width"]/2
        self.move(left+slider["width"]*.6,slider["y"])
        self.command("B 272 1")
        self.move(left+slider["width"]*.32,slider["y"],duration=.9)
        self.command("B 272 0")
        level=self.target(panel="detail",target="showcase-volume")
        if not .25<level["value"]<.4:raise ValueError("Native audio slider did not move: "+str(level))
        self.evidence("native audio slider",value=level["value"],nodes=level["audioNodes"])
        time.sleep(.7);self.frame("05-audio")
        self.aim("net");state=self.detail("network")
        time.sleep(1.5);self.frame("06-network")
        self.evidence("native networking panel",devices=state["network"]["devices"])
        self.aim("time");state=self.detail("datetime")
        time.sleep(1.5);self.frame("07-clock")
        self.evidence("native world clocks",rows=state["count"])
        self.detach()

    def detach(self):
        self.scene(3)
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
        dropped=shared.wait_for(lambda:(lambda s:s if s.get("detached") and not s.get("dropping") and s.get("nativePlaced") and s.get("dropFrames",0)>=2 else None)(self.target(panel="crypto",pair="BTC/USD")))
        time.sleep(.4)
        # Reposition the detached xdg-toplevel through its native system move.
        dropped=self.target(panel="crypto",pair="BTC/USD",header=True,fraction=.08)
        self.evidence("native chart detached",placement=dropped["placement"],dropFrames=dropped["dropFrames"],sourceFrames=dropped["sourceFrames"])
        self.move(dropped["x"],dropped["y"])
        self.command("B 272 1");time.sleep(.15)
        self.move(self.position[0]+12,self.position[1],duration=.15)
        time.sleep(.15)
        moving=self.target(panel="crypto",pair="BTC/USD",header=True,fraction=.08)
        self.move(self.position[0]+528-moving["left"],self.position[1]+180-moving["top"],duration=1.1)
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
                           BARTJIK_DEMO="1",SHOWCASE_ICONS=str(ROOT/"docs/icons"),SHOWCASE_MONITOR=monitor)
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
                        time.sleep(2)
                        raw=directory/"capture.mp4"
                        recorder=subprocess.Popen(["wf-recorder","-D","--no-dmabuf","-o",monitor,"-r","60","-c","libx264rgb","-p","preset=ultrafast","-p","crf=0","-x","bgr0","-f",str(raw)],env=env,stdout=capture_log,stderr=capture_log)
                        time.sleep(.3)
                        if recorder.poll() is not None:raise ValueError("Recorder failed to start")
                        director.started=time.monotonic();director.run(SOURCE)
                        recorder.send_signal(signal.SIGINT);recorder.wait(timeout=15);recorder=None
                        shared.checked(["ffmpeg","-v","error","-i",str(raw),"-an","-vf","fps=60","-c:v","libx264","-preset","slow","-crf","17","-pix_fmt","yuv420p","-movflags","+faststart",str(output)])
                        probe=json.loads(shared.checked(["ffprobe","-v","error","-show_streams","-show_format","-of","json",str(output)]))
                        stream=probe["streams"][0]
                        if (stream["width"],stream["height"],stream["r_frame_rate"])!=(1200,720,"60/1"):
                            raise ValueError("Unexpected capture dimensions")
                        receipt={"tool":"bartjik","dimensions":[1200,720],"fps":60,"demoFixturesOnly":True,
                            "input":"private Wayland compositor seat; no QtTest or comparison renderer", "scale":SCALE,
                            "duration":probe["format"]["duration"],"sha256":hashlib.sha256(output.read_bytes()).hexdigest(),
                            "sourceCommit":shared.checked(["git","-C",str(ROOT),"rev-parse","HEAD"]),
                            "directorSha256":hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
                            "stageSha256":hashlib.sha256((shared.HERE/"ShowcaseStage.qml").read_bytes()).hexdigest(),
                            "chapters":director.chapters,"evidence":director.events}
                        output.with_suffix(".json").write_text(json.dumps(receipt,indent=2)+"\n")
                        print("Recording: "+str(output),flush=True)
                finally:
                    if recorder and recorder.poll() is None:recorder.send_signal(signal.SIGINT);recorder.wait(timeout=10)
                    if director:director.pointer.stdin.close();director.pointer.wait(timeout=5)
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
    args=parser.parse_args()
    if args.plan:
        print(json.dumps({"dimensions":[1200,720],"fps":60,"features":FEATURES,"comparison":False},indent=2));return
    if not args.output:parser.error("recording requires OUTPUT.mp4")
    output=args.output.expanduser().absolute()
    if output.suffix!=".mp4" or any(p.exists() for p in (output,output.with_suffix(".json"),output.with_suffix(".review"))):
        parser.error("choose a new .mp4 path; existing media and evidence are never overwritten")
    output.parent.mkdir(parents=True,exist_ok=True)
    record(output)


if __name__=="__main__":main()
