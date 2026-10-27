#!/usr/bin/env python3
"""Record native tjikOS tools in the shared 1200x720 Rose Punk stage.

Only demo fixtures are sent. UI actions use a private compositor's Wayland seat;
read-only instrumentation supplies target geometry and verifies resulting state.
"""
import argparse
from contextlib import contextmanager
import hashlib
import json
import os
from pathlib import Path
import shlex
import shutil
import signal
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
HERE = Path(__file__).resolve().parent
WIDTH, HEIGHT, FPS = 1200, 720, 60
FEATURES = (
    ("Live updates, in one place.", "Progress and status that stay readable."),
    ("A conversation, kept together.", "Grouped messages open straight into searchable history."),
    ("Reply without leaving your flow.", "Write and send from the notification itself."),
    ("Listen. Then leave a voice reply.", "Play a voice message and record a reply from the card."),
    ("The right urgency, at a glance.", "Low, normal and critical notifications together."),
)


def checked(args, **kwargs):
    return subprocess.check_output(args, text=True, stderr=subprocess.PIPE, **kwargs).strip()


def wait_for(fn, timeout=15):
    deadline = time.monotonic() + timeout
    last = None
    while time.monotonic() < deadline:
        try:
            result = fn()
            if result:
                return result
        except (OSError, ValueError, subprocess.SubprocessError) as error:
            last = error
        time.sleep(.08)
    raise ValueError("Timed out waiting for native showcase: " + str(last))


def replace(path, old, new):
    source = path.read_text()
    if source.count(old) != 1:
        raise ValueError("Showcase instrumentation needs updating: " + str(path))
    path.write_text(source.replace(old, new, 1))


