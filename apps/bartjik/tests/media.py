"""Synthetic Wayland media parts, ticker, album hover and secondary-output checks."""

# Test-only comparison oracle; production launches the Rust binary.
import sys as _sys
from pathlib import Path as _Path
_sys.path.insert(0, str(_Path(__file__).resolve().parent / "reference"))
_sys.path.insert(0, str(_Path(__file__).resolve().parent / "reference/qml"))
import json, os, shutil, subprocess, sys, tempfile, time
from pathlib import Path
from render import wait_for
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from bartjik import ipc


def main():
    assert os.environ.get("DBUS_SESSION_BUS_ADDRESS", "").startswith("unix:path=/tmp/")
    destination = Path(sys.argv[1]); destination.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="bartjik-media-") as td:
        temp=Path(td);shutil.copytree(ROOT/"qml",temp/"qml")
        p=temp/"qml/Part.qml"
        p.write_text(p.read_text().replace("    id: part", "    id: part\n    FontMetrics {id:testMetrics;font:labelText.font}\n    function mediaTestState(){return {kind:kind,width:width,textWidth:labelText.implicitWidth,natural:testMetrics.advanceWidth(label),x:labelText.x,ticking:ticking};}"))
        p=temp/"qml/Bar.qml";s=p.read_text().replace("import QtQuick", "import QtQuick\nimport QtTest",1)
        s=s.replace("    id: panel", """    id: panel
    TestEvent {id:input}
    function mediaParts(item) {
        let result=[];
        for(const child of item.children || []) {
            if(child.kind!==undefined && child.visible && child.moduleConfig?.provider==='playerctl')result.push(child);
            result=result.concat(mediaParts(child));
        }
        return result;
    }
    function mediaTest(req) {
        const island=groups.children.find(i=>i.islandId==='media');
        const parts=mediaParts(contentItem);
        if(req.click) {const part=parts.find(p=>p.kind===req.click);input.mouseClick(part,part.width/2,part.height/2,Qt.LeftButton,Qt.NoModifier,0);}
        if(req.middle) {
            let target=island,x=2,y=island.height/2;
            const part=parts.find(p=>p.kind===req.middle);
            if(part){target=part;x=part.width/2;y=part.height/2;}
            else if(req.middle==='right')x=island.width-2;
            else if(req.middle==='top'){x=island.width/2;y=2;}
            else if(req.middle==='bottom'){x=island.width/2;y=island.height-2;}
            else if(req.middle==='gap' || req.middle==='separator'){
                const left=parts.find(p=>p.kind===(req.middle==='gap'?'icon':'artist'));
                const right=parts.find(p=>p.kind===(req.middle==='gap'?'artist':'track'));
                const a=left.mapToItem(island,left.width,0),b=right.mapToItem(island,0,0);
                x=(a.x+b.x)/2;
            }
            input.mouseClick(target,x,y,Qt.MiddleButton,Qt.NoModifier,0);
        }
        if(req.hover!==undefined && island)input.mouseMove(island,req.hover?2:-500,req.hover?5:-500,0,Qt.NoButton,Qt.NoModifier);
        return {parts:parts.map(p=>p.mediaTestState())};
    }""")
        p.write_text(s)
        p=temp/"qml/shell.qml";s=p.read_text().replace("    id: root", "    id: root\n    property var testActions:[]\n    property bool hideTestPlayer:false\n    property var testMedia:null")
        s=s.replace("if(msg.kind==='pointer'){root.pointer=msg.pointer;root.pointerUpdated=Date.now();}", "if(msg.kind==='pointer'){}")
        s=s.replace("if(demo)return;", "if(demo){testActions=testActions.concat([{action:action,context:context}]);return;}")
        s=s.replace("const m=bar.moduleConfigs[id];if(!m)return [];", "const m=bar.moduleConfigs[id];if(!m || (m.provider==='playerctl' && hideTestPlayer))return [];if(m.provider==='playerctl' && testMedia)return [testMedia];")
        s=s.replace("case 'status':break;", """case 'mediaTest':
            if(req.track!==undefined){
                const b=bars.find(b=>b.id==='workspace-bar');
                const row=JSON.parse(JSON.stringify(items(b,'music')[0]));
                row.track.value=req.track;row.artist.value=req.artist;row.media.title=req.track;row.media.artist=req.artist;
                testMedia=row;
            }
            if(req.disappear)hideTestPlayer=true;
            return Object.assign({ok:true,actions:testActions},panels.instances.find(p=>p.bar.screen===screenInfo[0].name && p.bar.id==='workspace-bar').mediaTest(req));
        case 'status':break;""")
        p.write_text(s)
        p=temp/'qml/MediaDetails.qml'
        s=p.read_text().replace('import QtQuick\n','import QtQuick\nimport QtTest\n',1)
        s=s.replace('    id:details', '''    id:details
    TestEvent {id:middleInput}
    function testMiddle(x,y,showLyrics){
        lyricsShown=showLyrics;
        middleInput.mouseClick(details,x/320*details.width,y/360*details.height,Qt.MiddleButton,Qt.NoModifier,0);
    }''')
        p.write_text(s)
        p=temp/'qml/DetailPanel.qml'
        p.write_text(p.read_text().replace('    id:popup',
            '    id:popup\n    function testMiddle(x,y,lyrics){pinned=true;hide.stop();mediaDetails.testMiddle(x,y,lyrics);}'))
        p=temp/'qml/shell.qml'
        p.write_text(p.read_text().replace("case 'status':break;",
            "case 'middlePanel':detailPanel.testMiddle(req.x,req.y,req.lyrics);return {ok:true};\n        case 'status':break;"))
        cfg=json.loads((ROOT/"examples/comparison.json").read_text())
        # Live monitors are used only as surfaces; all contents and actions are synthetic.
        cfg["modules"]["music"]["track"]["style"]["maxTextWidth"]=230
        cfg["layout"]["centre_width"]="100%"
        cfg["bars"]=cfg["bars"][:1]
        for bar in cfg["bars"]:
            bar["autoHide"]=False
            bar["monitors"]="all"
        path=temp/"config.json";path.write_text(json.dumps(cfg))
        os.environ.update(BARTJIK_CONFIG=str(path),BARTJIK_QML=str(temp/"qml"))
        with (destination/"media.log").open("w") as log:
            process=subprocess.Popen([str(ROOT/"bin/bartjik"),"--demo"],stdout=log,stderr=log)
            try:
                wait_for(lambda:ipc({"op":"status"}).get("ready"))
                time.sleep(.6)
                bars=ipc({"op":"bars"})["bars"]
                for b in bars:
                    if b["key"].startswith("DP-2/"):assert [i["id"] for i in b["islands"]]==["workspaces"],b
                first=ipc({"op":"mediaTest"})
                assert {p["kind"] for p in first["parts"]}=={"icon","artist","track"},first
                assert next(p for p in first["parts"] if p["kind"]=="track")["ticking"],first
                assert not next(p for p in first["parts"] if p["kind"]=="artist")["ticking"],first
                wait_for(lambda:next(p for p in ipc({"op":"mediaTest"})["parts"] if p["kind"]=="track")["x"] < -2)
                # Successive tracks must remeasure independently of the previous elided width.
                for artist,title in [("An artist with a very long name "*4,"A long track title "*6),("Earth", "Cari"),("Another long artist "*6,"Another track "*8),("Short", "A medium length song")]:
                    ipc({"op":"mediaTest", "artist":artist, "track":title})
                    time.sleep(.15)
                    current=ipc({"op":"mediaTest"})["parts"]
                    for part in current:
                        if part["kind"] not in ("artist","track"):continue
                        limit=180 if part["kind"]=="artist" else 230
                        assert abs(part["width"]-max(20,min(part["natural"],limit)))<2,part
                        assert part["ticking"]==(part["natural"]>limit+1),part
                        if part["ticking"]:assert abs(part["x"])<1,part
                for part,action in [("icon","player.toggle"),("artist","player.artist"),("track","player.playlist")]:
                    before=len(ipc({"op":"mediaTest"})["actions"])
                    ipc({"op":"mediaTest","click":part})
                    wait_for(lambda:len(ipc({"op":"mediaTest"})["actions"])>before)
                    events=ipc({"op":"mediaTest"})["actions"]
                    assert events[-1]["action"]==action and events[-1]["context"]["part"]==part,events
                    assert events[-1]["context"]["monitor"]==ipc({"op":"monitors"})["monitors"][0]["name"],events
                # Real middle-button delivery must reach every label and uncovered area once.
                for area in ('icon','artist','track','left','right','top','bottom','gap','separator'):
                    before=len(ipc({'op':'mediaTest'})['actions'])
                    ipc({'op':'mediaTest','middle':area})
                    events=ipc({'op':'mediaTest'})['actions'][before:]
                    assert [e['action'] for e in events]==['player.toggle'],(area,events)
                # Reenter via island padding and wait for the configured hover dwell.
                ipc({"op":"mediaTest","hover":False})
                ipc({"op":"detailTest","module":"music","leave":True})
                wait_for(lambda:not ipc({"op":"detailState"})["panel"]["opened"])
                ipc({"op":"mediaTest","hover":True})
                time.sleep(.08)
                assert not ipc({"op":"detailState"})["panel"]["opened"]
                wait_for(lambda:ipc({"op":"detailState"})["panel"]["opened"])
                wait_for(lambda:ipc({"op":"detailState"})["panel"]["artworkReady"])
                for lyrics in (False,True):
                    for x,y in ((160,100),(10,310),(106,328),(160,328),(214,328),(290,328),(2,2),(318,358)):
                        before=len(ipc({'op':'mediaTest'})['actions'])
                        ipc({'op':'middlePanel','x':x,'y':y,'lyrics':lyrics})
                        events=ipc({'op':'mediaTest'})['actions'][before:]
                        assert [e['action'] for e in events]==['player.toggle'],(lyrics,x,y,events,ipc({'op':'detailState'})['panel'])
                shot=destination/"album-panel.png";ipc({"op":"captureDetail","path":str(shot)})
                wait_for(lambda:shot.exists() and shot.read_bytes().endswith(b"IEND\xaeB`\x82"))
                for b in bars:
                    if b["key"].endswith("workspace-bar"):
                        shot=destination/(b["key"].replace("/","-")+".png")
                        ipc({"op":"capture","bar":b["key"],"path":str(shot)})
                        wait_for(lambda:shot.exists() and shot.read_bytes().endswith(b"IEND\xaeB`\x82"))
                ipc({"op":"mediaTest","disappear":True})
                wait_for(lambda:not ipc({"op":"detailState"})["panel"]["opened"])
                assert not ipc({"op":"mediaTest"})["parts"]
            finally:
                process.terminate();process.wait(timeout=5)
        errors=[line for line in (destination/"media.log").read_text().splitlines() if any(s in line for s in ("WARN scene","ERROR","ReferenceError","TypeError","Binding loop"))]
        assert not errors,errors
    print("Media: real independent clicks, overflow ticker, 300ms artwork hover, primary-only islands and disappearance passed")

if __name__=="__main__":main()
