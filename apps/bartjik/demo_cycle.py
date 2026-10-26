"""Record the synthetic Bartjik/Waybar comparison without touching the session bar."""
import argparse
from contextlib import contextmanager
import json
import hashlib
import os
from pathlib import Path
import re
import shutil
import signal
import shlex
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parent
from demo_tray import fixtures as tray_fixtures
SCENES = (
    ("markets", ("prices",)), ("media", ("music",)), ("workspaces", ("spaces",)),
    ("applications", ("tray",)), ("ai", ("quota", "credit")),
    ("connectivity", ("net", "vpn", "tailnet")),
    ("audio-clock-input", ("sound", "time", "input")),
    ("controls", ("notifications", "idle", "filter", "display")),
)

WAYBAR_ITEMS = {
    "prices": ("custom/crypto", "₿ 67432  \u200a⊙ 148  \u200aⓐ 18.4", "BTC  67,432 USD\nSOL     148 USD\nAR     18.4 USD"),
    "music": ("custom/spotify", " Karla Blum - Acid Rain", "Karla Blum - Acid Rain"),
    "spaces": ("custom/workspaces", " 1   2  <span foreground='#ff53a6'> 3</span>   6   X", "Workspace 3"),
    "tray": ("custom/tray", "◌  ◇  ◉", "Synthetic app tray"),
    "quota": ("custom/codex", "󰃭 83%", "Codex weekly remaining: 83%\nReset: 3d 12h · synthetic account"),
    "credit": ("custom/openrouter", "$24.80", "OpenRouter credit: $24.80"),
    "net": ("network", " Demo Wi-Fi", "Demo Wi-Fi · synthetic network status"),
    "vpn": ("custom/vpn", "JP", "Mullvad: Japan, Tokyo"),
    "tailnet": ("custom/tailscale", "vpn_lock", "Tailscale: connected"),
    "sound": ("custom/audio-output", "headphones", "Audio route: Synthetic headphones\nVolume: 42%"),
    "volume": ("pulseaudio", "42%", "Volume: 42%"),
    "time": ("clock", "21:34", "Monday, 15 June 2026"),
    "input": ("custom/input-method", "EN", "English"),
    "notifications": ("custom/notifications", "", "Notifications : ON"),
    "idle": ("custom/caffeine", "", "Idle Inhibit : ON"),
    "filter": ("custom/nightlight", "󰖙", "Nightshift : OFF"),
    "display": ("custom/display-mode", "", "Desktop Mode"),
}

CAPTIONS = {
    "markets":"Market prices together · hover for a tooltip or a live graph",
    "media":"The same track · a tooltip beside playback controls",
    "workspaces":"Workspace islands · names beside window previews",
    "applications":"Application icons · hover to identify each app",
    "ai":"Usage and credit · compact summaries beside account details",
    "connectivity":"Wi-Fi, VPN and tailnet · one connected island",
    "audio-clock-input":"Audio, clock and input · everyday controls together",
    "controls":"Notifications, idle, nightshift and display · readable state",
}


def build_helpers(directory):
    checked(["wayland-scanner","client-header",str(ROOT / "demo_pointer.xml"),str(directory / "pointer.h")])
    checked(["wayland-scanner","private-code",str(ROOT / "demo_pointer.xml"),str(directory / "pointer.c")])
    flags=shlex.split(checked(["pkg-config","--cflags","--libs","wayland-client"]))
    checked(["cc","-I"+str(directory),str(ROOT / "demo_pointer.c"),str(directory / "pointer.c"),"-o",str(directory / "pointer"),*flags])
    flags=shlex.split(checked(["pkg-config","--cflags","--libs","gtk+-3.0"]))
    checked(["cc","-shared","-fPIC",str(ROOT / "demo_gtk.c"),"-o",str(directory / "gtk.so"),*flags])


def checked(args):
    return subprocess.check_output(args, text=True, stderr=subprocess.PIPE).strip()


def wait_for(fn, timeout=12):
    end = time.monotonic() + timeout
    last = None
    while time.monotonic() < end:
        try:
            value = fn()
            if value:
                return value
        except (OSError, ValueError) as error:
            last = error
        time.sleep(.08)
    raise ValueError("Timed out waiting for synthetic Bartjik: " + str(last))