def prepare_notjik(app):
    """Add stage control and read-only observations to a temporary source copy."""
    qml = app / "qml"
    shutil.copy2(HERE / "ShowcaseStage.qml", qml / "ShowcaseStage.qml")
    replace(qml / "shell.qml", "id: root", '''id: root
    ShowcaseStage { id: showcaseStage; controller: root; iconRoot: "file://" + Quickshell.env("SHOWCASE_ICONS") }
''')
    replace(qml / "shell.qml", "Variants {\n        model: root.screens", "Variants {\n        id: showcasePanels\n        model: root.screens")
    replace(qml / "shell.qml", "case \"status\": break;", '''case "showcase": {
            if (request.scene) showcaseStage.scene(request.scene);
            let card = null, panel = null;
            for (const p of showcasePanels.instances) {
                const c = p.showcaseCard(request.id);
                if (c) { card = c; panel = p; break; }
            }
            if (!request.id) return {ok:true, drawerOpen:drawerOpen, historyQuery:historyQuery,
                historyCount:historyItems.length, filteredCount:filteredHistoryItems.length,
                playing:telegramPlayback.playing, playbackPosition:telegramPlayback.player.position,
                recording:telegramRecording.phase, recordingDuration:telegramRecording.duration,
                recordingError:telegramRecording.error};
            if (!card) return {ok:true, present:false};
            const item = card.showcaseTarget(request.target || "card");
            if (!item) throw Error("Native control unavailable: " + request.target);
            const pos = item.mapToItem(panel.contentItem, item.width/2, request.target === "card" ? 18 : item.height/2);
            const origin = card.mapToItem(panel.contentItem, 0, 0);
            return {ok:true, present:true, centerX:panel.coordinates.x+origin.x+card.width/2,
                centerY:panel.coordinates.y+origin.y+card.height/2, cardHeight:card.height,
                urgency:card.notice.urgency, x:panel.coordinates.x+pos.x, y:panel.coordinates.y+pos.y,
                width:item.width, height:item.height, visible:item.visible, enabled:item.enabled,
                hovered:card.hovered, controlsRevealed:card.controlsRevealed,
                replying:!!card.showcaseDetail?.replying, draft:card.showcaseDetail?.showcaseDraft || ""};
        }
        case "status": break;''')
    replace(qml / "NotificationPanel.qml", "id: panel", '''id: panel
    function showcaseCard(id) {
        for (let i=0; i<cardItems.count; ++i) {
            const c=cardItems.itemAt(i).showcaseCard;
            if(c.notice.id === id) return c;
        }
        return null;
    }
''')
    # Presentation geometry only: keep the native stack centred as details expand.
    replace(qml / "NotificationPanel.qml",
            'readonly property var coordinates: Logic.geometry(placement, screen.width, screen.height, width, layoutHeight, screen.devicePixelRatio)',
            'readonly property var coordinates: ({x:(screen.width-width)/2, y:200})')
    replace(qml / "NotificationPanel.qml",
            'readonly property real layoutHeight: Math.max(1, (screen.height - 2 * placement.margin) * (controller.config.stack?.heightPercent || 75) / 100)',
            'readonly property real layoutHeight: 440')
    replace(qml / "NotificationPanel.qml",
            'readonly property real stackTop: placement.edge === "t" || ((placement.edge === "l" || placement.edge === "r") && placement.position === "l") ? 0\n'
            '        : ((placement.edge === "l" || placement.edge === "r") && placement.position === "c") ? Math.max(0, (layoutHeight - stack.implicitHeight) / 2)\n'
            '        : Math.max(0, layoutHeight - stack.implicitHeight)',
            'readonly property real stackTop: Math.max(0, (layoutHeight - stack.implicitHeight)/2)')
    replace(qml / "NotificationPanel.qml", "property var cachedNotice: null", "property var cachedNotice: null\n                        property alias showcaseCard: card")
    replace(qml / "NotificationCard.qml", "id: card\n", '''id: card
    property alias showcaseDetail: detail.item
    function showcaseTarget(name) {
        function find(item) {
            if (item.objectName === name && item.visible) return item;
            for (const child of (item.children || [])) { const match=find(child); if(match) return match; }
            return null;
        }
        return name === "card" ? card : name === "group" ? groupIndicator : find(card);
    }
''')
    replace(qml / "components/Telegram.qml", "id: detail\n", "id: detail\n    readonly property alias showcaseDraft: reply.text\n")
    # Fit the native drawer below the title, preserving its content and handlers.
    replace(qml / "HistoryDrawer.qml", 'implicitHeight: Math.min(760, (screen?.height || 1000) * 0.72)',
            'implicitHeight: Math.min(440, (screen?.height || 1000) * 0.72)')
    replace(qml / "HistoryDrawer.qml", 'bottom: targetScreen ? targetScreen.height - height - coordinates.y : 16;',
            'bottom: 80;')
    # Keep the production recorder and FFmpeg pipeline; use bundled audio input.
    # No microphone or network path can be reached from the demo helper.
    source = ROOT / "apps/notjik"
    shutil.copy2(source / "telegram_recording.py", app / "showcase_recording_backend.py")
    shutil.copy2(source / "examples/demo-voice.wav", app / "showcase-voice.wav")
    (app / "telegram_recording.py").write_text(
        'from pathlib import Path\nimport showcase_recording_backend as recording\n'
        'raise SystemExit(recording.run(["-re", "-stream_loop", "-1", "-i", '
        'str(Path(__file__).with_name("showcase-voice.wav"))]))\n')
    (app / "telegram_bridge.py").write_text('raise SystemExit("Use only demo fixtures in the showcase")\n')
    replace(qml / "TelegramPlayback.qml", 'audioOutput: AudioOutput {}',
            'audioOutput: AudioOutput { muted: true }')


