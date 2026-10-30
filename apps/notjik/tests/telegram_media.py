"""Private-bus, synthetic-only photo/voice UI acceptance. Never signs into Telegram.

dbus-run-session --config-file tests/bus.conf -- python3 -B tests/telegram_media.py /tmp/notjik-media-preview
"""
import json
import os
from pathlib import Path
import shutil
import socketserver
import subprocess
import sys
import tempfile
import threading
import time

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tests/reference"))
from notjik import ipc
from integration import wait_for
from native_fixture import copy_runtime, copy_file


def main():
    if not os.environ.get("DBUS_SESSION_BUS_ADDRESS", "").startswith("unix:path=/tmp/"):
        raise RuntimeError("A private D-Bus session is required")
    output = Path(sys.argv[1]).resolve()
    output.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="notjik-media-") as temp:
        root = Path(temp)
        copy_runtime(root)
        os.environ["NOTJIK_CONFIG"] = str(root / "config.json")
        os.environ["NOTJIK_STATE_DIR"] = str(root / "state")
        os.environ["WAYLAND_DISPLAY"] = str(Path(os.environ["XDG_RUNTIME_DIR"]) / os.environ["WAYLAND_DISPLAY"])
        os.environ["NOTJIK_TELEGRAM_SOCKET"] = str(root / "runtime/notjik-telegram/control.sock")
        (root / "runtime/notjik-telegram").mkdir(parents=True, mode=0o700)
        os.chmod(root / "runtime", 0o700)
        # Synthetic input is fixed by the native helper; never open a microphone.
        recorder = root / "qml/TelegramRecorder.qml"
        recorder.write_text(recorder.read_text().replace('"record"]', '"record", "--synthetic"]'))
        os.environ["XDG_RUNTIME_DIR"] = str(root / "runtime")
        requests = []
        voice_result = {"ok":True}
        reply_result = {"ok":True}
        class Handler(socketserver.StreamRequestHandler):
            def handle(self):
                requests.append(json.loads(self.rfile.readline()))
                result = voice_result if requests[-1].get("action") == "voiceReply" else reply_result if requests[-1].get("action") == "reply" else {"ok":True}
                self.wfile.write(json.dumps(result).encode()+b"\n")
        server = socketserver.UnixStreamServer(str(root / "runtime/notjik-telegram/control.sock"), Handler)
        threading.Thread(target=server.serve_forever, daemon=True).start()
        (root / "config.json").write_text(json.dumps({"theme":"rose-punk", "placement":{"monitors":[1], "edge":"b", "position":"c"},
            "grouping":{"enabled":False}, "pauseOnHover":False, "stack":{"animationMs":0}}))
        # Test-only inspection hooks stay outside the shipped application.
        shell = root / "qml/shell.qml"
        code = shell.read_text().replace(
            'readonly property string focusedAppId: ToplevelManager.activeToplevel?.appId || ""',
            'readonly property string focusedAppId: ""').replace('Variants {\n        model:', 'Variants {\n        id: panels\n        model:')
        code = code.replace('case "status": break;', '''case "testMedia": {
            const panel = panels.instances[0];
            const floating = root.isFloating(request.id);
            const floatingPanel = floatingPanels.instances.find(p => p.noticeId === request.id);
            const held = [panel.testCard(request.id), ...floatingPanels.instances.map(p => p.testCard(request.id))].find(c => c?.dragging);
            const card = held || (floating ? floatingPanel?.testCard(request.id) : panel.testCard(request.id));
            const item = card?.testDetail;
            if (request.hover !== undefined && card) card.testHover(request.hover);
            if (request.gesture && card) card.gesture(request.gesture, request.dx ?? -160, request.dy ?? -80);
            if (request.draft !== undefined && item) item.testDraft(request.draft);
            if (request.key && card) card.testKey(request.key);
            if (request.capacity) root.config = Object.assign({}, root.config, {maxQueued:request.capacity});
            if (request.close && card) { card.testClose(); return {ok:true}; }
            if (request.remaining !== undefined) root.find(request.id).remaining = request.remaining;
            if (request.floatCapture && card) card.grabToImage(result=>result.saveToFile(request.floatCapture));
            if (request.stopRecording) telegramRecording.stop();
            if (request.button && item) {card.testHover(true);item.testButton(request.button);}
            if (request.text !== undefined && item) item.testReply(request.text);
            if (request.seek !== undefined) telegramPlayback.player.position = request.seek;
            if (request.lock !== undefined) root.lockState = request.lock ? "locked" : "unlocked";
            if (request.expireLease) root.telegramBridgeUntil = Date.now() - 1;
            if (request.capture) panel.capture(request.capture);
            if (request.archive && item) item.historyMode = true;
            const position = root.floatingPositions[request.id];
            return {ok:true, revealed:card?.controlsRevealed || false, recording:{phase:telegramRecording.phase, source:telegramRecording.source, token:telegramRecording.token,
                duration:telegramRecording.duration, error:telegramRecording.error, owner:telegramRecording.noticeId}, pinned:root.isFloating(request.id), dragging:root.draggingId === request.id,
                ready:root.draggingId === request.id, floating:!!floatingPanel?.placed && !!floatingPanel?.visible && !root.locked,
                moves:floatingPanel?.testSystemMoves || 0, natural:card?.implicitHeight, height:card?.height, actionHeight:card?.testActionHeight, hint:card?.testHint,
                screens:Quickshell.screens.map(s=>({name:s.name,x:s.x,y:s.y,width:s.width,height:s.height})),
                point:card?.gripPoint ? card.gripPoint() : null, monitor:position?.screen || "", x:position?.x, y:position?.y,
                playButton:item?.testPlay || {}, replying:item?.replying || false, draft:item?.testDraftText || "", editing:item?.testEditing || false, buttons:item?.testButtons ? item.testButtons() : [], grip:!!card,
                loaded:!!item && (!floating || !!floatingPanel?.placed), photo:item ? item.testPhotoStatus : -1,
                visuals:item?.testVisuals ? item.testVisuals() : [],
                interactive:item ? item.interactive : false, feedback:item ? item.feedback : "", transcript:item ? item.demoTranscript || item.mediaData.transcript || "" : "",
                active:telegramPlayback.noticeId, playing:telegramPlayback.playing,
                position:telegramPlayback.player.position, duration:telegramPlayback.player.duration,
                rate:telegramPlayback.player.playbackRate, error:telegramPlayback.error};
        }
        case "status": break;''')
        shell.write_text(code)
        panel = root / "qml/NotificationPanel.qml"
        code = panel.read_text().replace('id: panel', '''id: panel
    function testCard(id) {
        for (let i=0; i<cardItems.count; ++i) {
            const card=cardItems.itemAt(i).testCard;
            if (card.notice.id === id) return card;
        }
        return null;
    }
    function capture(path) { notificationScene.grabToImage(result=>result.saveToFile(path)); }
''', 1).replace('property var cachedNotice: null', 'property var cachedNotice: null\n                    property alias testCard: card')
        panel.write_text(code.replace('monitorName: panel.screen.name', 'testHost: panel\n                            monitorName: panel.screen.name'))
        card = root / "qml/NotificationCard.qml"
        card.write_text(card.read_text().replace('import QtQuick\n', 'import QtQuick\nimport QtTest\n', 1).replace('id: card', '''id: card
    TestEvent { id: input }
    property alias testDetail: detail.item
    property var testHost: null
    property point testStart: Qt.point(0,0)
    property var testWindow: null
    function gripPoint() {
        const point=card.mapToItem(testHost.contentItem, width/2, 18);
        return {x:testHost.screen.x+(testHost.margins?.left || 0)+point.x, y:testHost.screen.y+(testHost.margins?.top || 0)+point.y};
    }
    readonly property real testActionHeight: actionFooter.implicitHeight
    readonly property bool testHint: children.find(c=>c.objectName==='interaction-hint')?.visible || false
    function testHover(inside) { input.mouseMove(card, inside ? width/2 : -20, inside ? 18 : -20, 0, Qt.NoButton, Qt.NoModifier); }
    function testClose() { closeButton.clicked(); }
    function testKey(key) { input.keyClickChar(key, Qt.NoModifier, 0); }
    function gesture(action, dx, dy) {
        if (action === "press") {
            testWindow = testHost.contentItem;
            testStart = card.mapToItem(testWindow, width/2, 18);
            input.mousePress(testWindow, testStart.x, testStart.y, Qt.LeftButton, Qt.NoModifier, 0);
        }
        if (action === "move") {
            input.mouseMove(testWindow, testStart.x+dx/2, testStart.y+dy/2, 0, Qt.LeftButton, Qt.NoModifier);
            input.mouseMove(testWindow, testStart.x+dx, testStart.y+dy, 0, Qt.LeftButton, Qt.NoModifier);
        }
        if (action === "release") input.mouseRelease(testWindow, testStart.x+dx, testStart.y+dy, Qt.LeftButton, Qt.NoModifier, 0);
    }
''', 1))
        floating = root / "qml/FloatingPanel.qml"
        floating.write_text(floating.read_text().replace('id: panel', '''id: panel
    property int testSystemMoves: 0
    function testCard(id) { return panel.noticeId === id ? card : null; }
''', 1).replace('monitorName: panel.screen?.name', 'testHost: panel\n                monitorName: panel.screen?.name')
            .replace('onDragStarted: panel.startSystemMove()', 'onDragStarted: panel.testSystemMoves++'))
        detail = root / "qml/components/Telegram.qml"
        detail.write_text(detail.read_text().replace('id: detail', '''id: detail
    readonly property var testPlay: ({visible:play.visible,opacity:play.opacity,enabled:play.enabled})
    readonly property alias testPhotoStatus: photo.status
    readonly property alias testDraftText: reply.text
    readonly property alias testEditing: reply.activeFocus
    function testVisuals() {
        const found=[];
        function walk(item) {
            if (typeof item.ready === "boolean" && typeof item.format === "string" && item.source)
                found.push({ready:item.ready, failed:item.failed, frame:item.frame, position:item.position, active:item.active, format:item.format});
            for (const child of (item.children || [])) walk(child);
        }
        walk(detail); return found;
    }
    function testDraft(text) { reply.text=text; replying=true; }
    function testButtons() {
        const found=[];
        function walk(item) {
            if (["telegram-compose", "telegram-quote", "telegram-read", "telegram-open", "telegram-send"].includes(item.objectName) && item.visible && item.opacity > 0)
                found.push({name:item.objectName, width:item.width, implicit:item.implicitWidth, textWidth:item.contentItem.width,
                    icon:item.iconName, accessible:item.Accessible.name, status:item.contentItem.status, padding:[item.leftPadding,item.rightPadding]});
            for (const child of (item.children || [])) walk(child);
        }
        walk(detail); return found;
    }
    function testButton(name) {
        function find(item) {
            if (item.objectName === name) return item;
            for (const child of (item.children || [])) { const found=find(child); if(found) return found; }
            return null;
        }
        const button=find(detail);
        if (!button || !button.enabled) throw Error("Button unavailable: " + name);
        button.clicked();
    }
    function testReply(text) { reply.text=text; action("reply", reply.text); }
''', 1))
        playback = root / "qml/TelegramPlayback.qml"
        playback.write_text(playback.read_text().replace('AudioOutput {}', 'AudioOutput { muted: true }'))
        sound = root / "voice.ogg"
        subprocess.run(["ffmpeg", "-v", "error", "-f", "lavfi", "-i", "anullsrc=r=48000:cl=mono", "-t", "8", "-c:a", "libopus", str(sound)], check=True)
        with (output / "qml.log").open("w+") as log:
            process = subprocess.Popen(["quickshell", "--path", str(root / "qml"), "--no-color"], stdout=log, stderr=log)
            try:
                def call(op, **fields):
                    return ipc(root, {"op":op, **fields})
                def media(ident, **fields):
                    return call("testMedia", id=ident, **fields)
                wait_for(lambda: call("status")["lockState"] == "unlocked")
                # Decode every visual transport locally; no account connection is used.
                for kind, format, filename in (("sticker","png","demo-sticker.png"),
                        ("sticker","gif","demo-animated-sticker.gif"),
                        ("animation","gif","demo-animation.gif"),
                        ("animation","video","demo-animation.mp4"),
                        ("emoji","gif","demo-animated-sticker.gif")):
                    rich = call("send", notice={"app":"notjik demo", "id":"rich", "title":"Synthetic media", "body":"", "type":"telegram", "timeout":0,
                        "data":{"kind":kind,"format":format,"source":(ROOT/"examples"/filename).as_uri(),"width":128,"height":128,"chatId":"1","messageId":"1","demo":True}})["id"]
                    print("Checking visual", kind, format, filename, flush=True)
                    try:
                        wait_for(lambda: media(rich)["visuals"] and all(v["ready"] for v in media(rich)["visuals"]))
                    except AssertionError:
                        print("Visual state", media(rich)["visuals"], flush=True)
                        raise
                    if format in ("gif","video"):
                        first=media(rich)["visuals"][0]
                        wait_for(lambda: media(rich)["visuals"][0]["frame" if format=="gif" else "position"] != first["frame" if format=="gif" else "position"])
                        media(rich,lock=True)
                        wait_for(lambda: not media(rich)["visuals"] or not media(rich)["visuals"][0]["active"])
                        time.sleep(.15)
                        frozen=media(rich)["visuals"]
                        time.sleep(.15)
                        after=media(rich)["visuals"]
                        if frozen and after:
                            assert frozen[0]["frame"] == after[0]["frame"] and frozen[0]["position"] == after[0]["position"]
                        media(rich,lock=False)
                        wait_for(lambda: media(rich)["visuals"] and all(v["ready"] for v in media(rich)["visuals"]))
                    media(rich,capture=str(output/(kind+"-"+format+".png")))
                    wait_for(lambda: (output/(kind+"-"+format+".png")).exists())
                    call("dismiss",id=rich)
                custom=call("send",notice={"app":"notjik demo","id":"custom","title":"Inline emoji","body":"Hello 🙂 there","type":"telegram","timeout":0,
                    "data":{"kind":"text","source":"","chatId":"1","messageId":"1","demo":True,"parts":[{"kind":"text","text":"Hello "},
                        {"kind":"emoji","text":"🙂","source":(ROOT/"examples/demo-animation.gif").as_uri(),"format":"gif"},{"kind":"text","text":" there"}]}})["id"]
                wait_for(lambda: media(custom)["visuals"] and all(v["ready"] for v in media(custom)["visuals"]))
                media(custom,capture=str(output/"custom-emoji.png"))
                wait_for(lambda: (output/"custom-emoji.png").exists())
                call("dismiss",id=custom)
                ordinary = call("send", notice={"app":"Demo app", "id":"ordinary-actions", "title":"Backup ready", "body":"Choose an action", "timeout":0,
                    "actions":[{"label":"Inspect", "command":["true"]}]})["id"]
                wait_for(lambda:media(ordinary)["natural"] > 0)
                media(ordinary,hover=False)
                wait_for(lambda:media(ordinary)["actionHeight"] == 0 and media(ordinary)["hint"])
                media(ordinary,hover=True)
                wait_for(lambda:media(ordinary)["actionHeight"] > 20 and not media(ordinary)["hint"])
                media(ordinary,hover=False)
                wait_for(lambda:media(ordinary)["actionHeight"] == 0 and media(ordinary)["hint"])
                call("dismiss",id=ordinary)
                # Demo Telegram cards use a generic sender name; type still controls hover behavior.
                photo = call("send", notice={"app":"notjik demo", "id":"photo", "title":"Weekend plans", "body":"This is the spot for tomorrow.", "type":"telegram",
                    "timeout":0, "data":{"kind":"photo", "chatId":"123", "messageId":"456", "source":(ROOT / "examples/demo-artwork.png").as_uri()}})["id"]
                wait_for(lambda: media(photo)["photo"] == 1)
                media(photo, hover=False)
                wait_for(lambda:not media(photo)["revealed"])
                time.sleep(.2)
                hidden_height=media(photo)["natural"]
                assert media(photo)["buttons"]==[] and media(photo)["hint"]
                media(photo,capture=str(output/"photo-collapsed.png"))
                media(photo, hover=True)
                wait_for(lambda:media(photo)["revealed"] and len(media(photo)["buttons"])>=3)
                wait_for(lambda:media(photo)["natural"] > hidden_height + 20)
                media(photo, hover=False)
                wait_for(lambda:not media(photo)["revealed"] and not media(photo)["buttons"])
                wait_for(lambda:abs(media(photo)["natural"]-hidden_height)<1)
                media(photo, hover=True)
                wait_for(lambda: all(button["status"] == 1 for button in media(photo)["buttons"]))
                assert all(button["icon"] and button["accessible"] for button in media(photo)["buttons"]), media(photo)["buttons"]
                media(photo, capture=str(output / "photo.png"))
                wait_for(lambda: (output / "photo.png").exists())
                def drag(ident, dx=-160, dy=-80):
                    before = media(ident)
                    media(ident, gesture="press")
                    media(ident, gesture="move", dx=dx, dy=dy)
                    if not before["pinned"]:
                        wait_for(lambda: media(ident)["ready"])
                    media(ident, gesture="release", dx=dx, dy=dy)
                    wait_for(lambda: media(ident)["pinned"] and not media(ident)["dragging"] and media(ident)["floating"])
                    if before["pinned"]:
                        assert media(ident)["moves"] == before["moves"] + 1, "Detached drag did not request a native move"
                    time.sleep(0.25)
                # Initial detachment can cross monitors; subsequent dragging is
                # handed to the compositor (native geometry is tested in dragging.py).
                screens = media(photo)["screens"]
                if len(screens) > 1:
                    point = media(photo)["point"]
                    origin = next(s for s in screens if s["x"] <= point["x"] < s["x"]+s["width"])
                    destination = next(s for s in screens if s["name"] != origin["name"])
                    drag(photo, dx=destination["x"]+destination["width"]//2-point["x"],
                         dy=destination["y"]+destination["height"]//2-point["y"])
                    assert media(photo)["monitor"] == destination["name"]
                    print("PASS: detach onto another monitor", flush=True)
                else:
                    drag(photo)
                assert media(photo)["floating"]
                assert call("status")["visible"] == 0
                media(photo, remaining=150)
                time.sleep(0.4)
                assert call("status")["pinned"] == 1
                drag(photo, dx=80, dy=-40)
                media(photo, floatCapture=str(output / "photo-pinned.png"))
                wait_for(lambda: (output / "photo-pinned.png").exists())
                media(photo, close=True)
                wait_for(lambda: call("status")["queued"] == 0)
                assert call("status")["pinned"] == 0
                voice_notice = {"app":"Telegram via Notjik", "id":"voice", "title":"Maya", "body":"", "type":"telegram", "timeout":0,
                    "image":(ROOT / "examples/demo-artwork.png").as_uri(),
                    "data":{"kind":"voice", "chatId":"123", "messageId":"789", "source":sound.as_uri(), "duration":8, "waveform":[0, 3, 8, 15, 31, 22, 12, 4] * 12}}
                voice = call("send", notice=voice_notice)["id"]
                wait_for(lambda: media(voice)["loaded"])
                media(voice,hover=False)
                wait_for(lambda:not media(voice)["revealed"])
                assert media(voice)["playButton"] == {"visible":True,"opacity":1,"enabled":True}
                time.sleep(.2)
                media(voice,capture=str(output/"voice-idle.png"))
                assert not media(voice)["playing"]
                media(voice, button="telegram-play")
                wait_for(lambda: media(voice)["playing"] and media(voice)["duration"] >= 7900)
                wait_for(lambda: media(voice)["position"] > 100)
                for text in ("Hello", "Hello from Telegram", "Hello from Telegram, streamed into the card."):
                    call("send",notice={**voice_notice,"data":{**voice_notice["data"],"transcriptionState":"pending","transcript":text}})
                    wait_for(lambda: media(voice)["transcript"] == text)
                    assert media(voice)["playing"] and media(voice)["active"] == voice
                call("send",notice={**voice_notice,"data":{**voice_notice["data"],"transcriptionState":"done","transcript":"Hello from Telegram, streamed into the card."}})
                media(voice,capture=str(output/"streamed-transcript.png"))
                wait_for(lambda: (output/"streamed-transcript.png").exists())
                media(voice, button="telegram-play")
                assert not media(voice)["playing"]
                media(voice, seek=3000)
                assert abs(media(voice)["position"] - 3000) < 200
                media(voice, button="telegram-speed")
                assert media(voice)["rate"] == 1.25
                # Updating a card must preserve its player and loaded component.
                call("send", notice={**voice_notice, "body":""})
                assert media(voice)["active"] == voice
                media(voice, capture=str(output / "voice.png"))
                wait_for(lambda: (output / "voice.png").exists())
                reply_result.update(ok=False,error="CHAT_WRITE_FORBIDDEN")
                media(voice,draft="Keep my failed reply")
                media(voice,text="Keep my failed reply")
                wait_for(lambda:media(voice)["feedback"]=="CHAT_WRITE_FORBIDDEN")
                assert media(voice)["draft"]=="Keep my failed reply" and media(voice)["replying"]
                reply_result.clear();reply_result.update(ok=True)
                media(voice, text="Synthetic reply; literal $(nothing)\nsecond line")
                wait_for(lambda: requests and requests[-1].get("text")=="Synthetic reply; literal $(nothing)\nsecond line")
                assert requests[-1] == {"action":"reply", "chatId":"123", "messageId":"789", "text":"Synthetic reply; literal $(nothing)\nsecond line", "quote":False}
                wait_for(lambda:not any(n["id"]==voice for n in call("list")["notices"]))
                voice=call("send",notice=voice_notice)["id"]
                wait_for(lambda:media(voice)["loaded"])
                media(voice,button="telegram-quote")
                media(voice,capture=str(output/"quote-reply.png"))
                media(voice,text="Quoted synthetic reply")
                wait_for(lambda: requests[-1].get("text")=="Quoted synthetic reply")
                assert requests[-1] == {"action":"reply", "chatId":"123", "messageId":"789", "text":"Quoted synthetic reply", "quote":True}
                wait_for(lambda:not any(n["id"]==voice for n in call("list")["notices"]))
                demo_reply=call("send",notice={**voice_notice,"id":"demo-text-reply","data":{**voice_notice["data"],"demo":True}})["id"]
                wait_for(lambda:media(demo_reply)["loaded"])
                count=len(requests)
                media(demo_reply,text="Demo reply")
                wait_for(lambda:not any(n["id"]==demo_reply for n in call("list")["notices"]))
                assert len(requests)==count, "Demo sent a real bridge request"
                voice=call("send",notice=voice_notice)["id"]
                wait_for(lambda:media(voice)["loaded"])
                media(voice, button="telegram-play")
                wait_for(lambda: media(voice)["playing"])
                media(voice, draft="Unsent draft stays with the voice note")
                before_drag = media(voice)["position"]
                drag(voice)
                after_drag = media(voice)
                assert after_drag["playing"] and after_drag["position"] >= before_drag
                assert after_drag["draft"] == "Unsent draft stays with the voice note"
                media(voice, floatCapture=str(output / "voice-pinned.png"))
                wait_for(lambda: (output / "voice-pinned.png").exists())
                # Named updates stay in the detached surface, preserving playback.
                call("send", notice={**voice_notice, "body":"Updated while pinned"})
                assert media(voice)["pinned"] and media(voice)["playing"]
                media(voice, button="telegram-compose")  # Close then reopen the carried draft.
                media(voice, button="telegram-compose")
                wait_for(lambda: media(voice)["editing"])
                before_typing = media(voice)["draft"]
                media(voice, key="x")
                assert len(media(voice)["draft"]) == len(before_typing) + 1
                media(voice, lock=True)
                assert not media(voice)["playing"]
                assert not media(voice)["floating"]
                media(voice, lock=False)
                wait_for(lambda: media(voice)["floating"])
                assert not media(voice)["playing"]
                wait_for(lambda: media(voice)["loaded"])
                # The microphone helper is replaced with a generated, silent input.
                before_recording = len(requests)
                media(voice,draft="Text draft before recording")
                assert media(voice)["replying"]
                media(voice, button="telegram-record")
                assert not media(voice)["replying"]
                assert media(voice)["draft"]=="Text draft before recording"
                wait_for(lambda: media(voice)["recording"]["duration"] >= 500)
                assert not media(voice)["playing"]
                drag(voice, dx=50, dy=-30)
                assert media(voice)["recording"]["phase"] == "recording"
                media(voice, stopRecording=True)
                wait_for(lambda: media(voice)["recording"]["phase"] == "ready")
                wait_for(lambda: media(voice)["height"] >= media(voice)["natural"] - 1)
                recorded = media(voice)["recording"]
                from urllib.parse import urlparse, unquote
                draft_path = Path(unquote(urlparse(recorded["source"]).path))
                assert draft_path.exists() and draft_path.stat().st_mode & 0o777 == 0o600
                media(voice, floatCapture=str(output / "voice-reply.png"))
                wait_for(lambda: (output / "voice-reply.png").exists())
                assert len(requests) == before_recording, "Recording sent a message without Send"
                voice_result.update(ok=False, error="VOICE_MESSAGES_FORBIDDEN")
                media(voice, button="telegram-record-send")
                wait_for(lambda: media(voice)["recording"]["phase"] == "ready" and media(voice)["recording"]["error"])
                assert draft_path.exists(), "Failed send deleted the draft"
                voice_result.clear(); voice_result["ok"] = True
                media(voice, button="telegram-record-send")
                wait_for(lambda: media(voice)["recording"]["phase"] == "idle")
                assert requests[-1] == {"action":"voiceReply", "chatId":"123", "messageId":"789", "token":recorded["token"]}
                assert not draft_path.exists()
                media(voice, button="telegram-record")
                wait_for(lambda: media(voice)["recording"]["duration"] >= 500)
                media(voice, lock=True)
                wait_for(lambda: media(voice)["recording"]["phase"] == "ready")
                media(voice, lock=False)
                wait_for(lambda: media(voice)["loaded"])
                assert not media(voice)["playing"]
                media(voice, button="telegram-record-discard")
                wait_for(lambda: media(voice)["recording"]["phase"] == "idle")
                media(voice, archive=True)
                assert not media(voice)["interactive"]
                call("dismiss", id="all")
                # Recording belongs to the root, so destroying the popup on
                # initial detachment cannot stop capture or lose the draft.
                voice = call("send", notice=voice_notice)["id"]
                wait_for(lambda: media(voice)["loaded"])
                media(voice, button="telegram-record")
                wait_for(lambda: media(voice)["recording"]["duration"] >= 500)
                drag(voice)
                assert media(voice)["recording"]["phase"] == "recording"
                media(voice, stopRecording=True)
                wait_for(lambda: media(voice)["recording"]["phase"] == "ready")
                closing_draft = Path(unquote(urlparse(media(voice)["recording"]["source"]).path))
                media(voice, close=True)
                wait_for(lambda: media("")["recording"]["phase"] == "idle")
                assert not closing_draft.exists(), "Closing notification retained the draft"
                # Ordinary cards support the same gesture, even a quick release.
                # Rich-message Open uses the same workspace focus and dismissal as native cards.
                opened = call("send", notice=voice_notice)["id"]
                wait_for(lambda: media(opened)["loaded"])
                media(opened, button="telegram-open")
                wait_for(lambda: opened not in [n["id"] for n in call("list")["notices"]])
                assert requests[-1]["action"] == "open"
                # Demo account actions stay simulated; explicit recording uses a self-only send route.
                demo_voice = call("send", notice={**voice_notice, "id":"demo-voice",
                    "data":{**voice_notice["data"], "demo":True}})["id"]
                wait_for(lambda: media(demo_voice)["loaded"])
                before = len(requests)
                for button in ("telegram-read", "telegram-open", "telegram-transcribe"):
                    media(demo_voice, button=button)
                    assert "Demo only" in media(demo_voice)["feedback"]
                assert "synthetic demo transcript" in media(demo_voice)["transcript"]
                assert media(demo_voice)["recording"]["phase"] == "idle"
                assert len(requests) == before
                media(demo_voice, button="telegram-record")
                wait_for(lambda: media(demo_voice)["recording"]["duration"] >= 500)
                media(demo_voice, button="telegram-record-send")
                wait_for(lambda: media(demo_voice)["recording"]["phase"] == "idle")
                assert requests[-1]["action"] == "demoVoiceReply"
                before_reset = len(requests)
                media(demo_voice, button="telegram-record")
                wait_for(lambda: media(demo_voice)["recording"]["duration"] >= 500)
                media(demo_voice, button="telegram-record-discard")
                wait_for(lambda: media(demo_voice)["recording"]["phase"] == "idle")
                assert len(requests) == before_reset, "Reset sent a recording"
                call("dismiss", id=demo_voice)
                plain = call("send", notice={"title":"Ordinary notification", "body":"Drag works here too", "timeout":0})["id"]
                wait_for(lambda: media(plain)["grip"])
                media(plain, gesture="press")
                media(plain, gesture="move")
                media(plain, gesture="release")  # A quick release still finishes the drag.
                wait_for(lambda: media(plain)["pinned"])
                media(plain, remaining=100, capacity=1)
                try:
                    call("send", notice={"title":"Queue pressure", "timeout":100})
                except ValueError as error:
                    assert "Queue is full" in str(error)
                else:
                    raise AssertionError("A pinned notification was evicted")
                assert call("status")["pinned"] == 1
                media(plain, capacity=100)
                call("dismiss", id=plain)
                assert call("status")["pinned"] == 0
                # Bridge leases suppress only native Telegram chat notifications.
                def notify(app, category):
                    subprocess.run(["busctl","--user","call","org.freedesktop.Notifications","/org/freedesktop/Notifications",
                        "org.freedesktop.Notifications","Notify","susssasa{sv}i",app,"0","","Synthetic native","Body","0","1","category","s",category,"0"], check=True, capture_output=True)
                call("telegramBridge", active=True)
                notify("Telegram Desktop", "im.received")
                assert not call("list")["notices"]
                notify("Telegram Desktop", "call.incoming")
                assert len(call("list")["notices"]) == 1
                call("dismiss", id="all")
                media("", expireLease=True)
                wait_for(lambda: not call("status")["telegramBridgeActive"])
                notify("Telegram Desktop", "im.received")
                assert len(call("list")["notices"]) == 1
                print("PASS: WebP/TGS-derived GIF/MP4 rendering; inline animated emoji; lock pauses animation; streaming transcript preserves playback; photo rendering; Opus playback/pause/seek/speed; no autoplay; update continuity; reply IPC; lock stop; archive controls; native fallback; photo/voice/plain drag; re-drag; pinned timeout; draft/playback continuity; locked pin concealment; record/send/reset; failed-send draft retention; recording drag continuity; lock stops microphone")
            finally:
                process.terminate()
                process.wait(timeout=5)
                server.shutdown()
                server.server_close()
                log.seek(0)
                text = log.read()
                if any(word in text for word in ("TypeError", "ReferenceError", "Cannot assign", "component failed", "is not a type", "Binding loop", "Detected function", "Unable to assign")):
                    raise AssertionError(text)
        # Trim transparent space from the synthetic-only card captures.
        from PIL import Image
        for name in ("photo.png", "voice.png"):
            with Image.open(output / name) as image:
                bounds = image.getbbox()
                image.crop((0, max(0, bounds[1]-12), image.width, min(image.height, bounds[3]+12))).save(output / name)


if __name__ == "__main__":
    main()