@contextmanager
def private_bus():
    previous = os.environ.get("DBUS_SESSION_BUS_ADDRESS")
    with tempfile.TemporaryDirectory(prefix="bartjik-bus-") as directory:
        bus = subprocess.Popen(["dbus-daemon", "--session", "--nofork", "--print-address=1",
                                "--address=unix:path=" + directory + "/bus"], stdout=subprocess.PIPE, text=True)
        fixture = None
        try:
            address = bus.stdout.readline().strip()
            if not address.startswith("unix:path=" + directory + "/bus"):
                raise ValueError("Private demo bus did not start; refusing desktop bus fallback")
            os.environ["DBUS_SESSION_BUS_ADDRESS"] = address
            fixture = subprocess.Popen(["/usr/bin/python3", "-B", str(ROOT / "demo_tray.py")])
            yield
        finally:
            for process in (fixture, bus):
                if process:
                    process.terminate()
                    process.wait(timeout=5)
            if previous is None:
                os.environ.pop("DBUS_SESSION_BUS_ADDRESS", None)
            else:
                os.environ["DBUS_SESSION_BUS_ADDRESS"] = previous


@contextmanager
def private_compositor():
    """Isolate capture geometry and input from desktop monitor automation."""
    directory=Path(tempfile.mkdtemp(prefix="bc-"))
    runtime=directory/"r"
    runtime.mkdir(mode=0o700)
    fontconfig=Path.home()/".config/fontconfig"
    if fontconfig.exists():shutil.copytree(fontconfig,directory/"config/fontconfig")
    config=directory/"hyprland.lua"
    config.write_text('hl.monitor({output="",mode="1000x680@60",position="0x0",scale=1})\n'
                      'hl.config({general={border_size=0},decoration={rounding=12,shadow={enabled=false}}})\n')
    keys=("XDG_RUNTIME_DIR","WAYLAND_DISPLAY","HYPRLAND_INSTANCE_SIGNATURE","XDG_CONFIG_HOME","XDG_CACHE_HOME","XDG_DATA_HOME")
    previous={key:os.environ.get(key) for key in keys}
    env={**os.environ,"XDG_RUNTIME_DIR":str(runtime),"XDG_CONFIG_HOME":str(directory/"config"),"XDG_CACHE_HOME":str(directory/"cache"),"XDG_DATA_HOME":str(directory/"data")}
    parent_display=Path(os.environ.get("WAYLAND_DISPLAY","wayland-1"))
    env["WAYLAND_DISPLAY"]=str(parent_display if parent_display.is_absolute() else Path(os.environ["XDG_RUNTIME_DIR"])/parent_display)
    env.pop("HYPRLAND_INSTANCE_SIGNATURE",None)
    with (directory/"compositor.log").open("w") as log:
        process=subprocess.Popen(["Hyprland","--config",str(config)],env=env,stdout=log,stderr=log,start_new_session=True)
        print("Private compositor: "+str(directory),flush=True)
        try:
            signature=wait_for(lambda:next((p.parent.name for p in (runtime/"hypr").glob("*/.socket.sock")),None),timeout=20)
            os.environ.update({key:env[key] for key in keys if key in env})
            display=wait_for(lambda:next((p.name for p in runtime.glob("wayland-*") if not p.name.endswith(".lock")),None))
            os.environ.update(WAYLAND_DISPLAY=display,HYPRLAND_INSTANCE_SIGNATURE=signature)
            yield
        finally:
            process.terminate()
            try:process.wait(timeout=8)
            except subprocess.TimeoutExpired:process.kill();process.wait()
            for key,value in previous.items():
                if value is None:os.environ.pop(key,None)
                else:os.environ[key]=value


@contextmanager
def output_monitor():
    before = {m["name"] for m in json.loads(checked(["hyprctl", "-j", "monitors"]))}
    name = None
    try:
        checked(["hyprctl", "output", "create", "headless"])
        for _ in range(30):
            new = [m["name"] for m in json.loads(checked(["hyprctl", "-j", "monitors"])) if m["name"] not in before]
            if len(new) == 1:
                name = new[0]
                break
            time.sleep(.1)
        if not name:
            raise ValueError("Headless recording output did not appear")
        time.sleep(4)
        if checked(["hyprctl", "eval", "hl.monitor({output=" + json.dumps(name) + ",mode=\"1000x680@60\",position=\"10000x10000\",scale=1})"]) != "ok":
            raise ValueError("Could not configure the recording output")
        time.sleep(2)
        wait_for(lambda:any(m["name"]==name and m["width"]==1000 and m["height"]==680 and m["scale"]==1 for m in json.loads(checked(["hyprctl","-j","monitors"]))))
        yield name
    finally:
        if name:
            subprocess.run(["hyprctl", "output", "remove", name], capture_output=True, timeout=5)