@contextmanager
def native_session(directory):
    """A nested compositor, private runtime, configuration, state, and D-Bus."""
    runtime = directory / "r"
    runtime.mkdir(mode=0o700)
    config_home = directory / "config"
    fonts = Path.home() / ".config/fontconfig"
    if fonts.exists():
        shutil.copytree(fonts, config_home / "fontconfig")
    styles = Path.home() / ".local/share/quickshell/styles"
    if styles.exists():
        shutil.copytree(styles, directory / "data/quickshell/styles")
    config = directory / "hyprland.lua"
    config.write_text('hl.monitor({output="",mode="1200x720@60",position="0x0",scale=1})\n'
                      'hl.monitor({output="HEADLESS-1",mode="1200x720@60",position="10000x10000",scale=1})\n'
                      'hl.config({general={border_size=0},decoration={rounding=0,shadow={enabled=false}},misc={disable_watchdog_warning=true,disable_hyprland_logo=true,disable_splash_rendering=true}})\n')
    env = {**os.environ, "XDG_RUNTIME_DIR": str(runtime), "XDG_CONFIG_HOME": str(config_home),
           "XDG_CACHE_HOME": str(directory / "cache"), "XDG_DATA_HOME": str(directory / "data"),
           "XDG_STATE_HOME": str(directory / "state"), "QT_LOGGING_RULES": "qt.qpa.wayland.textinput=false"}
    parent_display = Path(os.environ["WAYLAND_DISPLAY"])
    env["WAYLAND_DISPLAY"] = str(parent_display if parent_display.is_absolute() else Path(os.environ["XDG_RUNTIME_DIR"]) / parent_display)
    env.pop("HYPRLAND_INSTANCE_SIGNATURE", None)
    env.pop("DBUS_STARTER_ADDRESS", None)
    env.pop("DBUS_STARTER_BUS_TYPE", None)
    children = []
    with (directory / "compositor.log").open("w") as log:
        try:
            compositor = subprocess.Popen(["Hyprland", "--config", str(config)], env=env, stdout=log, stderr=log, start_new_session=True)
            children.append(compositor)
            signature = wait_for(lambda: next((p.parent.name for p in (runtime / "hypr").glob("*/.socket.sock")), None))
            display = wait_for(lambda: next((p.name for p in runtime.glob("wayland-*") if not p.name.endswith(".lock")), None))
            env.update(WAYLAND_DISPLAY=display, HYPRLAND_INSTANCE_SIGNATURE=signature)
            before = {m["name"] for m in json.loads(checked(["hyprctl", "-j", "monitors"], env=env))}
            checked(["hyprctl", "output", "create", "headless"], env=env)
            monitor = wait_for(lambda: next((m["name"] for m in json.loads(checked(["hyprctl", "-j", "monitors"], env=env)) if m["name"] not in before), None))
            result = checked(["hyprctl", "eval", "hl.monitor({output=" + json.dumps(monitor) + ',mode="1200x720@60",position="10000x10000",scale=1})'], env=env)
            if result != "ok":
                raise ValueError(result)
            # Disable the nested window after the headless output is ready.
            for name in before:
                checked(["hyprctl", "eval", "hl.monitor({output=" + json.dumps(name) + ",disabled=true})"], env=env)
            checked(["hyprctl", "eval", "hl.dispatch(hl.dsp.focus({monitor=" + json.dumps(monitor) + "}))"], env=env)
            wait_for(lambda: any(m["name"] == monitor and m["width"] == WIDTH and m["height"] == HEIGHT for m in json.loads(checked(["hyprctl", "-j", "monitors"], env=env))))
            bus = subprocess.Popen(["dbus-daemon", "--nofork", "--config-file=" + str(ROOT / "apps/notjik/examples/comparison-bus.conf"), "--print-address=1", "--address=unix:path=" + str(directory / "bus")], env=env, stdout=subprocess.PIPE, text=True)
            children.append(bus)
            address = bus.stdout.readline().strip()
            if address != "unix:path=" + str(directory / "bus") and not address.startswith("unix:path=" + str(directory / "bus") + ","):
                raise ValueError("Private D-Bus did not start")
            env["DBUS_SESSION_BUS_ADDRESS"] = address
            yield env, monitor
        finally:
            for process in reversed(children):
                if process.poll() is None:
                    process.terminate()
                    try:
                        process.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        process.kill()
                        process.wait()


