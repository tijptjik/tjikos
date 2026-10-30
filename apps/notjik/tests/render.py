"""Capture real QML cards using grabToImage; no desktop content is captured.

dbus-run-session --config-file tests/bus.conf -- python3 -B tests/render.py /tmp/notjik-previews
The capture IPC is injected only into a temporary copy of the application.
Add --groups --hover for real-pointer group acceptance on Hyprland; requires
ydotoold and YDOTOOL_SOCKET pointing to its socket. Cursor position is restored.
"""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time
from PIL import Image

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tests/reference"))
from notjik import ipc
from demo import fixtures
from integration import wait_for
from native_fixture import copy_runtime, copy_file


def readable_image(path):
    try:
        with Image.open(path) as image:
            image.verify()
        return True
    except (OSError, SyntaxError):
        return False


def read_motion(root):
    count = ipc(root, {"op":"stopMotion"})["count"]
    return [frame for offset in range(0, count, 5)
            for frame in ipc(root, {"op":"motionFrames", "offset":offset})["frames"]]


def motion_preview(root, destination):
    """Capture only synthetic QML cards at render-frame cadence."""
    from PIL import ImageDraw
    for scenario in ("burst", "midflight"):
        ipc(root, {"op":"dismiss", "id":"all"})
        time.sleep(.4)
        directory = destination / scenario
        directory.mkdir(exist_ok=True)
        ipc(root, {"op":"startMotion", "path":str(directory)})
        ids = []
        for index in range(4):
            if scenario == "midflight" and index == 3:
                time.sleep(.14)
                speed_before = ipc(root, {"op":"metrics"})["metrics"]["flowSpeed"]
                assert speed_before > 0, speed_before
            ids.append(ipc(root, {"op":"send", "notice":{"app":"notjik demo", "id":scenario+str(index),
                "title":"Photo album " + str(index+1), "body":"New photos have been added to your collection.", "timeout":0}})["id"])
            if scenario == "midflight" and index == 3:
                speed_after = ipc(root, {"op":"metrics"})["metrics"]["flowSpeed"]
                assert speed_after > speed_before * .6, (speed_before, speed_after)
        time.sleep(.9)
        recorded = read_motion(root)
        (directory / "metrics.json").write_text(json.dumps(recorded))
        frames, timestamps = [], []
        for entry in recorded:
            if not entry.get("image") or not readable_image(entry["image"]):
                continue
            with Image.open(entry["image"]) as image:
                crop = image.crop((0, image.height-600, image.width, image.height)).convert("RGBA")
                canvas = Image.new("RGBA", (image.width, 625), "#121018")
                canvas.alpha_composite(crop, (0,25))
                ImageDraw.Draw(canvas).text((10,7), str(entry["now"]-recorded[0]["now"])+" ms", fill="white")
                frames.append(canvas.convert("RGB"))
                timestamps.append(entry["now"])
        durations = [max(10,b-a) for a,b in zip(timestamps,timestamps[1:])] + [500]
        frames[0].save(destination / (scenario+".gif"), save_all=True, append_images=frames[1:], duration=durations, loop=0)
        selected = [min(range(len(frames)), key=lambda i: abs(timestamps[i]-timestamps[0]-target)) for target in (40,80,120,160,210,270,350,500)]
        sheet = Image.new("RGB", (frames[0].width*4, frames[0].height*2), "#121018")
        for index, chosen in enumerate(selected):
            sheet.paste(frames[chosen], ((index%4)*frames[0].width,(index//4)*frames[0].height))
        sheet.save(destination / (scenario+".png"))
        print(destination / (scenario+".gif"), flush=True)


def group_preview(root, destination):
    wait_for(lambda: ipc(root, {"op":"status"})["lockState"] == "unlocked")
    def geometry():
        return ipc(root, {"op":"metrics"})["metrics"]["cards"][0]["group"]
    def settle(count):
        wait_for(lambda: len(ipc(root, {"op":"metrics"})["metrics"]["cards"]) == 1 and
                 geometry()["requested"] == count and not geometry()["moving"] and
                 abs(geometry()["opacity"] - (.75 if count > 1 else 0)) < .001)
        return geometry()
    def send(index):
        return ipc(root, {"op":"send", "notice":{"app":"notjik demo", "id":"notch-"+str(index), "group":"notches",
            "title":"Weekend plans", "body":"See you at the café tomorrow.", "timeout":0}})["id"]
    ids = []
    initial_height = None
    for count in range(1,7):
        ids.append(send(count))
        group = settle(count)
        wait_for(lambda: ipc(root, {"op":"metrics"})["metrics"]["cards"][0]["opacity"] == 1)
        card = ipc(root, {"op":"metrics"})["metrics"]["cards"][0]
        assert group["count"] == (0 if count == 1 else min(5,count)), group
        assert str(count) in group["label"], group
        if count > 1: assert group["visible"] and group["indicatorOpacity"] == 1, group
        if initial_height is None:
            initial_height = card["height"]
        assert card["height"] == initial_height, card
        marks = group["notches"]
        assert len(marks) == group["count"] and all(abs(n["height"]-6) < .01 for n in marks), group
        assert all(abs(n["x"] + n["width"] - group["width"]) < .01 and n["width"] >= 3 for n in marks), marks
        assert abs(group["x"] + group["width"] - group["cardWidth"]) < .01, group
        gaps = [b["y"]-a["y"] for a,b in zip(marks,marks[1:])]
        assert all(abs(gap - 10) < .01 for gap in gaps), marks
        if marks:
            assert abs((marks[0]["y"] + marks[-1]["y"] + 6) / 2 - group["notchAreaHeight"] / 2) < .01, group
        if count in (1,2,3,4,5,6):
            path = destination / ("group-"+str(count)+".png")
            path.unlink(missing_ok=True)
            ipc(root, {"op":"capture", "path":str(path)})
            wait_for(lambda: readable_image(path))
            with Image.open(path) as shot:
                bounds = shot.getbbox()
                shot.crop((0, max(0,bounds[1]-12), shot.width, min(shot.height,bounds[3]+12))).save(path)
            print(path, flush=True)
    capped = geometry()["slices"]
    for count in (5,4,3):
        ipc(root, {"op":"dismiss", "id":ids.pop()})
        group = settle(count)
        if count >= 5:
            assert group["slices"] == capped, group
    def animate(name, action, count):
        before = geometry()
        directory = destination / name
        directory.mkdir(exist_ok=True)
        ipc(root, {"op":"startMotion", "path":str(directory)})
        action()
        after = settle(count)
        time.sleep(.1)
        recorded = read_motion(root)
        moving = [f["cards"][0]["group"] for f in recorded if f["cards"] and f["cards"][0]["group"]["moving"]]
        assert moving, "No intermediate notch frames"
        for frame in moving:
            assert len(frame["slices"]) == 60 and abs(frame["opacity"] - .75) < .001, frame
        if count == 2:
            assert any(g["slices"][20]["y"] < before["slices"][20]["y"]-.2 and
                       g["slices"][39]["y"] > before["slices"][39]["y"]+.2 for g in moving), moving
        else:
            assert any(g["slices"][20]["y"] > before["slices"][20]["y"]+.2 and
                       g["slices"][39]["y"] < before["slices"][39]["y"]-.2 for g in moving), moving
        frames, times = [], []
        for record in recorded:
            if not record.get("image") or not readable_image(record["image"]):
                continue
            with Image.open(record["image"]) as shot:
                bounds = shot.getbbox()
                crop = shot.crop((0,bounds[1]-4,80,bounds[3]+4)).convert("RGBA")
                background = Image.new("RGBA", crop.size, "#121018")
                background.alpha_composite(crop)
                frames.append(background.convert("RGB").resize((crop.width*4,crop.height*4), Image.Resampling.NEAREST))
                times.append(record["now"])
        durations = [max(10,b-a) for a,b in zip(times,times[1:])] + [600]
        frames[0].save(destination/(name+".gif"), save_all=True, append_images=frames[1:], duration=durations, loop=0)
        sheet = Image.new("RGB", (frames[0].width*5,frames[0].height), "#121018")
        for i, fraction in enumerate((0,.15,.35,.65,1)):
            chosen = round((len(frames)-1)*fraction)
            sheet.paste(frames[chosen],(i*frames[0].width,0))
        sheet.save(destination/(name+".png"))
        assert len(after["notches"]) == count, after
    animate("merge-3-to-2", lambda: ipc(root, {"op":"dismiss", "id":ids.pop()}), 2)
    animate("split-2-to-3", lambda: ids.append(send(3)), 3)
    # Reverse an in-flight morph, then settle with no leftover fragments.
    ipc(root, {"op":"dismiss", "id":ids.pop()})
    time.sleep(.04)
    ids.append(send(3))
    assert len(settle(3)["notches"]) == 3
    # Reduced-motion configuration makes count changes immediate.
    config_path = root / "config.json"
    config = json.loads(config_path.read_text())
    config["stack"] = {"animationMs":0}
    config_path.write_text(json.dumps(config))
    wait_for(lambda: ipc(root, {"op":"metrics"})["metrics"]["animationMs"] == 0)
    ids.append(send(4))
    assert not geometry()["moving"] and len(settle(4)["notches"]) == 4
    if "--hover" in sys.argv[2:]:
        previous = json.loads(subprocess.check_output(["hyprctl", "cursorpos", "-j"], text=True))
        try:
            p = ipc(root, {"op":"groupHoverPoint"})["point"]
            subprocess.run(["hyprctl", "dispatch", "hl.dsp.cursor.move({x=" + str(round(p["globalX"])) + ",y=" + str(round(p["globalY"])) + "})"], check=True, capture_output=True)
            subprocess.run(["ydotool", "mousemove", "--", "1", "0"], check=True, capture_output=True)
            time.sleep(.7)
            assert not ipc(root, {"op":"status"})["drawerOpen"]
            print("PASS: hovering the grouped card leaves history closed")
        finally:
            subprocess.run(["hyprctl", "dispatch", "hl.dsp.cursor.move({x=" + str(previous["x"]) + ",y=" + str(previous["y"]) + "})"], check=True, capture_output=True)
    assert not ipc(root, {"op":"status"})["drawerOpen"]
    print("PASS: symmetric 2-to-3 split and 3-to-2 merge; interrupted morph; five-notch cap; disabled motion; stable card height; history stays closed")


def main():
    if not os.environ.get("DBUS_SESSION_BUS_ADDRESS", "").startswith("unix:path=/tmp/"):
        raise RuntimeError("A private dbus-run-session is required")
    destination = Path(sys.argv[1]).resolve()
    destination.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="notjik-render-") as temp:
        root = Path(temp)
        copy_runtime(root)
        if "--covers" in sys.argv[2:]:
            # Keep geometry screenshots deterministic while testing the shared cover layout.
            logic = root / "qml/Logic.js"
            logic.write_text(logic.read_text().replace("Math.floor(Math.random() * robotFiles.length)", "0"))
        shell = root / "qml/shell.qml"
        code = shell.read_text().replace('Variants {\n        model:', 'Variants {\n        id: panels\n        model:')
        if "--covers" in sys.argv[2:]:
            fixture = (ROOT / "examples/demo-artwork.png").as_uri()
            code = code.replace('root.avatarCache = response.avatars || {byChat: {}, byTitle: {}};',
                                'root.avatarCache = {byChat: {}, byTitle: {"Cached native avatar": ' + json.dumps(fixture) + '}};')
        code = code.replace('case "status": break;', 'case "advanceActions": panels.instances[0].advanceActions(request.direction); return {ok:true};\n        case "metrics": return {ok:true, metrics:panels.instances[0].metrics()};\n        case "capture": panels.instances[0].capture(request.path); return {ok:true};\n        case "status": break;')
        code = code.replace('HistoryDrawer { controller: root }', 'HistoryDrawer { id: historyDrawer; controller: root }')
        code = code.replace('case "capture":', 'case "groupHoverPoint": return {ok:true, point:panels.instances[0].groupHoverPoint()};\n        case "startMotion": panels.instances[0].startMotion(request.path || ""); return {ok:true};\n        case "stopMotion": return {ok:true, count:panels.instances[0].stopMotion()};\n        case "motionFrames": return {ok:true, frames:panels.instances[0].motionFrames.slice(request.offset, request.offset + 5)};\n        case "capture":')
        code = code.replace('case "capture":', 'case "expandHistory": for (const group of historyGroups) expandedGroups = Object.assign({}, expandedGroups, {[group.key]:true}); break;\n        case "captureHistory": historyDrawer.capture(request.path); return {ok:true};\n        case "capture":')
        code = code.replace('case "status": break;', 'case "revealControls": panels.instances[0].revealControls(request.revealed !== false); return {ok:true};\n        case "status": break;')
        shell.write_text(code)
        history = root / "qml/HistoryDrawer.qml"
        history.write_text(history.read_text().replace('id: drawer', 'id: drawer\n    function capture(path) { drawerContent.grabToImage(result => result.saveToFile(path)); }', 1)
                           .replace('WlrKeyboardFocus.OnDemand', 'WlrKeyboardFocus.None')
                           .replace('else if (drawer.visible && drawer.hadWindowFocus)', 'else if (false)'))
        notches = root / "qml/GroupNotches.qml"
        notches.write_text(notches.read_text().replace("id: notches", """id: notches
    function geometry() {
        const parts = Array.from({length:slices.count}, (_, i) => {
            const piece = slices.itemAt(i);
            return {x:piece.x, y:piece.y, height:piece.height, width:piece.width, moving:piece.testMoving};
        });
        const marks = [];
        if (opacity > .001) for (const piece of parts) {
            const last = marks[marks.length - 1];
            if (last && Math.abs(last.y + last.height - piece.y) < .01) last.height += piece.height;
            else marks.push({x:piece.x, y:piece.y, width:piece.width, height:piece.height});
        }
        return {notchAreaHeight:height, count:grouped ? cappedCount : 0, requested:count, opacity:opacity, moving:parts.some(p => p.moving), slices:parts, notches:marks};
    }
""", 1).replace("id: slice\n", "id: slice\n            readonly property bool testMoving: slide.running || resize.running\n", 1))
        card = root / "qml/NotificationCard.qml"
        if "--groups" in sys.argv[2:]:
            import re
            card.write_text(re.sub(r"readonly property bool controlsRevealed:.*?(?=    readonly property bool hasInteractions)",
                                   "readonly property bool controlsRevealed: false\n", card.read_text(), count=1, flags=re.S))
        card.write_text(card.read_text().replace("readonly property bool controlsRevealed:", "property bool controlsRevealed:").replace("id: card", 'id: card\n    function advanceActions(direction) { if (direction > 0 && nextActions.enabled) nextActions.clicked(); else if (direction < 0 && previousActions.enabled) previousActions.clicked(); }\n    readonly property int artworkStatus: coverArtwork ? coverImage.status : artworkImage.status\n    readonly property url artworkSource: coverArtwork ? coverImage.source : artworkImage.source\n    function footerGeometry() { return {imageCover:card.imageCover, avatarCover:card.avatarCover, border:card.border.width, cover:card.coverArtwork, coverWidth:coverRail.width, coverHeight:coverRail.height, imageHeight:coverImage.height, contentLeft:content.x, contentTop:content.y, contentHeight:content.implicitHeight, detailHeight:detail.height, detailVisible:detail.visible, visible:actionFooter.visible, y:actionFooter.y, height:actionFooter.height, bottom:actionFooter.y+actionFooter.height, cardHeight:height, mainBottom:content.y+content.implicitHeight}; }\n    function actionGeometry() {\n        if (!pages.count) return [];\n        const row = pages.itemAt(actions.page).children[0];\n        return [{x:row.x, width:row.width, available:actionViewport.width, page:actions.page, count:pages.count, previous:previousActions.enabled, next:nextActions.enabled, stripX:actionStrip.x,\n            buttons:row.children.filter(button => button.modelData !== undefined).map(button => ({label:button.text, index:button.modelData.actionIndex, x:button.x, width:button.width, height:button.height, textHeight:button.contentItem.implicitHeight, truncated:button.contentItem.truncated}))}];\n    }\n', 1))
        card.write_text(card.read_text().replace("id: card", """id: card
    function groupGeometry() {
        return Object.assign(groupNotches.geometry(), {label:groupIndicator.Accessible.name, visible:groupIndicator.visible, indicatorOpacity:groupIndicator.opacity,
            enabled:groupIndicator.enabled, dragging:card.dragging, drawer:card.controller.drawerOpen, cardHovered:card.hovered, x:groupIndicator.x, width:groupIndicator.width, cardWidth:width, height:height});
    }
    function groupHoverPoint() { return card.mapToItem(card.Window.window.contentItem, card.width / 2, card.height / 2); }
""", 1))
        if "--height-limit" in sys.argv[2:]:
            card_fixture = root / "qml/NotificationCard.qml"
            card_fixture.write_text(card_fixture.read_text().replace("readonly property bool controlsRevealed:", "property bool controlsRevealed:"))
        panel = root / "qml/NotificationPanel.qml"
        panel.write_text(panel.read_text().replace('id: panel', 'id: panel\n    function capture(path) { notificationScene.grabToImage(result => result.saveToFile(path)); }', 1))
        panel_code = panel.read_text().replace("property var cachedNotice: null", "property var cachedNotice: null\n                    property alias renderedCard: card")
        # Record frame times in QML: polling begins after the burst has been sent.
        panel_code = panel_code.replace("property real reveal: 0", """property real reveal: 0
                        property real firstRevealAt: 0
                        property real firstSpaceAt: 0
                        onRevealChanged: { if (reveal > .01 && !firstRevealAt) firstRevealAt = Date.now(); }
                        onExtentChanged: { if (extent > .001 && !firstSpaceAt) firstSpaceAt = Date.now(); }""")
        panel_code = panel_code.replace("id: panel\n", """id: panel
    function groupHoverPoint() {
        const card = cardItems.itemAt(0).renderedCard;
        const p = card.groupHoverPoint();
        return {x:p.x,y:p.y,globalX:panel.screen.x+panel.margins.left+p.x,globalY:panel.screen.y+panel.margins.top+p.y};
    }
    property bool recordingMotion: false
    property string motionPath: ""
    property var motionFrames: []
    property bool capturePending: false
    function startMotion(path) { motionFrames = []; motionPath = path; recordingMotion = true; }
    function stopMotion() { recordingMotion = false; return motionFrames.length; }
    FrameAnimation {
        running: panel.recordingMotion
        onTriggered: {
            const metrics = panel.metrics();
            const sample = {now:metrics.now, flowSpeed:metrics.flowSpeed, layoutHeight:metrics.layoutHeight,
                cards:metrics.cards.map(c => ({id:c.id, y:c.y, height:c.height, fullHeight:c.fullHeight, extent:c.extent,
                    opacity:c.opacity, started:c.started, entranceAt:c.entranceAt, group:c.group}))};
            if (panel.motionPath && !panel.capturePending) {
                sample.image = panel.motionPath + "/" + panel.motionFrames.length + ".png";
                panel.capturePending = true;
                notificationScene.grabToImage(result => {
                    result.saveToFile(sample.image);
                    panel.capturePending = false;
                });
            }
            panel.motionFrames.push(sample);
        }
    }
    function advanceActions(direction) { for (let i = 0; i < cardItems.count; ++i) cardItems.itemAt(i).renderedCard.advanceActions(direction); }
    function revealControls(revealed) { for (let i = 0; i < cardItems.count; ++i) cardItems.itemAt(i).renderedCard.controlsRevealed = revealed; }
    function metrics() {
        const result = [];
        for (let i = 0; i < cardItems.count; ++i) {
            const slot = cardItems.itemAt(i), card = slot.renderedCard;
            const point = card.mapToItem(viewport, 0, 0);
            const slotPoint = slot.mapToItem(viewport, 0, 0);
            const visibleTop = slot.clip ? Math.max(point.y,slotPoint.y) : point.y;
            const visibleBottom = slot.clip ? Math.min(point.y+card.height*card.scale,slotPoint.y+slot.height) : point.y+card.height*card.scale;
            result.push({structuredReady:card.structuredReady, presentation:card.presentation, id:card.notice.id, lifeKey:slot.noticeKey, title:card.notice.title, y:visibleTop, x:point.x, width:card.width*card.scale,
                height:Math.max(0,visibleBottom-visibleTop), fullHeight:card.height, admissionHeight:card.admissionHeight, extent:slot.extent, slotClip:slot.clip, cardZ:slot.z, scale:card.scale, behind:card.parent === exitLayer, layerZ:card.parent === exitLayer ? exitLayer.z : viewport.z, entranceAt:slot.entranceAt, started:slot.started, firstRevealAt:slot.firstRevealAt, firstSpaceAt:slot.firstSpaceAt, urgency:card.notice.urgency, titleInk:card.urgencyInk.title, bodyInk:card.urgencyInk.body, opacity:card.opacity*slot.opacity, retiring:slot.retiring, artwork:card.artwork, artworkReady:card.artworkStatus === Image.Ready, artworkSource:card.artworkSource.toString(), actions:card.actionGeometry(), group:card.groupGeometry(), footer:card.footerGeometry()});
        }
        return {handlerCount:(controller.config.handlers || []).length, cards:result, heightBlocked:panel.heightBlocked, flowSpeed:panel.motionSpeed, now:Date.now(), height:panel.height, layoutHeight:panel.layoutHeight, exitCount:exitLayer.children.length, contentHeight:viewport.contentHeight, animationMs:panel.animationMs};
    }
""", 1)
        start = panel_code.index("    visible: controller.ready")
        end = panel_code.index("    anchors {", start)
        # Handler cards must also be measured while the real panel starts hidden.
        if "--handlers" not in sys.argv[2:]:
            panel_code = panel_code[:start] + "    visible: controller.ready\n" + panel_code[end:]
        # Capture surfaces must not intercept desktop clicks or lose fixtures to them.
        mask_start = panel_code.index("    mask: Region {")
        mask_end = panel_code.index("    // A separate, input-transparent surface", mask_start)
        if "--hover" not in sys.argv[2:]:
            panel_code = panel_code[:mask_start] + "    mask: Region { width: 0; height: 0 }\n\n" + panel_code[mask_end:]
        if "--height-limit" in sys.argv[2:]:
            panel_code = panel_code.replace('readonly property real layoutHeight: Math.max(1, (screen.height - 2 * placement.margin) * (controller.config.stack?.heightPercent || 75) / 100)', 'readonly property real layoutHeight: cardItems.count && cardItems.itemAt(0) ? cardItems.itemAt(0).admissionHeight * 3 + (theme.gap || 0) * 2 + 1 : 350')
        panel.write_text(panel_code)
        os.environ["NOTJIK_CONFIG"] = str(root / "config.json")
        os.environ["NOTJIK_STATE_DIR"] = str(root / "state")
        config = {"theme": "rose-punk", "placement": {"monitors": [1], "edge": "b", "position": "c"}, "pauseOnHover": False}
        if "--telegram-formatting" in sys.argv[2:]:
            config["grouping"] = {"enabled": False}
        if "--hover" in sys.argv[2:]:
            config["placement"].update({"edge":"t", "position":"l", "offsetX":100, "offsetY":100})
        (root / "config.json").write_text(json.dumps(config))
        with (destination / "render.log").open("w") as log:
            process = subprocess.Popen(["quickshell", "--path", str(root / "qml"), "--no-color"], stdout=log, stderr=log, start_new_session=True)
            try:
                wait_for(lambda: ipc(root, {"op": "status"})["ok"])
                if "--telegram-formatting" in sys.argv[2:]:
                    from telegram_bridge import describe
                    text = "Bold · italic · underline · strike\nCode: a  b <literal>\nNested bold + strike · 🙂 custom emoji"
                    entities = []
                    for label, kind in (("Bold", "Bold"), ("italic", "Italic"), ("underline", "Underline"),
                                        ("strike", "Strikethrough"), ("a  b <literal>", "Code"),
                                        ("bold + strike", "Bold"), ("bold + strike", "Strikethrough")):
                        start = text.index(label)
                        entities.append({"offset": len(text[:start].encode("utf-16-le")) // 2,
                            "length": len(label), "type": {"@type": "textEntityType" + kind}})
                    for custom in (False, True):
                        current = list(entities)
                        if custom:
                            current.append({"offset": len(text[:text.index("🙂")].encode("utf-16-le")) // 2,
                                "length": 2, "type": {"@type":"textEntityTypeCustomEmoji", "custom_emoji_id":"123"}})
                        body, data, _ = describe({"content":{"@type":"messageText", "text":{"text":text, "entities":current}}})
                        data.update(chatId="1", messageId="1", demo=True)
                        ipc(root, {"op":"send", "notice":{"app":"synthetic formatting", "type":"telegram",
                            "title":"Telegram styles" + (" with custom emoji" if custom else ""),
                            "body":body, "data":data, "timeout":0}})
                    wait_for(lambda: len(ipc(root, {"op":"metrics"})["metrics"]["cards"]) == 2
                        and all(c["opacity"] == 1 and c["extent"] == 1 for c in ipc(root, {"op":"metrics"})["metrics"]["cards"]))
                    image = destination / "telegram-formatting.png"
                    ipc(root, {"op":"capture", "path":str(image)})
                    wait_for(lambda: readable_image(image))
                    errors = (destination / "render.log").read_text()
                    assert not any(word in errors for word in ("ReferenceError", "TypeError", "Unable to assign", "Binding loop", "ERROR:")), errors
                    print("PASS: Telegram formatting and custom emoji cards rendered: " + str(image))
                    return
                if "--action-collapse" in sys.argv[2:]:
                    ipc(root, {"op":"send", "notice":{"app":"notjik demo", "title":"Stable artwork", "body":"", "timeout":0,
                        "actions":[{"label":"Open", "command":["true"]}]}})
                    wait_for(lambda: len(ipc(root, {"op":"metrics"})["metrics"]["cards"]) == 1)
                    def footer():
                        return ipc(root, {"op":"metrics"})["metrics"]["cards"][0]["footer"]
                    for cycle in range(3):
                        ipc(root, {"op":"revealControls"})
                        time.sleep(.3)
                        expanded = footer()
                        ipc(root, {"op":"revealControls", "revealed":False})
                        samples = []
                        until = time.monotonic() + .3
                        while time.monotonic() < until:
                            samples.append(footer())
                            time.sleep(.01)
                        assert samples[-1]["cardHeight"] < expanded["cardHeight"], samples
                        assert all(abs(f["imageHeight"] - expanded["imageHeight"]) < .01 for f in samples), samples
                        assert all(abs(f["contentTop"] - expanded["contentTop"]) < .01 for f in samples), samples
                        assert all(b["cardHeight"] <= a["cardHeight"] + .01 for a, b in zip(samples, samples[1:])), samples
                    print("PASS: three action collapses retain image scale and shrink monotonically")
                    return
                if "--height-limit" in sys.argv[2:]:
                    for duration in (0, 220):
                        config["stack"] = {"animationMs": duration}
                        config["grouping"] = {"enabled": False}
                        (root / "config.json").write_text(json.dumps(config))
                        wait_for(lambda: ipc(root, {"op":"metrics"})["metrics"]["animationMs"] == duration)
                        fixture_ids = []
                        for i in range(4):
                            fixture_ids.append(ipc(root, {"op":"send", "notice":{"id":f"fit-{i}", "title":"Height fixture", "body":"Synthetic hover controls\nSpace is reserved before arrival", "timeout":0, "actions":[{"label":"Open", "command":["true"]}]}})["id"])
                        def settled():
                            m = ipc(root, {"op":"metrics"})["metrics"]
                            return m if len(m["cards"]) == 4 and sum(c["extent"] == 1 for c in m["cards"]) == 3 else None
                        m = wait_for(settled)
                        assert not m["cards"][3]["started"] and m["cards"][3]["opacity"] == 0, m
                        assert m["contentHeight"] <= m["layoutHeight"] and m["heightBlocked"], m
                        time.sleep(.4)
                        assert not ipc(root, {"op":"metrics"})["metrics"]["cards"][3]["started"]
                        assert sum(c["fullHeight"] for c in m["cards"]) < m["layoutHeight"], m
                        ipc(root, {"op":"revealControls"})
                        time.sleep(.3)
                        expanded = ipc(root, {"op":"metrics"})["metrics"]
                        assert expanded["cards"][0]["fullHeight"] > m["cards"][0]["fullHeight"], expanded
                        assert expanded["contentHeight"] <= expanded["layoutHeight"], expanded
                        assert not expanded["cards"][3]["started"], expanded
                        ipc(root, {"op":"dismiss", "id":fixture_ids[0]})
                        wait_for(lambda: len((m := ipc(root, {"op":"metrics"})["metrics"])["cards"]) == 3 and all(c["extent"] == 1 for c in m["cards"]))
                        wait_for(lambda: not ipc(root, {"op":"metrics"})["metrics"]["heightBlocked"])
                        ipc(root, {"op":"dismiss", "id":"all"})
                        wait_for(lambda: not ipc(root, {"op":"metrics"})["metrics"]["cards"])
                    print("PASS: height overflow stays hidden and enters after dismissal, with and without animation")
                    return
                if "--groups" in sys.argv[2:]:
                    group_preview(root, destination)
                    return
                if "--motion" in sys.argv[2:]:
                    motion_preview(root, destination)
                    return
                if "--handlers" in sys.argv[2:]:
                    from test_handlers import RULE
                    BODY = "🎬 Demo (2026)\n\n1080p · WEB-DL · H 264\nGrabbed by Radarr · Example.index"
                    config["handlers"] = [RULE]
                    config["actions"] = {"blacklist":["telegram"]}
                    config["theme"] = "rose-punk"
                    (root / "config.json").write_text(json.dumps(config))
                    wait_for(lambda: ipc(root, {"op":"metrics"})["metrics"]["handlerCount"] == 1 and ipc(root, {"op":"status"})["theme"] == "rose-punk")
                    for transport in ("native", "bridge", "bridge-blocked", "unmatched"):
                        ipc(root, {"op":"dismiss", "id":"all"})
                        wait_for(lambda: not ipc(root, {"op":"metrics"})["metrics"]["cards"])
                        notice = {"title":"StarProwlarr", "app":"Telegram Desktop", "body":BODY, "timeout":0,
                                  "image":(ROOT / "examples/demo-artwork.png").as_uri()}
                        if transport.startswith("bridge"):
                            notice.update(app="Telegram via Notjik", desktopEntry="telegram" if transport == "bridge-blocked" else "", type="telegram", data={"kind":"text", "source":"", "status":"", "chatId":"123", "messageId":"456", "parts":[], "demo":True})
                        elif transport == "unmatched":
                            notice["body"] = "A new message format stays readable."
                        notice["id"] = "handler-update"
                        notice["group"] = "handler-group"
                        ipc(root, {"op":"send", "notice":notice})
                        ipc(root, {"op":"send", "notice":dict(notice, id="handler-second")})
                        time.sleep(.4)
                        wait_for(lambda: len(ipc(root, {"op":"metrics"})["metrics"]["cards"]) == 1 and ipc(root, {"op":"metrics"})["metrics"]["cards"][0]["extent"] == 1)
                        metric = ipc(root, {"op":"metrics"})["metrics"]["cards"][0]
                        print(f"{transport}: {metric['fullHeight']}px", flush=True)
                        assert metric["structuredReady"] == (transport != "unmatched"), metric
                        if transport != "unmatched":
                            assert metric["presentation"]["data"]["codec"] == "H 264", metric
                            rail = metric["footer"]
                            assert not rail["avatarCover"], metric
                            assert abs(rail["coverHeight"] + 2 * rail["border"] - rail["cardHeight"]) < 1, metric
                        if transport.startswith("bridge"):
                            assert not metric["footer"]["detailVisible"], metric
                            assert metric["fullHeight"] < 240, metric
                        if transport == "bridge":
                            ipc(root, {"op":"revealControls"})
                            time.sleep(.3)
                            expanded = ipc(root, {"op":"metrics"})["metrics"]["cards"][0]
                            assert expanded["footer"]["detailVisible"] and expanded["fullHeight"] > metric["fullHeight"], expanded
                            ipc(root, {"op":"revealControls", "revealed":False})
                            time.sleep(.3)
                            collapsed = ipc(root, {"op":"metrics"})["metrics"]["cards"][0]
                            assert not collapsed["footer"]["detailVisible"], collapsed
                            assert abs(collapsed["fullHeight"] - metric["fullHeight"]) < 1, collapsed
                        image = destination / ("handler-" + transport + ".png")
                        ipc(root, {"op":"capture", "path":str(image)})
                        wait_for(lambda: readable_image(image))
                    errors = (destination / "render.log").read_text()
                    assert not any(value in errors for value in ("TypeError:", "ReferenceError:", "Unable to assign")), errors
                    print("PASS: structured native/bridge cards and unmatched-text fallback")
                    return
                examples = [
                    {"title": "Connected to Taihingtjik", "body": "Wi-Fi · wlp11s0 · signal 79%", "icon": "network-wireless-symbolic"},
                    {"title": "Archive upload", "body": "Encrypted backup · 1.6 / 2.4 GiB", "type": "progress", "data": {"value": 67, "label": "285 KiB/s · about 3 minutes"}},
                    {"title": "Backup needs attention", "body": "Free some space to keep your files protected.", "type": "status", "urgency": "critical", "data": {"state": "Failed", "detail": "Your backup drive is full", "tone": "critical"}, "actions": [{"label": "Open backup ↗", "command": ["true"]}, {"label": "View details", "command": ["true"]}]},
                    {"title": "Next sync", "type": "timer", "data": {"seconds": 90, "label": "Waiting for the next run"}},
                ]
                for notice in examples:
                    ipc(root, {"op": "send", "notice": {"app": "notjik", "timeout": 0, "group": notice.get("type", "standard"), **notice}})
                wait_for(lambda: ipc(root, {"op": "status"})["lockState"] == "unlocked")
                for theme in ("rose-punk", "monochrome"):
                    config["theme"] = theme
                    (root / "config.json").write_text(json.dumps(config))
                    wait_for(lambda: ipc(root, {"op": "status"})["theme"] == theme)
                    wait_for(lambda: len(ipc(root, {"op":"metrics"})["metrics"]["cards"]) == 4 and all(c["opacity"] == 1 and c["extent"] == 1 for c in ipc(root, {"op":"metrics"})["metrics"]["cards"]))
                    metrics = ipc(root, {"op":"metrics"})["metrics"]
                    cards = metrics["cards"]
                    assert len(cards) == 4, metrics
                    assert all(card["artworkReady"] for card in cards), metrics
                    assert all(card["extent"] == 1 for card in cards), metrics
                    assert cards[2]["titleInk"] != cards[0]["titleInk"], metrics
                    assert cards[2]["bodyInk"] == cards[0]["titleInk"], metrics
                    assert "wifi-cover.png" in cards[0]["artworkSource"], metrics
                    for card in cards:
                        footer = card["footer"]
                        assert footer["visible"] == bool(card["actions"]), card
                        if footer["visible"]:
                            assert footer["y"] >= footer["mainBottom"] + 8, card
                            assert abs(footer["bottom"] - footer["cardHeight"] + 1) <= 1, card
                        for row in card["actions"]:
                            assert abs(row["x"]) <= 1, row
                    assert sum("/icons/robots/robot-" in card["artwork"] for card in cards) == 3, metrics
                    assert all(abs(card["opacity"] - 1) < 0.001 for card in cards), metrics
                    assert all(abs(card["width"] - 420) < 0.01 for card in cards), metrics
                    assert cards[0]["y"] >= -1 and cards[-1]["y"] + cards[-1]["height"] <= metrics["height"] + 1, metrics
                    for before, after in zip(cards, cards[1:]):
                        assert after["y"] >= before["y"] + before["height"] + 8, metrics
                    image = destination / (theme + ".png")
                    image.unlink(missing_ok=True)
                    ipc(root, {"op": "capture", "path": str(image)})
                    wait_for(lambda: readable_image(image))
                    with Image.open(image) as shot:
                        bounds = shot.getbbox()
                        shot.crop((0, max(0, bounds[1] - 16), shot.width, shot.height)).save(image)
                    print(image)
                if "--covers" in sys.argv[2:]:
                    for item in cards:
                        assert item["footer"]["cover"] and item["footer"]["coverWidth"] == 80, item
                        assert item["footer"]["contentLeft"] == 96, item
                    cases = [
                        ({"app":"Telegram Desktop", "title":"Cached native avatar", "body":"Full-size cached profile photo", "image":"file:///missing-native-thumbnail.jpg"}, True),
                        ({"title":"Everything is up to date"}, True),
                        ({"title":"Connected to Taihingtjik", "body":"Wi-Fi · wlp11s0 · signal 79%", "icon":"network-wireless-symbolic"}, True),
                        ({"title":"Tall robot", "body":"\n".join(["A long notification line"] * 10)}, True),
                        ({"title":"Sender image", "body":"Now playing · Evening sessions", "image":(ROOT / "examples/demo-artwork.png").as_uri()}, True),
                        ({"title":"Tall sender image", "body":"\n".join(["A longer image notification"] * 10), "image":(ROOT / "examples/demo-artwork.png").as_uri()}, True),
                        ({"title":"Short avatar", "type":"telegram", "body":"Hello", "image":(ROOT / "examples/demo-artwork.png").as_uri(), "data":{"kind":"text","demo":True,"chatId":"123","messageId":"456","source":""}}, True),
                        ({"title":"Tall avatar", "type":"telegram", "body":"A photo with a square avatar", "image":(ROOT / "examples/demo-artwork.png").as_uri(), "data":{"kind":"photo","demo":True,"chatId":"123","messageId":"457","source":(ROOT / "examples/demo-artwork.png").as_uri()}}, True),
                        ({"title":"Telegram avatar", "type":"telegram", "data":{"kind":"text", "chatId":"123", "messageId":"456", "source":""}}, True),
                    ]
                    for notice, cover in cases:
                        ipc(root, {"op":"dismiss", "id":"all"})
                        time.sleep(.4)
                        ipc(root, {"op":"send", "notice":{"app":"cover test", "timeout":0, **notice}})
                        def settled_cover():
                            rows = ipc(root, {"op":"metrics"})["metrics"]["cards"]
                            return rows if len(rows) == 1 and rows[0]["opacity"] == 1 and rows[0]["extent"] == 1 else None
                        try:
                            row = wait_for(settled_cover)[0]
                        except AssertionError:
                            print(notice["title"], ipc(root, {"op":"metrics"}), flush=True)
                            raise
                        assert row["footer"]["cover"] == cover, row
                        if cover:
                            f=row["footer"]
                            bottom=f["cardHeight"]-(f["height"] if f["visible"] else 0)-f["contentTop"]-f["contentHeight"]
                            assert abs(f["contentTop"]-bottom)<1,(notice["title"],f)
                        if notice["title"] in ("Everything is up to date","Connected to Taihingtjik"):
                            image=destination/("centered-title.png" if not notice.get("body") else "centered-body.png")
                            ipc(root,{"op":"capture","path":str(image)})
                            wait_for(lambda:readable_image(image))
                        if notice["title"] == "Cached native avatar":
                            assert row["artworkReady"] and row["artworkSource"].endswith("demo-artwork.png"), row
                            assert not row["footer"]["avatarCover"], row
                            image = destination / "cached-native-avatar.png"
                            ipc(root, {"op":"capture", "path":str(image)})
                            wait_for(lambda: readable_image(image))
                        if notice["title"] == "Sender image":
                            image=destination/"sender-image.png"
                            ipc(root,{"op":"capture","path":str(image)})
                            wait_for(lambda:readable_image(image))
                        if row["footer"]["imageCover"]:
                            assert row["footer"]["coverWidth"] == 80 and row["footer"]["contentLeft"] == 96, row
                            if row["footer"]["avatarCover"]:
                                assert row["footer"]["coverHeight"] == row["footer"]["coverWidth"], row
                            else:
                                assert abs(row["footer"]["coverHeight"] + 2*row["footer"]["border"] - row["footer"]["cardHeight"]) < 1, row
                            if notice["title"] == "Tall sender image": assert row["footer"]["coverHeight"] > 160, row
                        elif cover:
                            assert row["footer"]["coverHeight"] == max(0,row["fullHeight"]-2*row["footer"]["border"]),row
                        else:
                            assert row["footer"]["contentLeft"] == 16, row
                    print("PASS: 80px covers, full-height robots and Telegram cover rails, including the missing-avatar initials rail")
                    return
                # Incoming cards fade as they push; their full shape is never clipped by a slot.
                ipc(root, {"op":"dismiss", "id":"all"})
                time.sleep(.5)
                config["stack"] = {"animationMs":800}
                (root / "config.json").write_text(json.dumps(config))
                wait_for(lambda: ipc(root, {"op":"metrics"})["metrics"]["animationMs"] == 800)
                older = ipc(root, {"op":"send", "notice":{"id":"motion-old", "title":"Already visible", "timeout":0}})["id"]
                time.sleep(.9)
                older_start = next(c for c in ipc(root, {"op":"metrics"})["metrics"]["cards"] if c["id"] == older)["y"]
                older_motion = []
                newer = ipc(root, {"op":"send", "notice":{"id":"motion-new", "title":"Whole card entrance", "body":"The entire card fades in while the upward push settles.", "timeout":0}})["id"]
                def sample_motion():
                    samples = []
                    for _ in range(24):
                        metrics = ipc(root, {"op":"metrics"})["metrics"]
                        cards = metrics["cards"]
                        for card in cards:
                            card["sampleTime"] = metrics["now"]
                            assert not card["slotClip"], card
                            if card["opacity"] > .001:
                                assert abs(card["height"] - card["fullHeight"] * card["scale"]) < .01, card
                            if card["id"] == older:
                                assert card["scale"] == 1 and card["opacity"] == 1, card
                                older_motion.append(card["y"])
                            if card["id"] == newer:
                                samples.append(card)
                        time.sleep(.03)
                    return samples
                entering = sample_motion()
                assert any(0 < c["extent"] < 1 and c["opacity"] > .01 for c in entering), entering
                assert not any(c["extent"] > .01 and c["opacity"] == 0 for c in entering), entering
                assert any(0 < c["opacity"] < 1 and .97 <= c["scale"] < 1 for c in entering), entering
                wait_for(lambda: all(c["extent"] == 1 for c in ipc(root, {"op":"metrics"})["metrics"]["cards"]))
                settled = ipc(root, {"op":"metrics"})["metrics"]["cards"]
                old_final = next(c for c in settled if c["id"] == older)
                new_final = next(c for c in settled if c["id"] == newer)
                assert abs(older_start - old_final["y"] - new_final["height"] - 10) < 1, settled
                assert any(older_start > y > old_final["y"] for y in older_motion), older_motion
                # Strong ease-out: a quick initial push, a slow tail, and no rebound.
                assert all(0 <= c["extent"] <= 1 for c in entering), entering
                assert all(b <= a + .01 for a,b in zip(older_motion,older_motion[1:])), older_motion
                assert min(older_motion) >= old_final["y"] - .01, older_motion
                def motion_at(fraction):
                    return min(entering, key=lambda c: abs((c["sampleTime"] - c["entranceAt"]) / 800 - fraction))
                early_a, early_b = motion_at(.1), motion_at(.3)
                late_a, late_b = motion_at(.65), motion_at(.85)
                early_speed = (early_b["extent"] - early_a["extent"]) / max(1, early_b["sampleTime"] - early_a["sampleTime"])
                late_speed = (late_b["extent"] - late_a["extent"]) / max(1, late_b["sampleTime"] - late_a["sampleTime"])
                assert early_speed > 4 * late_speed and early_speed > 0, (early_speed, late_speed)
                assert .4 < early_b["extent"] < .95, early_b
                ipc(root, {"op":"dismiss", "id":newer})
                leaving = sample_motion()
                assert any(0 < c["opacity"] < 1 for c in leaving), leaving
                assert any(c["extent"] < 1 and c["opacity"] == 0 for c in leaving), leaving
                assert all(c["behind"] and c["layerZ"] == 0 for c in leaving), leaving
                centers = [c["y"] + c["height"]/2 for c in leaving]
                assert all(b >= a-.01 for a,b in zip(centers,centers[1:])), centers
                assert max(centers) > new_final["y"] + new_final["height"]/2 + 15, centers
                ipc(root, {"op":"dismiss", "id":"all"})
                time.sleep(.9)
                # A middle card falls behind the lower survivor, rather than painting over it.
                middle_stack = []
                for i in range(3):
                    middle_stack.append(ipc(root, {"op":"send", "notice":{"id":"depth-"+str(i), "title":"Depth card "+str(i+1), "body":"Synthetic falling-away preview", "timeout":0}})["id"])
                wait_for(lambda: len(ipc(root, {"op":"metrics"})["metrics"]["cards"]) == 3 and all(c["opacity"] == 1 for c in ipc(root, {"op":"metrics"})["metrics"]["cards"]))
                ipc(root, {"op":"dismiss", "id":middle_stack[1]})
                overlapping = False
                for _ in range(26):
                    metrics = ipc(root, {"op":"metrics"})["metrics"]
                    falling = next((c for c in metrics["cards"] if c["id"] == middle_stack[1]), None)
                    lower = next(c for c in metrics["cards"] if c["id"] == middle_stack[2])
                    assert lower["opacity"] == 1 and lower["scale"] == 1, lower
                    if falling and falling["opacity"] > .05:
                        assert falling["behind"] and falling["layerZ"] < lower["layerZ"], falling
                        assert falling["y"] + falling["height"] <= metrics["height"] + 1, falling
                        if falling["y"] + falling["height"] > lower["y"]:
                            if not overlapping:
                                ipc(root, {"op":"capture", "path":str(destination / "falling-behind.png")})
                            overlapping = True
                    time.sleep(.03)
                assert overlapping, "Departing card never fell behind the next card"
                wait_for(lambda: readable_image(destination / "falling-behind.png"))
                assert ipc(root, {"op":"metrics"})["metrics"]["exitCount"] == 0
                ipc(root, {"op":"dismiss", "id":"all"})
                time.sleep(.9)
                config["stack"] = {"animationMs":220}
                (root / "config.json").write_text(json.dumps(config))
                wait_for(lambda: ipc(root, {"op":"metrics"})["metrics"]["animationMs"] == 220)
                for notice in examples:
                    ipc(root, {"op":"send", "notice":{"app":"notjik", "timeout":0,"group":notice.get("type","standard"),**notice}})
                time.sleep(.5)
                # Back-to-back sends must visibly enter one at a time, not as a block.
                ipc(root, {"op":"dismiss", "id":"all"})
                time.sleep(.5)
                ipc(root, {"op":"startMotion"})
                burst = []
                for i in range(4):
                    burst.append(ipc(root, {"op":"send", "notice":{"id":"burst-"+str(i), "title":"Flow card "+str(i+1), "body":"One\nTwo\nThree\nFour\nFive" if i == 2 else "", "timeout":0}})["id"])
                initial = ipc(root, {"op":"metrics"})["metrics"]["cards"]
                assert any(not c["started"] for c in initial), initial
                first_visible = {}
                first_space = {}
                flow_speeds = []
                first_at_bottom = False
                fully_shown_before_push = False
                for _ in range(50):
                    metrics = ipc(root, {"op":"metrics"})["metrics"]
                    cards = metrics["cards"]
                    first = next(c for c in cards if c["id"] == burst[0])
                    second = next(c for c in cards if c["id"] == burst[1])
                    if first["opacity"] > .05 and all(c["extent"] == 0 for c in cards if c["id"] in burst[1:]):
                        # Even queued gaps must not lift the first card off the bottom.
                        unscaled_bottom = first["y"] + first["height"] + (first["fullHeight"] - first["height"])/2
                        assert abs(unscaled_bottom - metrics["layoutHeight"]) < .1, metrics
                        first_at_bottom = True
                    if 0 < second["extent"] and any(c["id"] == burst[-1] and not c["started"] for c in cards):
                        flow_speeds.append(metrics["flowSpeed"])
                    if second["extent"] > .01:
                        assert first["opacity"] == 1, first
                        fully_shown_before_push = True
                        assert second["opacity"] > .01, second
                    for card in cards:
                        if not card["started"]:
                            assert card["extent"] == 0 and card["opacity"] == 0, card
                        if card["id"] not in burst:
                            continue
                        if card["extent"] > .001:
                            first_space.setdefault(card["id"], card["firstSpaceAt"] / 1000)
                        if card["opacity"] > .01:
                            first_visible.setdefault(card["id"], card["firstRevealAt"] / 1000)
                            assert not card["slotClip"], card
                    time.sleep(.025)
                recorded = read_motion(root)
                for frame in recorded:
                    first = next((c for c in frame["cards"] if c["id"] == burst[0]), None)
                    later = [c for c in frame["cards"] if c["id"] in burst[1:]]
                    if first and first["opacity"] > .05 and all(c["extent"] == 0 for c in later):
                        bottom = first["y"] + (first["height"] + first["fullHeight"]) / 2
                        assert abs(bottom - frame["layoutHeight"]) < .1, frame
                        first_at_bottom = True
                assert first_at_bottom and fully_shown_before_push, (first_at_bottom, fully_shown_before_push)
                assert list(first_visible) == burst and list(first_space) == burst, (burst, first_visible, first_space)
                for times in (first_visible, first_space):
                    values = list(times.values())
                    assert all(b > a for a,b in zip(values,values[1:])), values
                starts = list(first_space.values())
                # The tall card takes more travel time; there is no fixed interval.
                assert starts[3] - starts[2] > (starts[2] - starts[1]) * 1.4, starts
                assert len(flow_speeds) >= 3 and min(flow_speeds[1:]) > max(flow_speeds) * .65, flow_speeds
                # Updating active IDs must not restart them; dismiss/re-send must create new lives.
                before = {c["id"]:c for c in ipc(root, {"op":"metrics"})["metrics"]["cards"] if not c["retiring"]}
                ipc(root, {"op":"send", "notice":{"id":"burst-0", "title":"Updated in place", "timeout":0}})
                current = next(c for c in ipc(root, {"op":"metrics"})["metrics"]["cards"] if c["id"] == burst[0])
                assert current["lifeKey"] == before[burst[0]]["lifeKey"] and current["entranceAt"] == before[burst[0]]["entranceAt"] and current["opacity"] == 1, current
                ipc(root, {"op":"dismiss", "id":"all"})
                for i in range(4):
                    ident = ipc(root, {"op":"send", "notice":{"id":"burst-"+str(i), "title":"Replayed card "+str(i+1), "timeout":0}})["id"]
                    assert ident == burst[i]
                replaced = ipc(root, {"op":"metrics"})["metrics"]["cards"]
                active = [c for c in replaced if not c["retiring"]]
                assert len(active) == 4, replaced
                assert all(c["lifeKey"] != before[c["id"]]["lifeKey"] and c["extent"] == 0 and c["opacity"] == 0 for c in active), replaced
                assert all(not c["title"].startswith("Replayed") for c in replaced if c["retiring"]), replaced
                seen = {}
                for _ in range(48):
                    cards = ipc(root, {"op":"metrics"})["metrics"]["cards"]
                    for c in cards:
                        if not c["retiring"] and c["opacity"] > .01:
                            seen.setdefault(c["id"], c["firstRevealAt"] / 1000)
                            assert not c["slotClip"], c
                    time.sleep(.025)
                assert list(seen) == burst, seen
                moments = list(seen.values())
                assert all(b > a for a,b in zip(moments,moments[1:])), seen
                # Append a different burst to a full stack: keep the surviving old card intact.
                additions = []
                for i in range(4):
                    additions.append(ipc(root, {"op":"send", "notice":{"id":"added-"+str(i), "title":"Added card "+str(i+1), "timeout":0}})["id"])
                for _ in range(45):
                    cards = ipc(root, {"op":"metrics"})["metrics"]["cards"]
                    survivor = next(c for c in cards if c["id"] == burst[-1] and not c["retiring"])
                    assert survivor["scale"] == 1 and survivor["opacity"] == 1, survivor
                    shown = [c for c in cards if c["opacity"] > .03 and not c["retiring"]]
                    for first, second in zip(shown,shown[1:]):
                        if second["y"] < first["y"] + first["height"] - 1:
                            assert second["extent"] < 1 and second["cardZ"] > first["cardZ"], shown
                    time.sleep(.025)
                final = [c for c in ipc(root, {"op":"metrics"})["metrics"]["cards"] if not c["retiring"]]
                assert [c["id"] for c in final] == [burst[-1]] + additions, final
                assert all(c["opacity"] == 1 and c["extent"] == 1 for c in final), final
                ipc(root, {"op":"dismiss", "id":"all"})
                time.sleep(.4)
                pending = []
                for i in range(3):
                    pending.append(ipc(root, {"op":"send", "notice":{"id":"cancel-"+str(i), "title":"Queued cancellation", "timeout":0}})["id"])
                queued = next(c for c in ipc(root, {"op":"metrics"})["metrics"]["cards"] if c["id"] == pending[-1])
                assert not queued["started"], queued
                ipc(root, {"op":"dismiss", "id":pending[-1]})
                time.sleep(.6)
                remaining = ipc(root, {"op":"metrics"})["metrics"]
                assert [c["id"] for c in remaining["cards"]] == pending[:2] and remaining["exitCount"] == 0, remaining
                config["stack"] = {"animationMs":0}
                (root / "config.json").write_text(json.dumps(config))
                wait_for(lambda: ipc(root, {"op":"metrics"})["metrics"]["animationMs"] == 0)
                ipc(root, {"op":"dismiss", "id":"all"})
                for i in range(3):
                    ipc(root, {"op":"send", "notice":{"id":"immediate-"+str(i), "title":"Immediate card", "timeout":0}})
                wait_for(lambda: len(ipc(root, {"op":"metrics"})["metrics"]["cards"]) == 3 and all(c["opacity"] == 1 and c["extent"] == 1 for c in ipc(root, {"op":"metrics"})["metrics"]["cards"]))
                ipc(root, {"op":"dismiss", "id":"all"})
                wait_for(lambda: not ipc(root, {"op":"metrics"})["metrics"]["cards"])
                config["stack"] = {"animationMs":220}
                (root / "config.json").write_text(json.dumps(config))
                wait_for(lambda: ipc(root, {"op":"metrics"})["metrics"]["animationMs"] == 220)
                time.sleep(.5)
                for notice in examples:
                    ipc(root, {"op":"send", "notice":{"app":"notjik", "timeout":0,"group":notice.get("type","standard"),**notice}})
                time.sleep(1.4)
                if "--demo" in sys.argv[2:]:
                    config["theme"] = "rose-punk"
                    (root / "config.json").write_text(json.dumps(config))
                    wait_for(lambda: ipc(root, {"op":"status"})["theme"] == "rose-punk")
                    ipc(root, {"op":"dismiss", "id":"all"})
                    time.sleep(0.8)
                    frames = []
                    frame_path = root / "frame.png"
                    def frames_for(count):
                        for _ in range(count):
                            frame_path.unlink(missing_ok=True)
                            ipc(root, {"op":"capture", "path":str(frame_path)})
                            wait_for(lambda: readable_image(frame_path))
                            with Image.open(frame_path) as frame:
                                crop = frame.crop((0, max(0, frame.height - 600), frame.width, frame.height)).convert("RGBA")
                                canvas = Image.new("RGBA", (frame.width + 32, 632), "#121018")
                                canvas.alpha_composite(crop, (16, 16))
                                frames.append(canvas.convert("RGB"))
                            moving = ipc(root, {"op":"metrics"})["metrics"]
                            shown = [card for card in moving["cards"] if card["opacity"] > 0.03 and not card["retiring"]]
                            if moving["contentHeight"] <= moving["height"] + 1:
                                for card in shown:
                                    assert card["y"] >= -1 and card["y"] + card["height"] <= moving["height"] + 1, moving
                                for before, after in zip(shown, shown[1:]):
                                    if after["y"] < before["y"] + before["height"] - 1:
                                        assert after["extent"] < 1 and after["cardZ"] > before["cardZ"], moving
                            time.sleep(0.08)
                    for index, notice in enumerate(examples):
                        ipc(root, {"op":"send", "notice":{"app":"notjik demo", "id":str(index), "group":str(index), **notice}})
                        frames_for(7)
                    for value in (10, 25, 50, 75, 100):
                        ipc(root, {"op":"send", "notice":{"app":"notjik demo", "id":"1", "group":"1", "title":"Archive upload", "type":"progress", "data":{"value":value,"label":"Saving your encrypted archive"}}})
                        frames_for(4)
                    frames_for(8)
                    ipc(root, {"op":"dismiss", "id":"all"})
                    frames_for(8)
                    frames[0].save(destination / "demo.gif", save_all=True, append_images=frames[1:], duration=100, loop=0)
                    print(destination / "demo.gif")
                    for notice in examples:
                        ipc(root, {"op":"send", "notice":{"app":"notjik", "timeout":0,"group":notice.get("type","standard"),**notice}})
                config["theme"] = "rose-punk"
                (root / "config.json").write_text(json.dumps(config))
                wait_for(lambda: ipc(root, {"op":"status"})["theme"] == "rose-punk")
                # Check packing, forward/backward chevrons, stable height, and original action indices.
                config["style"] = {"width":360}
                (root / "config.json").write_text(json.dumps(config))
                for labels in (["Open backup ↗"], ["Open backup ↗", "View details"], ["Yes", "No", "Later"],
                               ["Open this notification in the application"],
                               ["Open this notification in the application", "View backup details", "Retry", "Dismiss"],
                               ["Open", "Retry", "Ignore", "View details", "Dismiss"]):
                    for notice in ipc(root, {"op":"list"})["notices"]:
                        if notice["title"] == "Backup needs attention":
                            ipc(root, {"op":"dismiss", "id":notice["id"]})
                    time.sleep(.4)
                    ipc(root, {"op":"send", "notice":{"title":"Backup needs attention", "body":"Choose what to do with your archive", "timeout":0,
                        "actions":[{"label":label,"command":["true"]} for label in labels]}})
                    time.sleep(.5)
                    def action_card():
                        return next(card for card in ipc(root, {"op":"metrics"})["metrics"]["cards"] if card["actions"])
                    first = action_card()
                    count = first["actions"][0]["count"]
                    if len(labels) <= 3:
                        assert count == 1, first
                    seen = []
                    for page in range(count):
                        card = action_card()
                        row = card["actions"][0]
                        assert row["page"] == page, row
                        assert row["previous"] == (page > 0) and row["next"] == (page < count-1), row
                        assert abs(row["stripX"] + page * row["available"]) <= 1, row
                        assert card["footer"]["height"] == first["footer"]["height"], card
                        assert abs(row["x"]) <= 1, row
                        assert row["width"] <= row["available"], row
                        for button in row["buttons"]:
                            if button["index"] == 0:
                                assert abs(button["x"] + button["width"] - row["available"]) <= 1, row
                            else:
                                previous = [b for b in row["buttons"] if 0 < b["index"] < button["index"]]
                                assert abs(button["x"] - sum(b["width"] + 12 for b in previous)) <= 1, row
                            assert not button["truncated"] and button["height"] >= button["textHeight"] + 4, button
                            seen.append((button["index"], button["label"]))
                        if len(labels) in (1, 2, 4):
                            capture = destination / f"actions-{len(labels)}-page-{page+1}.png"
                            capture.unlink(missing_ok=True)
                            ipc(root, {"op":"capture", "path":str(capture)})
                            wait_for(lambda: readable_image(capture))
                        ipc(root, {"op":"advanceActions", "direction":1})
                        time.sleep(.25)
                    assert seen == list(enumerate(labels)), seen
                    for page in range(count-2, -1, -1):
                        ipc(root, {"op":"advanceActions", "direction":-1})
                        time.sleep(.25)
                        assert action_card()["actions"][0]["page"] == page
                # Confirm actual raster sender artwork wins over the supplied symbolic icon.
                ipc(root, {"op":"dismiss", "id":"all"})
                time.sleep(.4)
                artwork = fixtures(ROOT, "image")[0]
                ipc(root, {"op":"send", "notice":{**artwork, "timeout":0}})
                time.sleep(.5)
                rendered = ipc(root, {"op":"metrics"})["metrics"]["cards"]
                assert len(rendered) == 1 and rendered[0]["artworkReady"], rendered
                assert rendered[0]["artworkSource"] == artwork["image"], rendered
                image = destination / "artwork.png"
                ipc(root, {"op":"capture", "path":str(image)})
                wait_for(lambda: readable_image(image))
                for index in range(8):
                    ipc(root, {"op":"send", "notice":{"app":"Backup", "title":"Archive complete " + str(index+1), "body":"Encrypted archive saved · synthetic history example", "timeout":0}})
                ipc(root, {"op": "history", "mode": "show"})
                time.sleep(0.5)
                image = destination / "history.png"
                image.unlink(missing_ok=True)
                ipc(root, {"op": "captureHistory", "path": str(image)})
                wait_for(lambda: readable_image(image))
                print(image)
                ipc(root, {"op":"expandHistory"})
                time.sleep(.5)
                image = destination / "history-expanded.png"
                ipc(root, {"op":"captureHistory", "path":str(image)})
                wait_for(lambda: readable_image(image))
                print(image)
                ipc(root, {"op":"navigate", "direction":"previous"})
                time.sleep(.5)
                image = destination / "history-details.png"
                ipc(root, {"op":"captureHistory", "path":str(image)})
                wait_for(lambda: readable_image(image))
                print(image)
                log.flush()
                ipc(root, {"op":"history", "mode":"hide"})
                ipc(root, {"op":"dismiss", "id":"all"})
                time.sleep(.5)
                gallery = fixtures(ROOT, "urgency") + fixtures(ROOT, "multiline") + fixtures(ROOT, "image")
                for index, notice in enumerate(gallery):
                    ipc(root, {"op":"send", "notice":{"app":"synthetic gallery", "id":"gallery-"+str(index), "group":"gallery-"+str(index), "timeout":0, **notice}})
                wait_for(lambda: len(ipc(root, {"op":"metrics"})["metrics"]["cards"]) == 5 and all(c["opacity"] == 1 and c["extent"] == 1 for c in ipc(root, {"op":"metrics"})["metrics"]["cards"]))
                cards = ipc(root, {"op":"metrics"})["metrics"]["cards"]
                assert len(cards) == 5 and all(c["artworkReady"] for c in cards), cards
                assert len({c["titleInk"] for c in cards[:3]}) == 3, cards
                assert cards[-1]["artworkSource"].endswith("demo-artwork.png"), cards
                image = destination / "urgency-lists-artwork.png"
                ipc(root, {"op":"capture", "path":str(image)})
                wait_for(lambda: readable_image(image))
                print(image)
                errors = (destination / "render.log").read_text()
                assert not any(word in errors for word in ("ReferenceError", "TypeError", "Unable to assign", "Binding loop", "ERROR:")), errors
            finally:
                process.terminate()
                process.wait(timeout=5)


if __name__ == "__main__":
    main()
    if any(flag in sys.argv[2:] for flag in ("--groups", "--height-limit", "--telegram-formatting")):
        pass
    elif "--motion" in sys.argv[2:]:
        print("Synthetic motion previews captured")
    else:
        print("PASS: continuous burst velocity; geometry-driven arrivals; first arrival at bottom; no invisible push or reserved gaps; downward exits; repeated/appended stacks; rendered layouts")