def demo_config(monitor, scene=None):
    config = json.loads((ROOT / "examples/comparison.json").read_text())
    for bar in config["bars"]:
        bar["monitors"] = [monitor]
        bar["autoHide"] = False
        bar["position"] = "c"
        bar["alignment"] = "c"
        bar["offsetY"] = -110
        if scene:
            island, modules = scene
            bar["islands"] = [item for item in bar["islands"] if item["id"] == island]
            if bar["islands"]:
                bar["islands"][0] = dict(bar["islands"][0], modules=list(modules))
    config["bars"] = [b for b in config["bars"] if b["islands"]]
    config["layout"]["centre_width"] = "100%"
    for module in config["modules"].values():module["demoPresentation"]=True
    config["modules"]["tray"]["demoTrayFixtures"] = tray_fixtures()
    config["modules"]["music"]["artist"]["style"]["maxTextWidth"] = 120
    config["modules"]["music"]["track"]["style"]["maxTextWidth"] = 190
    config["modules"]["music"]["artist"]["style"]["marquee"] = False
    config["modules"]["music"]["track"]["style"]["marquee"] = False
    config["modules"]["spaces"]["preview"]["width"] = 420
    return config


def load_waybar_config():
    """Read the managed desktop config; comments are the only JSONC feature used."""
    source = Path("/home/io/.config/waybar/config").read_text()
    return json.loads(re.sub(r'("(?:\\.|[^"\\])*")|//[^\n]*', lambda m: m[1] or "", source))


def waybar_config(directory, monitor, scene=None):
    island, modules = scene or ("all-islands", tuple(WAYBAR_ITEMS))
    if "sound" in modules:
        modules = tuple(m for item in modules for m in (("sound","volume") if item=="sound" else (item,)))
    script = directory / "waybar-fixture.py"
    names = [waybar_name(module) for module in modules]
    values = {waybar_name(module): {"text": WAYBAR_ITEMS[module][1], "tooltip": WAYBAR_ITEMS[module][2]} for module in modules}
    script.write_text("#!/usr/bin/env python3\nimport json,sys\nVALUES=" + repr(values) + "\nprint(json.dumps(VALUES[sys.argv[1]]))\n")
    script.chmod(0o755)
    # Preserve the user's real desktop config objects, then make a private
    # recording-only placement and replace every data/action path with a fixture.
    original = load_waybar_config()[0 if island in ("media", "workspaces") else 1]
    config = {k: v for k, v in original.items() if not isinstance(v, dict) and not k.startswith(("on-", "modules-", "margin"))}
    config.update(output=monitor, mode="dock", width=0, exclusive=False,
                  **{"margin-left": {"markets":10,"applications":12,"ai":12,"connectivity":12,"audio-clock-input":12}.get(island,0), "margin-right": 460, "margin-bottom": 110,
                     "modules-center": ["group/ai"] if island == "ai" else names})
    if island == "ai":
        config["group/ai"] = original["group/ai"]
    for name in names:
        original_name = next(WAYBAR_ITEMS[m][0] for m in modules if waybar_name(m) == name)
        config[name] = {k: v for k, v in original.get(original_name, {}).items() if k in ("escape", "justify", "min-length", "max-length")}
        config[name].update({"format": "{}", "return-type": "json", "exec": shlex.quote(str(script)) + " " + shlex.quote(name),
                        "interval": "once", "tooltip": True}
        )
    if island == "applications":
        config.pop("custom/tray", None)
        config["modules-center"]=["tray"]
        config["tray"]={key:value for key,value in original["tray"].items() if not key.startswith("on-")}
        values={}
    path = directory / "waybar.json"
    (directory / "tooltips.ini").write_text("\n".join("["+name.replace("/","-")+"]\ntext="+value["tooltip"].replace("\n",r"\n")+"\n" for name,value in values.items()))
    path.write_text(json.dumps(config))
    for name in ("style.css", "colors.css"):
        shutil.copy2(Path("/home/io/.config/waybar") / name, directory / name)
    css = (directory / "style.css").read_text()
    for native in ("network", "clock", "workspaces", "pulseaudio"):
        css = re.sub(r"#" + native + r"\b", "#custom-" + native, css)
    (directory / "style.css").write_text(css)
    return path


def waybar_name(module):
    name = WAYBAR_ITEMS[module][0]
    return name if name.startswith("custom/") else "custom/" + name