def build_pointer(directory):
    protocol = ROOT / "apps/bartjik/demo_pointer.xml"
    checked(["wayland-scanner", "client-header", str(protocol), str(directory / "pointer.h")])
    checked(["wayland-scanner", "private-code", str(protocol), str(directory / "pointer.c")])
    flags = shlex.split(checked(["pkg-config", "--cflags", "--libs", "wayland-client"]))
    target = directory / "pointer"
    checked(["cc", "-I" + str(directory), str(HERE / "showcase_pointer.c"), str(directory / "pointer.c"), "-o", str(target), *flags])
    return target


class Director:
    def __init__(self, app, env, pointer, monitor, review):
        self.app, self.env, self.review = app, env, review
        self.pointer = subprocess.Popen([str(pointer), monitor], env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, bufsize=1)
        self.position = (1080, 580)
        self.events, self.chapters = [], []
        self.started = time.monotonic()
        self.move(*self.position, duration=.1)

    def ipc(self, **request):
        # This process's environment is already the private session for ipc().
        from notjik import ipc
        return ipc(self.app, request)

    def observe(self, **data):
        return self.ipc(op="showcase", **data)

    def command(self, command):
        self.pointer.stdin.write(command + "\n")
        self.pointer.stdin.flush()
        if self.pointer.stdout.readline().strip() != "ok":
            raise ValueError("Native pointer disconnected")

    def move(self, x, y, duration=.7):
        start_x, start_y = self.position
        started = time.monotonic()
        frames = max(1, round(duration * FPS))
        for frame in range(1, frames + 1):
            t = frame / frames
            eased = t*t*t*(t*(t*6-15)+10)
            self.command(f"M {round(start_x+(x-start_x)*eased)} {round(start_y+(y-start_y)*eased)}")
            time.sleep(max(0, started + frame/FPS - time.monotonic()))
        self.position = x, y

    def point(self, ident, target, click=False):
        geometry = wait_for(lambda: (lambda s: s if s.get("present") and s.get("visible") and s.get("enabled") else None)(self.observe(id=ident, target=target)))
        self.move(geometry["x"], geometry["y"])
        time.sleep(.25)
        if click:
            # Re-measure after hover-driven layout animation, then route a real button.
            geometry = self.observe(id=ident, target=target)
            self.move(geometry["x"], geometry["y"], duration=.2)
            self.command("B 272 1")
            time.sleep(.09)
            self.command("B 272 0")
        self.events.append({"time":round(time.monotonic()-self.started,3), "input":"wayland-pointer", "target":target, "click":click})

    def type(self, text):
        checked(["wtype", "-d", "55", text], env=self.env)
        self.events.append({"time":round(time.monotonic()-self.started,3), "input":"wayland-keyboard", "text":text})

    def key(self, name):
        checked(["wtype", "-k", name], env=self.env)

    def scene(self, number):
        heading, caption = FEATURES[number-1]
        self.observe(scene={"mode":"feature", "heading":heading, "caption":caption, "chapter":number, "chapterCount":len(FEATURES)})
        self.chapters.append({"feature":heading, "start":round(time.monotonic()-self.started,3)})
        print(f"{number}/{len(FEATURES)} {heading}", flush=True)
        time.sleep(.65)

    def frame(self, name):
        checked(["grim", "-o", self.env["SHOWCASE_MONITOR"], str(self.review / (name + ".png"))], env=self.env)

    def send(self, key, **notice):
        result = self.ipc(op="send", notice={"id":key, "app":"notjik demo", "group":key, "timeout":0, **notice})
        return result["id"]

    def clear(self):
        self.move(1080, 580, .45)
        for notice in self.ipc(op="list")["notices"]:
            self.ipc(op="dismiss", id=notice["id"])
        self.ipc(op="history", mode="hide")
        time.sleep(.8)

    def centred(self, ident, label):
        state = wait_for(lambda: (lambda s: s if s.get("present") and abs(s.get("centerX", 0)-600) < 1
                         and abs(s.get("centerY", 0)-420) < 1 else None)(self.observe(id=ident)))
        self.events.append({"time":round(time.monotonic()-self.started,3), "verified":"centred " + label,
                            "center":[state["centerX"],state["centerY"]],"height":state["cardHeight"]})
        return state

    def run(self, source):
        from demo import fixtures
        self.observe(scene={"mode":"intro", "heading":"Notifications with flair", "caption":"Live updates. Telegram integration. Replies in place.", "chapterCount":len(FEATURES)})
        time.sleep(2.3)
        self.frame("00-intro")
        self.scene(1)
        ident = self.send("charts", title="Star chart transfer", body="Unknown sector · navigation data incoming", type="progress", data={"value":4,"label":"Receiving star charts"})
        time.sleep(.7)
        for value in (18, 36, 57, 78, 100):
            self.send("charts", title="Star chart transfer", body="Unknown sector · navigation data incoming", type="progress", data={"value":value,"label":"Sector mapped" if value == 100 else "Receiving star charts"})
            time.sleep(.5)
        self.send("relay", title="Deep-space relay", body="All systems ready for the next jump.", type="status", data={"state":"Connected","tone":"success","detail":"Signal stable · deck 7"})
        time.sleep(1.1)
        self.frame("01-live-updates")
        self.point(ident,"card")
        wait_for(lambda:self.observe(id=ident).get("hovered"))
        time.sleep(.65)
        self.clear()
        self.scene(2)
        for i, body in enumerate(("Maya: That moon was not on the chart.", "IO: Neither was the second one.", "Maya: Tell me those are moons.")):
            member = self.send("bridge-"+str(i), group="bridge", title="Bridge channel", body=body)
            if i == 0:
                grouped = member
            time.sleep(.65)
        self.frame("02-grouped")
        self.point(grouped, "card")
        self.point(grouped, "group")
        checked([sys.executable, "-B", str(self.app / "bin/notjikctl"), "history", "show"], env=self.env)
        wait_for(lambda:self.observe().get("drawerOpen"))
        self.events.append({"time":round(time.monotonic()-self.started,3), "command":"notjikctl history show"})
        self.move(600,235)
        self.command("B 272 1")
        time.sleep(.09)
        self.command("B 272 0")
        time.sleep(.85)
        self.frame("03-history")
        # Clicking the native drawer acquires its OnDemand keyboard focus.
        self.key("slash")
        self.type("moon")
        wait_for(lambda:self.observe().get("historyQuery") == "moon")
        state = self.observe()
        if not 0 < state["filteredCount"] < state["historyCount"]:
            raise ValueError("Native history filter did not narrow the results: " + str(state))
        self.events.append({"time":round(time.monotonic()-self.started,3), "verified":"native history search", "query":state["historyQuery"], "matches":state["filteredCount"], "total":state["historyCount"]})
        time.sleep(1.1)
        self.frame("04-history-search")
        self.key("Escape")
        self.key("Escape")
        wait_for(lambda:not self.observe().get("drawerOpen"))
        self.clear()
        self.scene(3)
        fixture = fixtures(source, "telegram-text")[0]
        ident = self.send("maya", **fixture)
        time.sleep(.9)
        self.centred(ident, "collapsed reply card")
        self.point(ident, "card")
        self.point(ident, "telegram-compose", click=True)
        wait_for(lambda:self.observe(id=ident).get("replying"))
        self.point(ident, "telegram-reply", click=True)
        self.type("On my way. Bringing a scanner.")
        wait_for(lambda:self.observe(id=ident).get("draft") == "On my way. Bringing a scanner.")
        self.events.append({"time":round(time.monotonic()-self.started,3), "verified":"native reply text", "draft":self.observe(id=ident)["draft"]})
        self.centred(ident, "expanded reply card")
        self.frame("05-inline-reply")
        time.sleep(.7)
        self.point(ident,"telegram-send",click=True)
        wait_for(lambda:not any(n["id"] == ident for n in self.ipc(op="list")["notices"]))
        self.events.append({"time":round(time.monotonic()-self.started,3), "verified":"native reply handler closed demo notice"})
        time.sleep(.75)
        self.clear()
        self.scene(4)
        ident = self.send("maya-voice", **fixtures(source, "telegram-voice")[0])
        time.sleep(.9)
        self.centred(ident, "voice card")
        self.point(ident, "card")
        self.point(ident, "telegram-play", click=True)
        wait_for(lambda:self.observe().get("playing") and self.observe().get("playbackPosition",0) > 1500)
        state = self.observe()
        self.events.append({"time":round(time.monotonic()-self.started,3), "verified":"native voice playback", "position":state["playbackPosition"]})
        self.frame("06-voice-playback")
        self.point(ident, "telegram-play", click=True)
        wait_for(lambda:not self.observe().get("playing"))
        self.point(ident, "telegram-record", click=True)
        wait_for(lambda:self.observe().get("recording") == "recording")
        wait_for(lambda:self.observe().get("recordingDuration",0) >= 3000)
        self.centred(ident, "expanded recording card")
        self.events.append({"time":round(time.monotonic()-self.started,3), "verified":"native voice recording",
                            "duration":self.observe()["recordingDuration"], "input":"bundled demo audio via production recorder"})
        self.frame("07-voice-recording")
        self.point(ident, "telegram-record-discard", click=True)
        wait_for(lambda:self.observe().get("recording") == "idle")
        self.centred(ident, "voice card after reset")
        self.frame("08-voice-reset")
        self.clear()
        self.scene(5)
        urgency_ids=[]
        for notice in fixtures(source, "urgency"):
            notice = {**notice, "title":notice["urgency"].capitalize() + " · " + notice["title"]}
            urgency_ids.append(self.send("urgency-"+notice["urgency"], **notice))
            time.sleep(.85)
        time.sleep(.8)
        levels=[self.observe(id=ident).get("urgency") for ident in urgency_ids]
        if levels != ["low", "normal", "critical"]:
            raise ValueError("Urgency cards were not all presented: " + str(levels))
        self.events.append({"time":round(time.monotonic()-self.started,3), "verified":"native urgency levels", "levels":levels})
        self.frame("09-urgency-levels")
        time.sleep(2)
        self.clear()
        self.observe(scene={"mode":"outro", "chapter":len(FEATURES)})
        time.sleep(1.2)
        self.frame("10-outro")
        time.sleep(2)


