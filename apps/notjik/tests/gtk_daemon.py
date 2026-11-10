"""Native notification server acceptance; private D-Bus, Wayland and synthetic data."""
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT.parent / "runtjik/tests"))
from private_compositor import private_compositor, wait_for


def motion_acceptance(request, send, config, env, work, daemon):
    """Exercise actual popup allocations, pointer playback and background dismissal."""
    request({"op":"dismiss", "id":"all"})
    time.sleep(.4)
    subprocess.run(["grim","-o","NOTJIK",str(work/"motion-empty.png")],check=True,capture_output=True)
    config.write_text(json.dumps({"theme":"rose-punk", "placement":{"monitors":"all","edge":"b","position":"r"}, "grouping":{"enabled":False}, "stack":{"animationMs":500}}))
    time.sleep(1.3)
    old=send("Older card stays attached",id="motion-old",body="Moves down after the lower card leaves.",actions=[{"identifier":"default","label":"Open","command":["true"]}],timeout=0)
    fixture=json.loads((ROOT/"examples/demo-fixtures.json").read_text())["telegram-voice"]["notices"][0]
    fixture=json.loads(json.dumps(fixture).replace("NOTJIK_ROOT",str(ROOT/"dist")))
    fixture.update(app="notjik demo",id="motion-voice",timeout=0,transient=True)
    voice=request({"op":"send","notice":fixture})["id"]
    time.sleep(1)
    trace=work/"layout.jsonl"
    def frames():
        values=[]
        for line in trace.read_text().splitlines():
            try: values.append(json.loads(line))
            except ValueError: pass  # The writer may be in its next frame.
        return values
    def row(frame,key): return next((r for r in frame['rows'] if r['id']=="card-"+key),None)
    def last(): return next(f for f in reversed(frames()) if row(f,voice))
    baseline=last()
    initial=row(baseline,voice)['height']
    request({"op":"navigate","direction":"previous"})
    wait_for(lambda: request({"op":"status"})["selectedId"]==voice)
    time.sleep(.4)
    expanded=last()
    assert row(expanded,voice)['height'] > initial, (baseline,expanded)
    def check_join(frame):
        a,b=row(frame,old),row(frame,voice)
        if a and b and not b['retiring'] and a['extent']==b['extent']==1:
            assert abs(a['y']+a['height']+10-b['y']) <= 1, frame
    for frame in frames(): check_join(frame)
    protocol=ROOT.parent/"runtjik/tests/virtual_pointer.xml"
    subprocess.run(["wayland-scanner","client-header",str(protocol),str(work/"pointer.h")],check=True)
    subprocess.run(["wayland-scanner","private-code",str(protocol),str(work/"protocol.c")],check=True)
    flags=subprocess.check_output(["pkg-config","--cflags","--libs","wayland-client"],text=True).split()
    subprocess.run(["cc",str(ROOT.parent/"runtjik/tests/pointer.c"),str(work/"protocol.c"),"-I"+str(work),"-o",str(work/"pointer"),*flags],check=True)
    pointer=subprocess.Popen([str(work/"pointer"),"NOTJIK"],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
    def event(command):
        pointer.stdin.write(command+"\n"); pointer.stdin.flush()
        assert pointer.stdout.readline().strip()=="ok"
    def move(x,y): event(f"m {round(x*10)} {round(y*10000/900)}")
    def click():
        event("b 1"); time.sleep(.03); event("b 0")
    layer=next(c for m in json.loads(subprocess.check_output(["hyprctl","-j","layers"],text=True)).values() for rows in m['levels'].values() for c in rows if c['namespace']=='notjik')
    layer.update(x=expanded['x'],y=expanded['y'])
    try:
        move(layer['x']+122, layer['y']+row(expanded,voice)['y']+75)
        click()
        wait_for(lambda: subprocess.run(["pgrep","-P",str(daemon.pid),"-x","ffplay"],capture_output=True).returncode==0)
        start=time.time()*1000
        time.sleep(1.2)
        playing=[f for f in frames() if f['time']>=start and row(f,voice)]
        heights=[row(f,voice)['height'] for f in playing]
        assert heights and max(heights)-min(heights)<=1, heights
        for frame in playing: check_join(frame)
        subprocess.run(["grim","-o","NOTJIK",str(work/"motion-playing.png")],check=True,capture_output=True)
        # Move to plain title text, then release: this must dismiss, not select.
        frame=last()
        before_old=row(frame,old)['y']
        move(layer['x']+190,layer['y']+row(frame,voice)['y']+24)
        click()
        wait_for(lambda: all(n['id']!=voice for n in request({"op":"list"})['notices']))
        time.sleep(.12)
        subprocess.run(["grim","-o","NOTJIK",str(work/"motion-departing.png")],check=True,capture_output=True)
        time.sleep(.6)
        departures=[f for f in frames() if row(f,voice) and row(f,voice)['retiring']]
        assert departures, "Missing dismissal animation"
        assert any(0 < row(f,voice)['departure'][0] < 1 and row(f,voice)['extent']==1 for f in departures)
        assert any(row(f,voice)['departure'][0]==0 and 0 < row(f,voice)['extent'] < 1 for f in departures)
        final=next(f for f in reversed(frames()) if row(f,old) and not row(f,voice))
        assert row(final,old)['y'] > before_old+50, (before_old,final)
        assert all(f['height']==expanded['height'] for f in departures), "Compositor surface resized during dismissal"
        subprocess.run(["grim","-o","NOTJIK",str(work/"motion-settled.png")],check=True,capture_output=True)
        # Drag plain card text; the live preview must move before button release.
        def layers():
            # Detached cards become native toplevels after drop. Their stack trace
            # coordinates are local; compositor geometry is the global authority.
            clients = json.loads(subprocess.check_output(["hyprctl", "-j", "clients"], text=True))
            native = next((c for c in clients if c.get("pid") == daemon.pid and c.get("title", "").startswith("notjik-detached:")), None)
            if native:
                frame = next((f for f in reversed(frames()) if row(f, old) and f['surface'].startswith('floating:')), None)
                if frame:
                    monitors = json.loads(subprocess.check_output(["hyprctl", "-j", "monitors"], text=True))
                    monitor = next((m['name'] for m in monitors if m['id'] == native['monitor']), None)
                    return [{**frame, 'x': native['at'][0], 'y': native['at'][1],
                             'monitor': monitor, 'nativeAddress': native['address']}]
            result = []
            for frame in frames()[-150:]:
                card = row(frame, old)
                if not card:
                    continue
                if card.get('lifted'):
                    frame = {**frame, 'x': frame['x'] + card['liftX'], 'y': frame['y'] + card['y']}
                result.append(frame)
            return result
        move(layer['x']+180,layer['y']+row(final,old)['y']+24)
        event("b 1"); time.sleep(.04); event("r -150 -160")
        try:
            preview=wait_for(lambda: next((c for c in layers() if abs(c['x']-(layer['x']-150))<3),None),timeout=4)
        except ValueError:
            raise AssertionError({"layers":layers(),"status":request({"op":"status"}),"cursor":subprocess.check_output(["hyprctl","-j","cursorpos"],text=True),"log":(work/"daemon.log").read_text()})
        assert preview['surface'].startswith('floating:'), "Drag did not leave the original surface"
        source = next(f for f in reversed(frames()) if f.get('surface')=='NOTJIK')
        assert not row(source, old), "Last dragged card left an original copy"
        time.sleep(.15)
        subprocess.run(["grim","-o","NOTJIK",str(work/"drag-single-held.png")],check=True,capture_output=True)
        from PIL import Image, ImageChops, ImageStat
        box=(layer['x'],layer['y']+int(row(final,old)['y']),layer['x']+int(final['width']),layer['y']+int(row(final,old)['y']+row(final,old)['height']))
        delta=ImageChops.difference(Image.open(work/"motion-empty.png").convert('RGB').crop(box),Image.open(work/"drag-single-held.png").convert('RGB').crop(box))
        assert max(ImageStat.Stat(delta).mean)<3, "Original card is still visible in the empty notification area"
        assert row(preview, old)['widget'] == row(final, old)['widget'], "Drag cloned the widget"
        assert request({"op":"status"})["queued"]==1, "Drag dismissed the card"
        assert request({"op":"status"})["floating"]==0, "Drop committed before release"
        # A lifted widget reserves no space for newly arriving neighbours.
        sibling = send("Neighbour fills the stack", id="drag-neighbour", timeout=0)
        def reflowed():
            for frame in reversed(frames()[-150:]):
                neighbour = row(frame, sibling)
                if neighbour and frame.get('surface')=='NOTJIK' and neighbour['extent'] == 1:
                    assert abs(neighbour['y'] + neighbour['height'] - frame['height']) <= 1, frame
                    assert sum(r['id'] == 'card-' + old for r in frame['rows']) == 0, frame
                    return True
            return False
        wait_for(reflowed)
        request({"op":"dismiss", "id":sibling})
        wait_for(lambda: request({"op":"status"})['queued'] == 1)
        event("r -55 -25")
        try:
            moved=wait_for(lambda: next((c for c in layers() if abs(c['x']-(preview['x']-55))<3),None),timeout=4)
        except ValueError:
            raise AssertionError({"preview":preview,"latest":layers()[-4:],"status":request({"op":"status"}),"cursor":subprocess.check_output(["hyprctl","-j","cursorpos"],text=True)})
        assert abs(moved['y']-(preview['y']-25))<3,(preview,moved)
        subprocess.run(["grim","-o","NOTJIK",str(work/"drag-held.png")],check=True,capture_output=True)
        event("b 0")
        wait_for(lambda: request({"op":"status"})["floating"]==1)
        time.sleep(.2)
        # Re-drag a detached card. Offsets are relative to its stationary grab
        # surface, not accumulated again after each window movement.
        moved = wait_for(lambda: next((f for f in layers() if f.get('nativeAddress')), None))
        move(moved['x']+8,moved['y']+8)
        event("b 1"); time.sleep(.04); event("r 45 -35")
        again=wait_for(lambda: next((c for c in layers() if abs(c['x']-(moved['x']+45))<3 and abs(c['y']-(moved['y']-35))<3),None))
        event("b 0"); time.sleep(.2)
        assert request({"op":"status"})["floating"]==1
        assert request({"op":"status"})["queued"]==1
        move(again['x']+280,again['y']+24)
        time.sleep(.2)
        settled=layers()[-1]
        assert row(settled, old)['widget'] == row(final, old)['widget'], "Drop rebuilt the widget"
        assert abs(settled['x']-again['x'])<3 and abs(settled['y']-again['y'])<3, (again,settled)
        subprocess.run(["grim","-o","NOTJIK",str(work/"drag-dropped.png")],check=True,capture_output=True)
        # Leaving the pinned card collapses its footer; switching workspace hides
        # the layer without dismissing its notification, then restores it.
        expanded_height = row(settled, old)['height']
        move(10, 10)
        wait_for(lambda: row(layers()[-1], old)['height'] < expanded_height)
        collapsed = layers()[-1]
        assert abs(collapsed['x']-settled['x'])<3 and abs(collapsed['y']-settled['y'])<3, (settled, collapsed)
        def popup_count():
            outputs = json.loads(subprocess.check_output(["hyprctl", "-j", "layers"], text=True))
            layers_count = sum(1 for output in outputs.values() for entries in output['levels'].values()
                               for entry in entries if entry.get('namespace') == 'notjik')
            clients = json.loads(subprocess.check_output(["hyprctl", "-j", "clients"], text=True))
            workspace = json.loads(subprocess.check_output(["hyprctl", "-j", "activeworkspace"], text=True))['id']
            return layers_count + sum(c.get('pid') == daemon.pid and c.get('title', '').startswith('notjik-detached:')
                                      and c['workspace']['id'] == workspace and c.get('mapped', True) for c in clients)
        original_workspace = json.loads(subprocess.check_output(["hyprctl", "-j", "activeworkspace"], text=True))['id']
        visible_count = popup_count()
        subprocess.run(["hyprctl", "eval", 'hl.dispatch(hl.dsp.focus({workspace="98"}))'], check=True, capture_output=True)
        wait_for(lambda: popup_count() < visible_count)
        assert request({"op":"status"})["floating"] == 1
        subprocess.run(["hyprctl", "eval", f'hl.dispatch(hl.dsp.focus({{workspace="{original_workspace}"}}))'], check=True, capture_output=True)
        wait_for(lambda: popup_count() == visible_count)
        subprocess.run(["grim", "-o", "NOTJIK", str(work / "pinned-idle.png")], check=True, capture_output=True)
        # Cross to a differently scaled output using real relative pointer events.
        subprocess.run(["hyprctl","output","create","headless","NOTJIK2"],check=True,capture_output=True)
        subprocess.run(["hyprctl","eval",'hl.monitor({output="NOTJIK2",mode="1600x1200@60",position="1000x0",scale=1.25})'],check=True,capture_output=True)
        wait_for(lambda: any(m['name']=='NOTJIK2' and m['x']==1000 and m['width']==1280 and m['height']==960 for m in request({"op":"status"})['monitors']))
        def latest_card():
            return layers()[-1]
        def global_move(x,y):
            cursor=json.loads(subprocess.check_output(["hyprctl","-j","cursorpos"],text=True))
            event(f"r {x-cursor['x']} {y-cursor['y']}")
            time.sleep(.12)
        position=latest_card()
        global_move(position['x']+8,position['y']+8)
        subprocess.run(['grim','-o','NOTJIK',str(work/'cross-before-pickup.png')],check=True,capture_output=True)
        event('b 1'); time.sleep(.04); event('r 900 -80')
        try:
            crossed=wait_for(lambda: (f if f.get('monitor') == 'NOTJIK2' and f.get('nativeAddress') == position.get('nativeAddress') else None) if (f := latest_card()) else None, timeout=5)
        except ValueError:
            raise AssertionError({"position":position,"status":request({"op":"status"}),"cursor":json.loads(subprocess.check_output(["hyprctl","-j","cursorpos"],text=True)),"outputs":json.loads(subprocess.check_output(["hyprctl","-j","monitors"],text=True)),"layers":json.loads(subprocess.check_output(["hyprctl","-j","layers"],text=True)),"frames":frames()[-4:]})
        assert row(crossed,old)['widget']==row(final,old)['widget']
        assert request({"op":"status"})['queued']==1
        subprocess.run(["grim","-o","NOTJIK2",str(work/"drag-cross-monitor-held.png")],check=True,capture_output=True)
        event('b 0'); time.sleep(.3)
        assert request({"op":"status"})['floating']==1
        position=latest_card()
        global_move(1800,100)
        time.sleep(.2)
        released=latest_card()
        assert abs(released['x']-position['x'])<2 and abs(released['y']-position['y'])<2, 'Cross-output drop did not release the card'
        global_move(position['x']+8,position['y']+8)
        event('b 1'); time.sleep(.04); event('r -900 80')
        returned=wait_for(lambda: (f if f.get('monitor') == 'NOTJIK' and f.get('nativeAddress') == position.get('nativeAddress') else None) if (f := latest_card()) else None, timeout=5)
        event('b 0'); time.sleep(.3)
        assert row(returned,old)['widget']==row(final,old)['widget']
        # Removing an output during a drag cancels the grab without dropping data.
        position=latest_card()
        # Re-enter after the compositor-owned grab before starting another one.
        global_move(position['x']+24,position['y']+8)
        global_move(position['x']+8,position['y']+8)
        event('b 1'); time.sleep(.04); event('r 900 -80')
        try:
            wait_for(lambda: latest_card().get('monitor')=='NOTJIK2',timeout=5)
        except ValueError:
            raise AssertionError({"pickup": position, "latest": latest_card(),
                                  "cursor": json.loads(subprocess.check_output(["hyprctl", "-j", "cursorpos"], text=True)),
                                  "clients": json.loads(subprocess.check_output(["hyprctl", "-j", "clients"], text=True)),
                                  "outputs": json.loads(subprocess.check_output(["hyprctl", "-j", "monitors"], text=True))})
        subprocess.run(["hyprctl","output","remove","NOTJIK2"],check=True,capture_output=True)
        wait_for(lambda: len(request({"op":"status"})['monitors'])==1)
        event('b 0'); time.sleep(.3)
        assert request({"op":"status"})['queued']==1
        assert row(latest_card(),old)['widget']==row(final,old)['widget']
        print("Cross-monitor drag: mixed scale, round trip, same widget and output removal during grab passed")
        def control_click(x,y):
            global_move(x,y)
            keyboard=subprocess.Popen(["wtype","-M","ctrl","-s","600","-m","ctrl"],env=env)
            time.sleep(.15); click(); keyboard.wait(timeout=3)
        sibling=send("Ctrl-click dismisses the queue",id="ctrl-sibling",timeout=0)
        time.sleep(.3)
        position=latest_card()
        control_click(position['x']+180,position['y']+24)
        wait_for(lambda: request({"op":"status"})['queued']==0)
        plain=send("Keep this ordinary card",id="ctrl-keep",timeout=0)
        telegram=request({"op":"send","notice":{"id":"ctrl-telegram","app":"notjik demo","title":"Synthetic Ctrl safe read","type":"telegram","timeout":0,"data":{"kind":"text","demo":True,"chatId":"17","messageId":"23"}}})['id']
        time.sleep(.5)
        frame=next(f for f in reversed(frames()) if row(f,telegram))
        control_click(frame['x']+180,frame['y']+row(frame,telegram)['y']+24)
        wait_for(lambda: request({"op":"status"})['queued']==1)
        assert request({"op":"list"})['notices'][0]['id']==plain
        time.sleep(.6)  # Let the dismissed neighbour finish collapsing before aiming.
        frame=next(f for f in reversed(frames()) if row(f,plain))
        global_move(frame['x']+180,frame['y']+row(frame,plain)['y']+24)
        keyboard=subprocess.Popen(["wtype","-M","shift","-s","600","-m","shift"],env=env)
        time.sleep(.15); click(); keyboard.wait(timeout=3)
        assert request({"op":"status"})['queued']==1, "Shift-click dismissed the card"
        copied=subprocess.check_output(["wl-paste","--no-newline"],env=env,text=True,timeout=3)
        assert "Keep this ordinary card" in copied
        print("Native Ctrl-click: ordinary dismiss-all and Telegram single-card safe-read intent passed")

        # Real typing must remain in another app on arrival, hover, replacement,
        # grouped updates and remapping an empty notification surface.
        request({"op":"dismiss","id":"all"})
        request({"op":"unfocus"})
        time.sleep(.6)
        config.write_text(json.dumps({"theme":"rose-punk", "placement":{"monitors":"all","edge":"b","position":"r"}, "grouping":{"enabled":True}, "stack":{"animationMs":400}}))
        time.sleep(1.3)
        move(100,100)
        probe_file=work/"focus-probe.json"
        probe=subprocess.Popen(["/usr/bin/python3",str(ROOT/"tests/focus_probe.py"),str(probe_file)],env=env,stdout=(work/"focus-probe.log").open("w"),stderr=subprocess.STDOUT)
        try:
            wait_for(lambda: probe_file.exists() and json.loads(probe_file.read_text())["active"])
            expected=""
            def type_into_probe(text):
                nonlocal expected
                expected+=text
                subprocess.run(["wtype",text],env=env,check=True)
                wait_for(lambda: json.loads(probe_file.read_text())["text"]==expected,timeout=3)
                assert json.loads(probe_file.read_text())["active"], "Notification took keyboard focus"
            type_into_probe("before")
            ids=[]
            for count in range(1,7):
                ids.append(send("Synthetic grouped chat",body="Typing in another app stays uninterrupted.",group="focus-test",timeout=0))
                time.sleep(.65)
                type_into_probe(str(count))
                frame=next(f for f in reversed(frames()) if row(f,ids[0]))
                move(frame['x']+180,frame['y']+row(frame,ids[0])['y']+24)
                time.sleep(.2)
                type_into_probe("hover")
                subprocess.run(["grim","-o","NOTJIK",str(work/f"group-notches-{count}.png")],check=True,capture_output=True)
            send("Synthetic replacement",replace=ids[0],group="focus-test",timeout=0)
            time.sleep(.5)
            type_into_probe("replacement")
            for key in reversed(ids[1:]):
                request({"op":"dismiss","id":key})
                time.sleep(.5)
                type_into_probe("merge")
            request({"op":"dismiss","id":"all"})
            time.sleep(.6)
            send("Remapped popup",timeout=0)
            time.sleep(.4)
            type_into_probe("remap")
            request({"op":"navigate","direction":"previous"})
            wait_for(lambda: not json.loads(probe_file.read_text())["active"])
            subprocess.run(["wtype","-k","Escape"],env=env,check=True)
            wait_for(lambda: json.loads(probe_file.read_text())["active"])
            type_into_probe("escape")
            request({"op":"navigate","direction":"previous"})
            wait_for(lambda: not json.loads(probe_file.read_text())["active"])
            move(100,450); click()
            wait_for(lambda: json.loads(probe_file.read_text())["active"])
            wait_for(lambda: request({"op":"status"})["selectedId"]=="")
            subprocess.run(["wtype","-k","End"],env=env,check=True)
            move(800,810)
            time.sleep(.3)
            type_into_probe("returned")
            print("Keyboard focus: arrivals, hover, group splits/merges and remap preserve typing; explicit selection and Escape passed")
        finally:
            probe.terminate(); probe.wait(timeout=5)

        # Dismissing a group member retains its slot and immediately reveals the
        # successor. The old face is drawn behind it without a new entrance/gap.
        request({"op":"dismiss","id":"all"})
        time.sleep(.6)
        group=[send("Group member "+str(i),body="Same group, immediately available successor.",group="transition",timeout=0) for i in range(3)]
        time.sleep(.7)
        before=next(f for f in reversed(frames()) if row(f,group[0]))
        original=row(before,group[0])
        for index in range(2):
            current=next(f for f in reversed(frames()) if row(f,group[index]))
            global_move(current['x']+180,current['y']+row(current,group[index])['y']+24)
            started=time.time()*1000
            click()
            after=wait_for(lambda: next((f for f in reversed(frames()) if f['time']>=started and row(f,group[index+1])),None),timeout=3)
            incoming=row(after,group[index+1])
            assert incoming['widget']==original['widget'], "Group successor rebuilt the card/notches"
            assert incoming['extent']==1, "Group successor played an entrance"
            assert abs(incoming['y']-original['y'])<1, "Group slot moved during replacement"
            assert len(after['rows'])==1 and not incoming['retiring'], "Outgoing member reserved a second slot"
            assert after.get('groupDepartures'), "Missing departing face behind group successor"
            time.sleep(.08)
            subprocess.run(["grim","-o","NOTJIK",str(work/f"group-successor-{index+1}.png")],check=True,capture_output=True)
            time.sleep(.5)
        print("Grouped dismissal: same widget/slot, immediate full-opacity successor, retained notches and independent departing face passed")

        print("Pinned card: hover footer collapse and workspace hide/restore passed")
        print("Native drag: plain-text pickup, live held movement, release without dismissal and repeated floating drag passed")
        print("Native motion: stable playback footer, frame-synchronous neighbours, background dismissal, recede/fade then collapse passed")
    finally:
        pointer.stdin.close(); pointer.wait(timeout=5)
    request({"op":"dismiss","id":"all"})
    time.sleep(.6)


def main():
    host = dict(os.environ)
    def hypr(*args):
        return subprocess.check_output(["hyprctl", *args], env=host, text=True)
    old = {c["address"] for c in json.loads(hypr("-j", "clients"))}
    rule = f"notjik_native_test_{os.getpid()}"
    result = hypr("eval", f'_G.{rule}=hl.window_rule({{name="{rule}",workspace="7 silent",no_initial_focus=true,no_focus=true,match={{initial_class="^aquamarine$"}}}})')
    assert "error" not in result.lower(), result
    work = Path(tempfile.mkdtemp(prefix="notjik-native-gtk-"))
    try:
        with private_compositor(isolate_pointer=True):
            nested = wait_for(lambda: next((c for c in json.loads(hypr("-j", "clients")) if c["initialClass"] == "aquamarine" and c["address"] not in old), None))
            assert nested["workspace"]["id"] == 7
            assert json.loads(hypr("-j", "activewindow")).get("address") != nested["address"]
            subprocess.run(["hyprctl", "output", "create", "headless", "NOTJIK"], check=True, capture_output=True)
            subprocess.run(["hyprctl", "eval", 'hl.monitor({output="WAYLAND-1",disabled=true}); hl.monitor({output="NOTJIK",mode="1000x900@60",position="0x0",scale=1})'], check=True, capture_output=True)
            config = work / "config.json"
            config.write_text(json.dumps({"theme":"rose-punk", "placement":{"monitors":"all","edge":"b","position":"r","offsetY":0}, "grouping":{"enabled":False}, "history":{"logTransient":True}}))
            bus = subprocess.Popen(["dbus-daemon", "--session", "--nofork", "--print-address"], stdout=subprocess.PIPE, text=True)
            address = bus.stdout.readline().strip()
            env = {**os.environ,"DBUS_SESSION_BUS_ADDRESS":address,"NOTJIK_HOME":str(ROOT / "dist"),"NOTJIK_CONFIG":str(config),"NOTJIK_STATE_DIR":str(work/"state"),"NOTJIK_TELEGRAM_DIR":str(work/"telegram"),"NOTJIK_SOCKET":str(work/"notjik.sock"),"GTK_A11Y":"none","GSK_RENDERER":"cairo","SDL_AUDIODRIVER":"dummy","NOTJIK_GTK_LAYOUT_TRACE":str(work/"layout.jsonl")}
            log = (work/"daemon.log").open("w")
            daemon = subprocess.Popen([str(ROOT/"dist/bin/notjik-core"),"launch"],env=env,stdout=log,stderr=log)
            def request(value):
                with socket.socket(socket.AF_UNIX) as client:
                    client.settimeout(5)
                    client.connect(env["NOTJIK_SOCKET"])
                    client.sendall(json.dumps(value).encode()+b"\n")
                    return json.loads(client.makefile("rb").readline())
            def ready():
                assert daemon.poll() is None, (work/"daemon.log").read_text()
                try:
                    v=request({"op":"status"})
                    return v if v.get("ok") and v.get("lockState")=="unlocked" and v.get("monitors") else None
                except (OSError, ValueError): return None
            def send(title, **kwargs):
                v=request({"op":"send","notice":{"title":title,"app":"notjik demo","data":{"demo":True},**kwargs}})
                assert v["ok"], v
                return v["id"]
            def dbus(code):
                return subprocess.check_output(["/usr/bin/python3","-c","import dbus\nb=dbus.SessionBus()\nn=dbus.Interface(b.get_object('org.freedesktop.Notifications','/org/freedesktop/Notifications'),'org.freedesktop.Notifications')\n"+code],env=env,text=True).strip()
            signals = (work/"signals.log").open("w")
            watcher = subprocess.Popen(["dbus-monitor", "--session", "interface=org.freedesktop.Notifications"], env=env, stdout=signals, stderr=subprocess.DEVNULL)
            try:
                wait_for(ready, timeout=15)
                presence=request({"op":"status"})
                assert isinstance(presence["idleSeconds"], int)
                assert presence["idle"] == (presence["idleSeconds"] >= 120)
                other=subprocess.run([str(ROOT/"dist/bin/notjik-gtk"),"--serve"],env=env,capture_output=True,text=True,timeout=8)
                assert other.returncode != 0 and "already has an owner" in other.stderr, other
                assert "Notjik" in dbus("print(n.GetServerInformation())")
                a=send("Replace me",id="stable",timeout=0)
                assert send("Replacement",id="stable",timeout=0)==a
                assert len(request({"op":"list"})["notices"])==1
                request({"op":"pause"})
                send("Paused expiry",id="timed",timeout=300)
                time.sleep(.6)
                assert request({"op":"status"})["queued"]==2
                request({"op":"resume"})
                wait_for(lambda: request({"op":"status"})["queued"]==1)
                native=int(dbus("print(n.Notify('notjik demo',0,'','D-Bus sample','Synthetic body',['default','Open'],{'resident':True},0))"))
                assert int(dbus(f"print(n.Notify('notjik demo',{native},'','D-Bus replacement','Synthetic body',['default','Open'],{{'resident':True}},0))"))==native
                assert request({"op":"action","id":f"dbus-{native}","index":0})["ok"]
                assert any(n["id"]==f"dbus-{native}" for n in request({"op":"list"})["notices"])
                wait_for(lambda: "member=ActionInvoked" in (work/"signals.log").read_text())
                dbus(f"n.CloseNotification({native})")
                wait_for(lambda: "member=NotificationClosed" in (work/"signals.log").read_text())
                wait_for(lambda: request({"op":"status"})["queued"]==1)
                image=int(dbus("print(n.Notify('notjik demo',0,'','Image-data','Synthetic pixels',[],{'image-data':dbus.Struct((2,2,8,True,8,4,dbus.ByteArray(bytes([255,83,166,255]*4))),signature='iiibiiay')},0))"))
                records=request({"op":"list"})["notices"]
                assert Path(next(n['image'] for n in records if n['id']==f'dbus-{image}')).is_file()
                send("Sign-in verification",type="auth-code",data={"code":"804 216","demo":True},timeout=0)
                time.sleep(1)
                layers=json.loads(subprocess.check_output(["hyprctl","-j","layers"],text=True))
                (work/"layers.json").write_text(json.dumps(layers,indent=2))
                assert "notjik" in json.dumps(layers), layers
                subprocess.run(["grim","-o","NOTJIK",str(work/"popups.png")],check=True,capture_output=True)
                marker = work / "action-result"
                command_id=send("Command action", resident=True, actions=[{"label":"Run synthetic command","command":["/usr/bin/python3","-c",f"from pathlib import Path; Path({str(marker)!r}).write_text('once')"]}])
                assert request({"op":"action","id":command_id,"index":0})["ok"]
                wait_for(lambda: marker.exists())
                assert marker.read_text()=="once"
                cli=subprocess.run([str(ROOT/"dist/bin/notjik-core"),"status"],env=env,capture_output=True,text=True,check=True)
                assert json.loads(cli.stdout)["renderer"]=="gtk"
                native_lib=ROOT/"../../packages/tjik-gtk/target/native/lib"
                lifecycle_env={**env,"PKG_CONFIG_PATH":str(native_lib/"pkgconfig"),"LD_LIBRARY_PATH":str(native_lib)}
                if not os.environ.get("NOTJIK_SKIP_LIFECYCLE"):
                    subprocess.run(["cargo","test","--release","--locked","--features","gtk-ui","--lib","gtk::host::tests::host_lifecycle_preserves_drafts_and_identity","--","--exact","--ignored","--nocapture"],cwd=ROOT,env=lifecycle_env,check=True)
                motion_acceptance(request,send,config,env,work,daemon)
                config.write_text(json.dumps({"theme":"monochrome","placement":{"monitors":"all"},"actions":{"blacklist":["notjik demo"]}}))
                wait_for(lambda: request({"op":"status"})["theme"]=="monochrome")
                history=request({"op":"history-data"})
                assert history["ok"] and history["items"]
                bad=request({"op":"send","notice":{"title":"Invalid","actions":[{"label":"No shell","command":"touch /tmp/notjik-bad"}]}})
                assert not bad["ok"]
                request({"op":"dismiss","id":"all"})
                wait_for(lambda: request({"op":"status"})["queued"]==0)
                print("Native GTK daemon: ownership, IPC, replacement, expiry holds, D-Bus actions/images, config reload, history, validation and layer surfaces passed")
            finally:
                daemon.terminate()
                daemon.wait(timeout=8)
                log.close()
                watcher.terminate()
                watcher.wait(timeout=5)
                signals.close()
                bus.terminate()
                bus.wait(timeout=5)
            output=(work/"daemon.log").read_text()
            assert "panicked" not in output and "Gtk-CRITICAL" not in output and "Gtk-WARNING" not in output, output
            print(f"Native GTK artifacts: {work}")
    finally:
        hypr("eval",f'if _G.{rule} then _G.{rule}:set_enabled(false); _G.{rule}=nil end')

if __name__=="__main__":main()