def prepare(qml):
    """Expose only directed synthetic-demo actions in a private QML copy."""
    bar = qml / "Bar.qml"
    source = bar.read_text().replace("import QtQuick", "import QtQuick\nimport QtTest", 1)
    source = re.sub(r"    readonly property var geometry:.*", "    readonly property var geometry: ({x:controller.stageFinale ? (1000-groups.implicitWidth)/2 : 730-groups.implicitWidth/2,y:570-groups.implicitHeight/2,width:groups.implicitWidth,height:groups.implicitHeight})", source)
    source = source.replace("    id: panel", '''    id: panel
    TestEvent {id:demoInput}
    readonly property bool demoMoving: demoMotion.running
    Timer {
        id:demoMotion;interval:16;repeat:true
        property point from;property point to;property double started:0
        function move(point){
            stop();from=controller.stagePointer;to=point;started=Date.now();
            if(from.x<0){panel.demoMove(point);return;}
            start();
        }
        onTriggered:{
            const progress=Math.min(1,(Date.now()-started)/700);
            const t=progress*progress*(3-2*progress);
            panel.demoMove(Qt.point(from.x+(to.x-from.x)*t,from.y+(to.y-from.y)*t));
            if(progress>=1)stop();
        }
    }
    function demoMove(point){
        demoInput.mouseMove(contentItem,point.x,point.y,0,Qt.NoButton,Qt.NoModifier);
        controller.stagePointer=point;
        controller.pointer={valid:true,monitor:targetScreen.name,x:point.x,y:point.y};
        controller.pointerUpdated=Date.now();
    }
    function demoHover(id,index,leave) {
        const island=groups.children.find(c=>c.islandId===id);
        if(!island)return {ok:false};
        const origin=island.mapToItem(contentItem,0,0);
        const targets=island.previewTargets();
        if(leave){demoMotion.stop();demoInput.mouseMove(contentItem,0,0,0,Qt.NoButton,Qt.NoModifier);}
        else if(index!==undefined){
            const t=targets[index];
            demoMotion.move(Qt.point(origin.x+t.x+Math.min(12,t.width/2),origin.y+t.y+t.height/2));
        }
        return {ok:true,moving:demoMoving,x:origin.x,y:origin.y,width:island.width,height:island.height,targets:targets.map(t=>({x:t.x,y:t.y,width:t.width,height:t.height,id:t.row.id,module:t.context.module}))};
    }''', 1)
    bar.write_text(source)
    # Keep the production panel anchor and its smooth tracking animation.
    workspace = qml / "WorkspacePreview.qml"
    source = workspace.read_text()
    source = re.sub(r"    implicitWidth:.*", "    implicitWidth:420", source)
    workspace.write_text(source)
    detail = qml / "DetailPanel.qml"
    source = detail.read_text()
    source = re.sub(r"(    readonly property real targetHeight:)([^\n]+)", r"\1Math.min(360,\2)", source)
    detail.write_text(source)
    shell = qml / "shell.qml"
    source = shell.read_text()
    marker = "        case 'status':break;"
    source = source.replace('    property var state: ({ready:false,config:{},bars:[],providers:{}})', '''    property var state: ({ready:false,config:{},bars:[],providers:{}})
    property string stageTitle: "Waybar vs Bartjik"
    property string stageCaption: "Synthetic desktop island comparison"
    property string stageCounter: ""
    property bool stageIntro: true
    property bool stageFinale: false
    property bool stageCurtain: false
    property var stageMontage: []
    property point stagePointer: Qt.point(-40,-40)
    DemoStage { controller: root }
    Timer {
        id:demoDrag;interval:16;repeat:true
        property var card:null
        property point origin
        property point offset
        property point destination
        property double started:0
        function begin(x,y){
            card=root.cryptoPanel;origin=card.attachedPosition();
            offset=Qt.point(card.width/2,24);destination=Qt.point(x,y);
            card.beginDrag(offset);started=Date.now();start();
        }
        onTriggered:{
            const progress=Math.min(1,(Date.now()-started)/1000);
            const t=progress*progress*(3-2*progress);
            const point=Qt.point(offset.x+(destination.x-origin.x)*t,offset.y+(destination.y-origin.y)*t);
            card.moveDrag(point);
            root.stagePointer=Qt.point(origin.x+point.x,origin.y+point.y);
            if(progress>=1){card.endDrag(point);stop();}
        }
    }''', 1)
    injected = '''        case 'demoStage':
            if(req.title !== undefined)root.stageTitle=req.title;
            if(req.caption !== undefined)root.stageCaption=req.caption;
            if(req.counter !== undefined)root.stageCounter=req.counter;
            if(req.intro !== undefined)root.stageIntro=req.intro;
            if(req.finale !== undefined)root.stageFinale=req.finale;
            if(req.curtain !== undefined)root.stageCurtain=req.curtain;
            if(req.montage !== undefined)root.stageMontage=req.montage;
            if(req.x !== undefined)root.stagePointer=Qt.point(req.x,req.y);
            return {ok:true};
        case 'demoHover':return panels.instances.find(p=>p.bar.islands.some(i=>i.id===req.island)).demoHover(req.island,req.index,req.leave);
        case 'demoDetach':
            if(!demo || !cryptoPanel.opened)throw Error('open a synthetic crypto panel first');
            demoDrag.begin(req.x,req.y);
            return {ok:true};
        case 'cryptoPool':return {ok:true,cards:cryptoPanels.map(p=>({pair:p.pair,opened:p.opened,detached:p.detached,points:p.points.length}))};
        case 'status':break;'''
    if marker not in source:
        raise ValueError("Bartjik demo instrumentation needs updating")
    source = source.replace("if(msg.kind==='pointer'){root.pointer=msg.pointer;root.pointerUpdated=Date.now();}", "if(msg.kind==='pointer'){}")
    shell.write_text(source.replace(marker, injected, 1))
    shutil.copy2(ROOT / "DemoStage.qml", qml / "DemoStage.qml")