def record(output):
    source = ROOT / "apps/notjik"
    sys.path.insert(0, str(source))
    review = output.with_suffix(".review")
    review.mkdir(parents=True)
    with tempfile.TemporaryDirectory(prefix="oc-") as name:
        directory = Path(name)
        app = directory / "app"
        for subdir in ("qml", "themes"):
            shutil.copytree(source / subdir, app / subdir)
        for module in source.glob("*.py"):
            shutil.copy2(module, app / module.name)
        shutil.copytree(source / "bin", app / "bin")
        prepare_notjik(app)
        pointer = build_pointer(directory)
        child = recorder = director = None
        try:
            with native_session(directory) as (env, monitor):
                env.update(NOTJIK_CONFIG=str(directory / "notjik.json"), NOTJIK_STATE_DIR=str(directory / "history"),
                           SHOWCASE_ICONS=str(ROOT / "docs/icons"), SHOWCASE_MONITOR=monitor)
                config = {"theme":"rose-punk", "style":{"width":640,"fontSize":20,"iconSize":80,"padding":20},
                          "placement":{"monitors":"primary","primaryMonitor":monitor,"edge":"b","position":"c","margin":80,"offsetY":-155},
                          "grouping":{"enabled":True},"history":{"enabled":True}, "stack":{"heightPercent":65}}
                (directory / "notjik.json").write_text(json.dumps(config))
                previous = os.environ.copy()
                os.environ.clear()
                os.environ.update(env)
                try:
                    with (directory / "app.log").open("w") as app_log, (directory / "record.log").open("w") as record_log:
                        child = subprocess.Popen(["quickshell", "--path", str(app / "qml"), "--no-color"], env=env, stdout=app_log, stderr=app_log)
                        from notjik import ipc
                        wait_for(lambda: ipc(app, {"op":"status"}).get("lockState") == "unlocked")
                        director = Director(app, env, pointer, monitor, review)
                        time.sleep(3)
                        raw = directory / "capture.mp4"
                        recorder = subprocess.Popen(["wf-recorder","-D","--no-dmabuf","-o",monitor,"-r",str(FPS),"-c","libx264rgb","-p","preset=ultrafast","-p","crf=0","-x","bgr0","-f",str(raw)], env=env, stdout=record_log, stderr=record_log)
                        time.sleep(.3)
                        if recorder.poll() is not None:
                            raise ValueError("Recorder failed: " + (directory / "record.log").read_text())
                        director.started = time.monotonic()
                        director.run(source)
                        recorder.send_signal(signal.SIGINT)
                        recorder.wait(timeout=15)
                        recorder = None
                        checked(["ffmpeg","-v","error","-i",str(raw),"-an","-vf","fps=60","-c:v","libx264","-preset","slow","-crf","17","-pix_fmt","yuv420p","-movflags","+faststart",str(output)])
                        probe = json.loads(checked(["ffprobe","-v","error","-show_streams","-show_format","-of","json",str(output)]))
                        stream = probe["streams"][0]
                        if (stream["width"],stream["height"],stream["r_frame_rate"]) != (WIDTH,HEIGHT,"60/1"):
                            raise ValueError("Unexpected output geometry or frame rate")
                        manifest = {"tool":"notjik", "dimensions":[WIDTH,HEIGHT], "fps":FPS, "demoFixturesOnly":True,
                                    "input":"private Wayland compositor seat; virtual pointer and wtype", "duration":probe["format"]["duration"],
                                    "sha256":hashlib.sha256(output.read_bytes()).hexdigest(),
                                    "sourceCommit":checked(["git","-C",str(ROOT),"rev-parse","HEAD"]),
                                    "directorSha256":hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
                                    "stageSha256":hashlib.sha256((HERE / "ShowcaseStage.qml").read_bytes()).hexdigest(), "chapters":director.chapters,"evidence":director.events}
                        output.with_suffix(".json").write_text(json.dumps(manifest,indent=2)+"\n")
                        print("Recording: " + str(output), flush=True)
                finally:
                    if recorder and recorder.poll() is None:
                        recorder.send_signal(signal.SIGINT)
                        recorder.wait(timeout=10)
                    if director:
                        director.pointer.stdin.close()
                        director.pointer.wait(timeout=5)
                    if child and child.poll() is None:
                        child.terminate()
                        child.wait(timeout=8)
                    os.environ.clear()
                    os.environ.update(previous)
        finally:
            for log in directory.glob("*.log"):
                shutil.copy2(log, review / log.name)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("tool", choices=("notjik",))
    parser.add_argument("output", type=Path, nargs="?")
    parser.add_argument("--plan", action="store_true")
    args = parser.parse_args()
    if args.plan:
        print(json.dumps({"dimensions":[WIDTH,HEIGHT],"fps":FPS,"features":FEATURES,"outro":"Icon, name, repository path, six-tool family"},indent=2))
        return
    if not args.output:
        parser.error("recording requires OUTPUT.mp4")
    output = args.output.expanduser().absolute()
    if output.suffix != ".mp4" or any(p.exists() for p in (output,output.with_suffix(".json"),output.with_suffix(".review"))):
        parser.error("choose a new .mp4 path; existing videos and evidence are never overwritten")
    output.parent.mkdir(parents=True,exist_ok=True)
    for command in ("Hyprland","hyprctl","quickshell","dbus-daemon","wf-recorder","ffmpeg","ffprobe","grim","wtype","cc","wayland-scanner","pkg-config"):
        if not shutil.which(command):
            parser.error("missing dependency: " + command)
    record(output)


if __name__ == "__main__":
    main()