def scene_label(config, scene):
    island, modules = scene
    return "AI usage and credit" if island == "ai" else island.replace("-", " ").title()


class Cycle:
    def __init__(self, qml, config_path, monitor, speed, waybar_dir, log):
        self.qml, self.config_path, self.monitor, self.speed = qml, config_path, monitor, speed
        sys.path.insert(0, str(ROOT))
        from bartjik import ipc
        self.ipc = ipc
        self.timeline = []
        self.started = time.monotonic()
        self.waybar_dir, self.log, self.waybar = waybar_dir, log, None
        self.pointer_process = subprocess.Popen([str(waybar_dir / "pointer"), monitor], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
        self.evidence=[]
        self.scenes=SCENES
        self.pointer_position = None

    def pause(self, seconds):
        time.sleep(seconds * max(1, self.speed))

    def set_config(self, config):
        self.config_path.write_text(json.dumps(config))
        self.ipc({"op": "reload"})
        wait_for(lambda: [i["id"] for b in self.ipc({"op":"bars"})["bars"] for i in b["islands"]] == [i["id"] for b in config["bars"] for i in b["islands"]])
        self.pause(.5)

    def set_waybar(self, scene):
        if self.waybar:
            self.waybar.terminate(); self.waybar.wait(timeout=3); self.waybar = None
        path = waybar_config(self.waybar_dir, self.monitor, scene)
        self.waybar_left=json.loads(path.read_text())["margin-left"]
        geometry = self.waybar_dir / "geometry"
        geometry.unlink(missing_ok=True)
        env = {**os.environ, "GTK_MODULES":str(self.waybar_dir / "gtk.so"), "BARTJIK_GTK_GEOMETRY":str(geometry),"BARTJIK_GTK_TOOLTIPS":str(self.waybar_dir / "tooltips.ini")}
        self.waybar = subprocess.Popen(["waybar", "-c", str(path), "-s", str(self.waybar_dir / "style.css")], env=env, stdout=self.log, stderr=self.log, start_new_session=True)
        marker = "tray-2 " if scene[0] == "applications" else "custom-"
        wait_for(lambda: geometry.exists() and marker in geometry.read_text())
        self.pause(.3)
        size, _ = self.waybar_geometry()
        # Center both bars at y=570 without changing either renderer's height.
        self.waybar_bottom = round(680 - 570 - size[1] / 2)
        config = json.loads(path.read_text())
        config["margin-bottom"] = self.waybar_bottom
        self.waybar.terminate(); self.waybar.wait(timeout=3)
        path.write_text(json.dumps(config))
        geometry.unlink(missing_ok=True)
        self.waybar = subprocess.Popen(["waybar", "-c", str(path), "-s", str(self.waybar_dir / "style.css")], env=env, stdout=self.log, stderr=self.log, start_new_session=True)
        wait_for(lambda: geometry.exists() and marker in geometry.read_text())
        self.pause(1)

    def waybar_geometry(self):
        lines = (self.waybar_dir / "geometry").read_text().splitlines()
        size = next(list(map(int, line.split()[2:])) for line in lines if line.startswith("window waybar "))
        return size, {line.split()[0]:list(map(int,line.split()[1:])) for line in lines if line.startswith(("custom-","ai ","tray ","tray-"))}

    def stage(self, **data):
        self.ipc({"op": "demoStage", **data})

    def move_pointer(self, x, y):
        self.pointer_process.stdin.write(f"{round(x)} {round(y)}\n")
        self.pointer_process.stdin.flush()
        if self.pointer_process.stdout.readline().strip() != "ok":
            raise ValueError("Virtual pointer failed")
        self.pointer_position = (x, y)

    def glide_pointer(self, x, y):
        origin = self.pointer_position or (x, y)
        started = time.monotonic()
        while True:
            progress = min(1, (time.monotonic() - started) / .7)
            t = progress * progress * (3 - 2 * progress)
            self.move_pointer(origin[0] + (x-origin[0])*t, origin[1] + (y-origin[1])*t)
            if progress == 1:
                return
            time.sleep(1/60)

    def capture(self, name):
        self.check_output()
        path = self.waybar_dir.parent / (name + ".png")
        checked(["grim", "-o", self.monitor, str(path)])
        return path

    def check_output(self):
        monitor=next(m for m in json.loads(checked(["hyprctl","-j","monitors"])) if m["name"]==self.monitor)
        if (monitor["width"],monitor["height"],monitor["scale"]) != (1000,680,1):
            raise ValueError("Recording output changed size or scale; refusing a misaligned take")

    def preflight(self):
        montage = []
        for scene in self.scenes:
            self.set_config(demo_config(self.monitor, scene))
            self.set_waybar(scene)
            self.move_pointer(20,640)
            size, widgets = self.waybar_geometry()
            layout = self.ipc({"op":"demoHover", "island":scene[0]})
            assert abs(layout["x"]+layout["width"]/2-730)<1, layout
            full = self.capture("source-"+scene[0])
            left=min(v[0] for v in widgets.values());right=max(v[0]+v[2] for v in widgets.values())-self.waybar_left
            assert abs(self.waybar_left+(left+right)/2-270)<=1, (scene,left,right,self.waybar_left)
            for lane, x, y, w, h in ((270,self.waybar_left+left,680-self.waybar_bottom-size[1],right-left,size[1]), (730,layout["x"],layout["y"],layout["width"],layout["height"])):
                path = self.waybar_dir.parent / f"montage-{scene[0]}-{lane}.png"
                checked(["ffmpeg","-v","error","-i",str(full),"-vf",f"crop={int(w)}:{int(h)}:{round(x)}:{round(y)}","-frames:v","1",str(path)])
                montage.append({"path":"file://"+str(path),"x":lane})
        self.waybar.terminate(); self.waybar.wait(timeout=3); self.waybar=None
        self.ipc({"op":"hide","bar":"all"})
        self.stage(montage=montage, intro=False)

    def modules(self, base):
        start = time.monotonic() - self.started
        self.stage(title="Waybar vs Bartjik", caption="All islands, built from the current Waybar layout", counter="", intro=True)
        self.pause(3)
        self.capture("verify-intro")
        self.pause(2)
        self.timeline.append({"scene": "intro", "title": "Waybar and Bartjik — the same desktop islands", "start": round(start, 3), "end": round(time.monotonic() - self.started, 3)})
        for index, scene in enumerate(self.scenes, 1):
            start = time.monotonic() - self.started
            self.move_pointer(20,640)
            self.stage(curtain=True)
            self.pause(.2)
            self.stage(title=scene_label(base, scene), caption=CAPTIONS[scene[0]], counter=f"{index:02d} / {len(SCENES):02d}", intro=False, x=-40, y=-40)
            self.set_config(demo_config(self.monitor, scene))
            self.ipc({"op":"show","bar":"all"})
            self.set_waybar(scene)
            self.stage(curtain=False)
            self.pause(2)
            layout = self.ipc({"op":"demoHover","island":scene[0]})
            size, widgets = self.waybar_geometry()
            targets=list(enumerate(layout["targets"]))
            if scene[0]=="connectivity":
                vpn_index=next(i for i,t in targets if t["module"]=="vpn")
                targets.append((vpn_index,{**layout["targets"][vpn_index],"module":"tailnet"}))
            for hover_index, (target_index, target) in enumerate(targets):
                name = waybar_name(target["module"]).replace("/","-")
                if target["module"]=="tray":
                    name="tray-"+tray_fixtures()[target_index][0]
                    if name not in widgets:
                        raise ValueError("Native tray tooltip identity is not ready: " + name)
                if name not in widgets: continue
                x,y,w,h=widgets[name]
                peers=[t for t in layout["targets"] if t["module"]==target["module"]]
                target_x = w/2 if len(peers)<=1 or target["module"]=="tray" else w*(peers.index(target)+.5)/len(peers)
                self.ipc({"op":"demoHover","island":scene[0],"index":target_index})
                pointer_x,pointer_y=self.waybar_left+x+target_x,680-self.waybar_bottom-size[1]+y+h/2
                if hover_index == 0:
                    self.move_pointer(pointer_x,pointer_y)
                else:
                    self.glide_pointer(pointer_x,pointer_y)
                # QtTest may warp the seat while a popup maps. Keep the real
                # GTK pointer on its paired target without resetting a stable hover.
                until=time.monotonic()+3*max(1,self.speed)
                while time.monotonic()<until:
                    position=json.loads(checked(["hyprctl","-j","cursorpos"]))
                    if abs(position["x"]-10000-pointer_x)>1 or abs(position["y"]-10000-pointer_y)>1:
                        self.move_pointer(pointer_x,pointer_y)
                    time.sleep(.15)
                (self.waybar_dir.parent / f"pointer-{scene[0]}-{target_index}.json").write_text(json.dumps({"requested":[pointer_x,pointer_y],"actual":json.loads(checked(["hyprctl","-j","cursorpos"]))}))
                self.capture(f"verify-{scene[0]}-{target_index}")
                wait_for(lambda:"GtkTooltipWindow" in (self.waybar_dir / "geometry").read_text(),timeout=3)
                self.evidence.append({"scene":scene[0],"target":target,"waybarTooltip":True,"bartjikCenter":layout["x"]+layout["width"]/2})
            self.ipc({"op":"demoHover","island":scene[0],"leave":True})
            self.move_pointer(20,640)
            self.stage(x=-40,y=-40)
            self.pause(1.5)
            self.timeline.append({"scene": scene[0], "title": scene_label(base, scene), "start": round(start, 3), "end": round(time.monotonic() - self.started, 3)})
            print(f"{index:02d}/{len(SCENES):02d} {scene[0]}", flush=True)

    def persistence(self):
        if self.waybar:
            self.waybar.terminate();self.waybar.wait(timeout=3);self.waybar=None
        config=demo_config(self.monitor)
        config["bars"]=config["bars"][:1]
        self.stage(finale=True,x=-40,y=-40)
        self.set_config(config)
        self.stage(title="Persistent attached panel", caption="Bitcoin → player → workspaces · one attached surface", counter="", x=-40, y=-40)
        start = time.monotonic() - self.started
        self.ipc({"op":"demoHover","island":"markets","index":0})
        wait_for(lambda: self.ipc({"op": "cryptoState"})["opened"])
        self.pause(4)
        self.ipc({"op":"demoHover","island":"media","index":0})
        wait_for(lambda: self.ipc({"op": "detailState"})["panel"]["opened"])
        self.pause(4)
        self.ipc({"op":"demoHover","island":"workspaces","index":2})
        wait_for(lambda: self.ipc({"op": "workspacePreviewState"})["preview"]["opened"])
        self.pause(4)
        self.capture("verify-persistence")
        self.timeline.append({"scene": "persistent-panel", "title": "Crypto to player to workspaces", "start": round(start, 3), "end": round(time.monotonic() - self.started, 3)})
        print("persistent-panel", flush=True)

    def widgets(self):
        start = time.monotonic() - self.started
        self.stage(title="Detached crypto widgets", caption="Drag each crypto graph away to keep it on the desktop",counter="",x=-40,y=-40)
        for index, pair in enumerate(("BTC/USD", "SOL/USD")):
            self.ipc({"op":"demoHover","island":"markets","index":index})
            wait_for(lambda: (lambda state: state["opened"] and state["pair"] == pair and state["points"] >= 2)(self.ipc({"op": "cryptoState"})))
            self.pause(2)
            self.ipc({"op":"demoDetach","x":50 if index==0 else 510,"y":180})
            self.ipc({"op":"demoHover","island":"markets","leave":True})
            wait_for(lambda: any(card["pair"] == pair and card["detached"] for card in self.ipc({"op": "cryptoPool"})["cards"]))
            self.pause(4)
        self.stage(x=-40,y=-40)
        wait_for(lambda:not any(card["opened"] and not card["detached"] for card in self.ipc({"op":"cryptoPool"})["cards"]))
        self.pause(2)
        self.capture("verify-widgets")
        self.timeline.append({"scene": "detached-widgets", "title": "Detached crypto graph widgets", "start": round(start, 3), "end": round(time.monotonic() - self.started, 3)})
        print("detached-widgets", flush=True)

    def close(self):
        if self.waybar:
            self.waybar.terminate(); self.waybar.wait(timeout=3); self.waybar = None
        self.pointer_process.stdin.close()
        self.pointer_process.wait(timeout=3)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=("cycle", "record"))
    parser.add_argument("output", nargs="?", type=Path)
    parser.add_argument("--speed", type=float, default=1.4, help="Playback rate; 1.4 is 40 percent faster")
    parser.add_argument("--plan", action="store_true")
    parser.add_argument("--scene",choices=[scene[0] for scene in SCENES],help="Record one island for visual verification")
    args = parser.parse_args(argv)
    if not 1 <= args.speed <= 4:
        parser.error("--speed is a playback rate between 1 and 4")
    if args.mode == "record" and not args.output:
        parser.error("record requires OUTPUT.mp4")
    if args.mode == "cycle" and args.output:
        parser.error("cycle takes no output path")
    if args.plan:
        print("\n".join([*(scene[0] for scene in SCENES), "persistent-panel", "detached-widgets"]))
        return 0
    output = args.output.absolute() if args.output else None
    if output and (output.suffix.lower() != ".mp4" or output.exists() or output.with_suffix(".chapters.json").exists()):
        parser.error("choose a new .mp4 output file; existing files are never overwritten")
    for command in ("dbus-daemon", "Hyprland", "hyprctl", "quickshell", "waybar", "wf-recorder", "ffmpeg", "ffprobe", "grim", "cc", "pkg-config", "wayland-scanner"):
        if not shutil.which(command):
            parser.error("required command missing: " + command)
    recorder = None
    partial = None
    try:
        with private_bus(), private_compositor(), output_monitor() as monitor:
            td = tempfile.mkdtemp(prefix="bartjik-cycle-")
            print("Capture diagnostics: " + td, flush=True)
            temp = Path(td)
            qml = temp / "qml"
            waybar_dir = temp / "waybar"
            waybar_dir.mkdir()
            build_helpers(waybar_dir)
            shutil.copytree(ROOT / "qml", qml)
            prepare(qml)
            config_path = temp / "config.json"
            config_path.write_text(json.dumps(demo_config(monitor)))
            env = os.environ.copy()
            env.update(BARTJIK_QML=str(qml), BARTJIK_CONFIG=str(config_path), BARTJIK_DEMO="1", BARTJIK_HOME=str(ROOT), BARTJIK_DEMO_MONITOR=monitor)
            with (temp / "bartjik.log").open("w") as log:
                process = subprocess.Popen([str(ROOT / "bin/bartjik"), "--demo"], env=env, stdout=log, stderr=log)
                previous = {key: os.environ.get(key) for key in ("BARTJIK_QML", "BARTJIK_CONFIG")}
                os.environ.update(BARTJIK_QML=str(qml), BARTJIK_CONFIG=str(config_path))
                try:
                    cycle = Cycle(qml, config_path, monitor, 1, waybar_dir, log)
                    if args.scene:cycle.scenes=tuple(scene for scene in SCENES if scene[0]==args.scene)
                    wait_for(lambda: cycle.ipc({"op": "status"}).get("ready"))
                    cycle.preflight()
                    cycle.check_output()
                    if output:
                        output.parent.mkdir(parents=True, exist_ok=True)
                        fd, filename = tempfile.mkstemp(prefix="." + output.stem + "-", suffix=".mkv", dir=output.parent)
                        os.close(fd)
                        partial = Path(filename)
                        Path(partial).unlink()
                        recorder = subprocess.Popen(["wf-recorder", "-D", "--no-dmabuf", "-o", monitor, "-r", "60", "-c", "libx264rgb", "-p", "preset=ultrafast", "-p", "crf=0", "-x", "bgr0", "-f", str(partial)], stdout=log, stderr=log)
                        time.sleep(.2)
                    base = json.loads((ROOT / "examples/comparison.json").read_text())
                    cycle.started = time.monotonic()
                    cycle.modules(base)
                    if not args.scene:
                        cycle.persistence()
                        cycle.widgets()
                    if recorder:
                        recorder.send_signal(signal.SIGINT); recorder.wait(timeout=15); recorder = None
                        playable = temp / "showcase.mp4"
                        checked(["ffmpeg", "-v", "error", "-i", str(partial), "-vf", f"setpts=(PTS-STARTPTS)/{args.speed},fps=60", "-c:v", "libx264", "-preset", "slow", "-crf", "14", "-pix_fmt", "yuv420p", "-movflags", "+faststart", str(playable)])
                        for chapter in cycle.timeline:
                            for key in ("start","end"):chapter[key]=round(chapter[key]/args.speed,3)
                        shutil.copyfile(playable, output)
                        sources={name:hashlib.sha256((Path.home()/".config/waybar"/name).read_bytes()).hexdigest() for name in ("config","style.css","colors.css")}
                        output.with_suffix(".chapters.json").write_text(json.dumps({"synthetic": True, "scenes": cycle.timeline,"hoverChecks":cycle.evidence,"waybarSourceSHA256":sources,"diagnostics":str(temp)}, indent=2) + "\n")
                        print("Recording: " + str(output))
                finally:
                    if 'cycle' in locals(): cycle.close()
                    if recorder:
                        recorder.send_signal(signal.SIGINT); recorder.wait(timeout=5)
                    process.terminate(); process.wait(timeout=5)
                    for key, value in previous.items():
                        if value is None: os.environ.pop(key, None)
                        else: os.environ[key] = value
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        print("bartjik demo: " + str(error), file=sys.stderr)
        return 1
    finally:
        if partial:
            partial.unlink(missing_ok=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
