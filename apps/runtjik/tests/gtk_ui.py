"""GTK fixtures on an isolated display, with its parent silently on workspace 7."""
from concurrent.futures import ThreadPoolExecutor
from contextlib import contextmanager
import json
import base64
import os
from pathlib import Path
import subprocess
import time

from private_compositor import private_compositor, wait_for

APP = Path(__file__).resolve().parents[1]



@contextmanager
def silent_compositor():
    """Place the dedicated nested window before launch; never move user windows."""
    host = dict(os.environ)
    def hypr(*args):
        return subprocess.check_output(["hyprctl", *args], env=host, text=True)
    clients = json.loads(hypr("-j", "clients"))
    existing = {c["address"] for c in clients}
    workspace = json.loads(hypr("-j", "activeworkspace"))["id"]
    rule = f'runtjik_gtk_test_rule_{os.getpid()}'
    result = hypr("eval", f'_G.{rule}=hl.window_rule({{name="{rule}",workspace="7 silent",no_initial_focus=true,no_focus=true,match={{initial_class="^aquamarine$"}}}})')
    assert "error" not in result.lower(), result
    try:
        with private_compositor():
            assert os.environ["XDG_RUNTIME_DIR"] != host["XDG_RUNTIME_DIR"]
            assert os.environ["HYPRLAND_INSTANCE_SIGNATURE"] != host["HYPRLAND_INSTANCE_SIGNATURE"]
            nested = wait_for(lambda: next((c for c in json.loads(hypr("-j", "clients")) if c["initialClass"] == "aquamarine" and c["address"] not in existing), None))
            assert nested["workspace"]["id"] == 7, nested
            assert json.loads(hypr("-j", "activewindow")).get("address") != nested["address"], "Test compositor stole host focus"
            yield
    finally:
        result = hypr("eval", f'if _G.{rule} then _G.{rule}:set_enabled(false); _G.{rule}=nil end')
        assert "error" not in result.lower(), result
    # The user may change workspaces during a long test. Do not restore their
    # workspace or mistake that interaction for test-window focus theft.
    if json.loads(hypr("-j", "activeworkspace"))["id"] != workspace:
        print("Host workspace changed during test; synthetic window placement/focus checks passed")


def main():
    with silent_compositor():
        root = Path(os.environ["XDG_RUNTIME_DIR"]).parent
        private_runtime = Path(os.environ["XDG_RUNTIME_DIR"])
        private_instance = os.environ["HYPRLAND_INSTANCE_SIGNATURE"]
        assert str(private_runtime).startswith("/tmp/bc-")
        assert (private_runtime / "hypr" / private_instance / ".socket.sock").is_socket()
        def private_hypr(*args):
            # Explicit instance plus private runtime: these cannot address the host.
            return subprocess.check_output(["hyprctl", "--instance", private_instance, *args], text=True,
                                           env={**os.environ, "XDG_RUNTIME_DIR": str(private_runtime)})
        os.environ.update(RUNTJIK_HOME=str(APP), RUNTJIK_CONFIG=str(root / "runtjik.conf"),
                          XDG_STATE_HOME=str(root / "state"), GTK_A11Y="none", GSK_RENDERER="cairo")
        (root / "runtjik.conf").write_text('[theme]\nicon_theme="Adwaita"\n[plugins]\nenabled=["Todo","Words","Clipboard","Recorder","Screenshots"]\n[ui]\nauto_launch_applications=false\nauto_launch_commands=false\n')
        private_hypr("output", "create", "headless", "GTK")
        private_hypr("eval", 'hl.monitor({output="WAYLAND-1",disabled=true}); hl.monitor({output="GTK",mode="1000x680@60",position="0x0",scale=1}); hl.dispatch(hl.dsp.focus({monitor="GTK"}))')
        monitors = wait_for(lambda: [m for m in json.loads(private_hypr("-j", "monitors")) if m["name"] == "GTK"])
        private_hypr("eval", 'hl.monitor({output="GTK",mode="1000x680@60",position="0x0",scale=1}); hl.window_rule({name="gtk-fixture",float=true,no_anim=true,match={title="^Runtjik$"}})')

        install_home = root / "installed-home"
        target = install_home / ".local/lib/runtjik"
        install_env = {**os.environ, "RUNTJIK_INSTALL_HOME": str(install_home),
                       "RUNTJIK_INSTALL_CONFIG": str(root / "installed-config"), "TJIKUP_MANAGED_CONFIG": "1"}
        def install():
            subprocess.run(["bash", str(APP / "scripts/install.sh")], env=install_env,
                           check=True, stdout=subprocess.DEVNULL)
            for name in ["runtjik-core", "runtjik-gtk"]:
                assert (target / name).read_bytes() == (APP / "dist" / name).read_bytes()
        install()
        binary = Path(os.environ.get("RUNTJIK_GTK_TEST_BINARY", str(target / "runtjik-gtk")))
        os.environ["RUNTJIK_HOME"] = str(target)

        def run(action, *args, demo=True):
            return subprocess.check_output([str(binary), action, *args] + (["--demo"] if demo else []), text=True)

        def status():
            return json.loads(run("status"))

        def settled(query, count=None):
            def check():
                state = status()
                return state if state["query"] == query and not state["busy"] and (count is None or len(state["rows"]) == count) else None
            try:
                return wait_for(check)
            except ValueError:
                raise AssertionError((query, count, status()))

        try:
            start = time.monotonic()
            with ThreadPoolExecutor(max_workers=4) as pool:
                list(pool.map(lambda _: run("show"), range(4)))
            initial = settled("", 15)
            assert initial["ready"] and initial["visible"] and initial["renderer"] == "gtk4", initial
            cold_ms = round((time.monotonic() - start) * 1000)
            pid = initial["pid"]
            inode = (target / "runtjik-gtk").stat().st_ino
            install()
            assert (target / "runtjik-gtk").stat().st_ino != inode
            assert status()["pid"] == pid
            wrapper = install_home / ".local/bin/runtjik"
            assert json.loads(subprocess.check_output([str(wrapper), "--gtk", "status", "--demo"], text=True))["pid"] == pid
            assert json.loads(subprocess.check_output([str(wrapper), "status", "--demo"], text=True))["pid"] == pid
            # The non-demo namespace must remain untouched.
            assert not json.loads(run("status", demo=False))["running"]
            for query, count in [("fire", 1), ("zzzzzz", 0), ("", 15), ("/define test", 2), ("/cliphist", 3)]:
                run("test-query", query)
                settled(query, count)
            for query in ["/stats", "/grab", "/rec", "/grab history", "/rec history"]:
                run("test-query", query)
                settled(query)
                assert not status()["backend_error"], status()
            # A burst may produce old backend responses; only the newest page wins.
            for query in ["f", "fire", "zzzz", "fire"]:
                run("test-query", query)
            settled("fire", 1)
            run("test-activate", "launch")
            wait_for(lambda: "Demo activation" in status()["message"])
            run("test-activate", "switch")
            wait_for(lambda: not status()["busy"] and status()["rows"] and status()["rows"][0]["id"].startswith("0x"))
            run("test-activate", "launch")
            wait_for(lambda: "Demo activation" in status()["message"] and not status()["busy"])
            run("test-query", "/todo")
            wait_for(lambda: isinstance(status()["app_state"].get("tasks"), list) and not status()["busy"])
            tasks_before = status()["app_state"]["tasks"]
            run("test-request", json.dumps({"op":"app-action", "provider":"Todo", "action":"create", "fields":{"title":"GTK synthetic task", "kind":"todo", "group":"All"}}))
            wait_for(lambda: any(t["title"] == "GTK synthetic task" for t in status()["app_state"].get("tasks", [])))
            task = next(t for t in status()["app_state"]["tasks"] if t["title"] == "GTK synthetic task")
            run("test-request", json.dumps({"op":"app-action", "provider":"Todo", "action":"toggle", "id":task["id"], "revision":task["revision"], "period":task["period"]}))
            wait_for(lambda: any(t["id"] == task["id"] and t["done"] for t in status()["app_state"]["tasks"]))
            run("test-request", json.dumps({"op":"app-action", "provider":"Todo", "action":"edit", "id":task["id"], "revision":task["revision"], "fields":{"title":"Must reject stale revision"}}))
            wait_for(lambda: "changed" in status()["message"].lower() or "revision" in status()["message"].lower())
            assert not any(t["title"] == "Must reject stale revision" for t in status()["app_state"]["tasks"])
            subprocess.run(["wtype", "-M", "ctrl", "-k", "n", "-m", "ctrl"], check=True)
            time.sleep(.15)
            subprocess.run(["wtype", "Created with GTK keyboard"], check=True)
            subprocess.run(["wtype", "-M", "ctrl", "-k", "Return", "-m", "ctrl"], check=True)
            wait_for(lambda: any(t["title"] == "Created with GTK keyboard" for t in status()["app_state"].get("tasks", [])))
            subprocess.run(["grim", str(root / "gtk-tasks.png")], check=True)
            run("test-query", "")
            settled("", 15)
            # Exercise actual GTK input, not only the control protocol.
            subprocess.run(["wtype", "fire"], check=True)
            settled("fire", 1)
            subprocess.run(["wtype", "-M", "shift", "-k", "Return", "-m", "shift"], check=True)
            wait_for(lambda: "Demo activation" in status()["message"])
            subprocess.run(["wtype", "-k", "Escape"], check=True)
            run("show")
            settled("", 15)
            previous_selection = status()["selected"]
            subprocess.run(["wtype", "-k", "Down"], check=True)
            wait_for(lambda: status()["selected"] == (previous_selection + status()["columns"]) % 15)
            image = root / "gtk-launcher.png"
            private_hypr("dismissnotify")
            time.sleep(.2)
            layers = json.loads(private_hypr("-j", "layers"))
            client = next(c for monitor in layers.values() for rows in monitor["levels"].values() for c in rows if c["namespace"] == "runtjik-gtk")
            geometry = f'{client["x"]},{client["y"]} {client["w"]}x{client["h"]}'
            subprocess.run(["grim", "-g", geometry, str(image)], check=True)
            subprocess.run(["wtype", "-k", "Escape"], check=True)
            wait_for(lambda: not status()["visible"])
            warm = []
            for _ in range(3):
                start = time.monotonic()
                run("show")
                wait_for(lambda: status()["visible"] and not status()["busy"])
                warm.append(round((time.monotonic() - start) * 1000))
                assert status()["pid"] == pid
                run("close")
                assert not status()["visible"]
            run("show")
            settled("", 15)
            # Actual Wayland pointer input is bound only to the private output.
            protocol = APP / "tests/virtual_pointer.xml"
            subprocess.run(["wayland-scanner", "client-header", str(protocol), str(root / "pointer.h")], check=True)
            subprocess.run(["wayland-scanner", "private-code", str(protocol), str(root / "protocol.c")], check=True)
            flags = subprocess.check_output(["pkg-config", "--cflags", "--libs", "wayland-client"], text=True).split()
            subprocess.run(["cc", str(APP / "tests/pointer.c"), str(root / "protocol.c"), "-I"+str(root), "-o", str(root / "pointer"), *flags], check=True)
            pointer = subprocess.Popen([str(root / "pointer"), "GTK"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
            def pointer_event(command):
                pointer.stdin.write(command+"\n"); pointer.stdin.flush()
                assert pointer.stdout.readline().strip() == "ok"
            try:
                time.sleep(.2)  # Passive observer is installed after the layer maps.
                def layer():
                    return next(c for monitor in json.loads(private_hypr("-j", "layers")).values() for rows in monitor["levels"].values() for c in rows if c["namespace"] == "runtjik-gtk")
                for query, filename in [("/define", "gtk-word-empty.png"), ("/synonym", "gtk-synonym-empty.png"), ("/synonym glimmer", "gtk-synonyms.png"), ("/rec", "gtk-recorder-menu.png"), ("/define glimmer", "gtk-words.png"), ("/cliphist", "gtk-clipboard.png")]:
                    run("test-query", query); settled(query); time.sleep(.2)
                    bounds = layer()
                    subprocess.run(["grim", "-g", f'{bounds["x"]},{bounds["y"]} {bounds["w"]}x{bounds["h"]}', str(root / filename)], check=True)
                    if query == "/rec":
                        cards=[w for w in status()["captureUi"] if w["class"]=="sense" and w["visible"]]
                        assert len(cards)==6, cards
                        assert abs(cards[0]["y"]-cards[1]["y"])<1 and cards[2]["y"]>cards[0]["y"], cards
                        assert abs(cards[4]["width"]-cards[0]["width"])<1 and cards[4]["height"]>cards[0]["height"]*1.9, cards
                        rows=status()["rows"][:-1]
                        run("test-response",json.dumps({"kind":"results","query":"/rec","results":rows,"total":len(rows)}))
                        time.sleep(.15)
                        cards=[w for w in status()["captureUi"] if w["class"]=="sense" and w["visible"]]
                        assert len(cards)==5 and cards[4]["width"]>cards[0]["width"]*1.9, cards
                        subprocess.run(["grim", "-g", f'{bounds["x"]},{bounds["y"]} {bounds["w"]}x{bounds["h"]}', str(root / filename)], check=True)
                    if query == "/cliphist":
                        icon_fixture = next(r["thumbnail"] for r in status()["rows"] if r.get("thumbnail"))
                        geometry = status()["resultGeometry"]
                        assert len(geometry) == 3, geometry
                        assert max(w["textX"] for w in geometry)-min(w["textX"] for w in geometry) < 1, geometry
                        assert all(abs(w["iconWidth"]-104)<1 for w in geometry), geometry
                    if query == "/define glimmer":
                        before_tab = status()["selected"]
                        subprocess.run(["wtype", "-k", "Tab"], check=True)
                        wait_for(lambda: status()["selected"] != before_tab)
                        subprocess.run(["wtype", "-M", "shift", "-k", "Tab", "-m", "shift"], check=True)
                        wait_for(lambda: status()["selected"] == before_tab)
                        providers = [w for w in status()["wordUi"] if w["class"] == "provider"]
                        assert len(providers) == 2 and all(w["active"] for w in providers), providers
                        provider = providers[0]
                        pointer_event(f'm {int((bounds["x"]+provider["x"]+15)*10)} {int((bounds["y"]+provider["y"]+15)*10000/680)}')
                        pointer_event("b 1"); pointer_event("b 0")
                        wait_for(lambda: not [w for w in status()["wordUi"] if w["class"] == "provider"][0]["active"])
                        visible = [w for w in status()["wordUi"] if w["class"] == "sense" and w["visible"]]
                        assert len(visible) == 1, visible
                        time.sleep(.15)
                        subprocess.run(["grim", "-g", f'{bounds["x"]},{bounds["y"]} {bounds["w"]}x{bounds["h"]}', str(root / "gtk-words-slang.png")], check=True)
                        pointer_event("b 1"); pointer_event("b 0")
                        wait_for(lambda: [w for w in status()["wordUi"] if w["class"] == "provider"][0]["active"])
                for n in range(3):
                    run("test-request", json.dumps({"op":"app-action","provider":"Todo","action":"create","fields":{"title":f"Synthetic saved page {n+1}","kind":"reading","reference":f"https://example.org/page-{n}","group":"All","tags":["fixture"]}}))
                    wait_for(lambda: not status()["busy"])
                run("test-query", "/reading"); settled("/reading")
                wait_for(lambda: status()["app_state"].get("gtk_view")=="reading")
                time.sleep(.2)
                bounds=layer()
                subprocess.run(["grim","-g",f'{bounds["x"]},{bounds["y"]} {bounds["w"]}x{bounds["h"]}',str(root/"gtk-reading.png")],check=True)
                run("test-query", "/browser"); settled("/browser")
                # Browser favicons can be embedded bytes, not GTK icon names.
                image_data = base64.b64encode(Path(icon_fixture).read_bytes()).decode()
                rows = [{"id":"Browser:fixture", "name":"Synthetic browser icon", "provider":"Browser", "favicon":"data:image/png;base64,"+image_data}]
                run("test-response", json.dumps({"kind":"results", "query":"/browser", "results":rows, "total":1}))
                wait_for(lambda: any(w["id"]=="Browser:fixture" and w["texture"] for w in status()["resultGeometry"]))
                run("test-query", ""); settled("", 15)
                before = layer()
                pointer_event(f'm {int((before["x"]+32)*10)} {int((before["y"]+42)*10000/680)}')
                pointer_event("b 1"); pointer_event("r 40 20"); time.sleep(.1); pointer_event("b 0")
                moved = wait_for(lambda: layer() if layer()["x"] > before["x"] + 20 else None)
                assert moved["y"] >= before["y"], (before, moved)
                # Repeated motion must follow the global pointer, not feed back
                # coordinates from the layer that is itself being moved.
                pointer_event(f'm {int((moved["x"]+32)*10)} {int((moved["y"]+42)*10000/680)}')
                time.sleep(.08)
                pointer_event("b 1"); time.sleep(.08)
                for _ in range(5):
                    pointer_event("r 8 0"); time.sleep(.04)
                pointer_event("b 0")
                stepped = layer()
                assert abs(stepped["x"]-moved["x"]-40) <= 3, (moved, stepped)
                moved = stepped
                pointer_event(f'm {int((moved["x"]+moved["w"]/2)*10)} {int((moved["y"]+moved["h"]-13)*10000/680)}')
                pointer_event("b 1"); pointer_event("r 35 15"); time.sleep(.1); pointer_event("b 0")
                wait_for(lambda: layer()["w"] > moved["w"] + 15)
                pointer_event("m 50 50")
                pointer_event("b 1"); pointer_event("b 0")
                wait_for(lambda: not status()["visible"])
                # Warm reopen must discard both dragged position and resized dimensions,
                # even when the configuration has not changed.
                run("show"); settled("", 15)
                restored = wait_for(lambda: layer() if all(layer()[k] == before[k] for k in ("x", "y", "w", "h")) else None)
                assert restored["x"] == (1000 - restored["w"]) // 2, restored
                assert restored["y"] == (680 - restored["h"]) // 2, restored
                run("close")
                policy_config = root / "runtjik.conf"
                policy_original = policy_config.read_text()
                policy_config.write_text(policy_original.replace("[ui]", "[ui]\nclose_on_outside_click=false"))
                run("show"); settled("",15)
                time.sleep(.2)
                pointer_event("m 50 50"); pointer_event("b 1"); pointer_event("b 0")
                time.sleep(.15)
                assert status()["visible"] and not status()["pinned"]
                bounds = layer()
                pointer_event(f'm {int((bounds["x"]+130)*10)} {int((bounds["y"]+42)*10000/680)}')
                pointer_event("b 1"); pointer_event("b 0"); time.sleep(.1)
                subprocess.run(["wtype", "fire"], check=True)
                settled("fire", 1)
                subprocess.run(["wtype", "-k", "Escape"], check=True)
                wait_for(lambda: not status()["visible"])
                policy_config.write_text(policy_original)
                run("show"); settled("",15)
                run("test-query", "/pin"); settled("/pin")
                run("test-activate", "default")
                wait_for(lambda: status()["pinned"] and not status()["busy"])
                wait_for(lambda: abs(status()["pinAngle"]-45)<.1)
                time.sleep(.2)
                pointer_event("m 50 50"); pointer_event("b 1"); pointer_event("b 0")
                time.sleep(.1)
                assert status()["visible"] and status()["pinned"]
                run("close"); run("show"); settled("",15)
                config = root / "runtjik.conf"
                original = config.read_text()
                config.write_text(original.replace("[ui]", '[ui]\nwidth=500\nheight=480\ncolumns=3'))
                run("close"); run("show"); settled("",15)
                wait_for(lambda: layer()["w"] == 500 and status()["columns"] == 3)
                subprocess.run(["grim", str(root / "gtk-narrow.png")], check=True)
                bounds = layer()
                pointer_event(f'm {int((bounds["x"]+130)*10)} {int((bounds["y"]+140)*10000/680)}')
                time.sleep(.1)
                # Stationary pointer must not steal keyboard selection after scrolling.
                for direction, delta in [("Down", 3)] * 4 + [("Up", -3)] * 4:
                    previous = status()["selected"]
                    subprocess.run(["wtype", "-k", direction], check=True)
                    expected = (previous + delta) % 15
                    wait_for(lambda: status()["selected"] == expected)
                    assert not status()["selectedBounds"]["highlightHidden"], status()["selectedBounds"]
                    time.sleep(.3)
                    assert status()["selected"] == expected, status()
                    bounds = status()["selectedBounds"]
                    assert bounds and not bounds["highlightHidden"], bounds
                    assert bounds["top"] >= -1 and bounds["bottom"] <= bounds["viewportHeight"] + 1, bounds
                gallery_rows = [{**row, "thumbnail": icon_fixture} for row in status()["rows"]]
                run("test-query", "/grab history"); settled("/grab history")
                run("test-response", json.dumps({"kind":"results", "query":"/grab history", "results":gallery_rows, "total":15}))
                wait_for(lambda: len(status()["rows"]) == 15)
                time.sleep(.2)
                for direction in ["Down"] * 6 + ["Up"] * 6:
                    state = status()
                    delta = state["columns"] * (1 if direction == "Down" else -1)
                    expected = (state["selected"] + delta) % 15
                    subprocess.run(["wtype", "-k", direction], check=True)
                    wait_for(lambda: status()["selected"] == expected)
                    assert not status()["selectedBounds"]["highlightHidden"], status()["selectedBounds"]
                    time.sleep(.3)
                    bounds = status()["selectedBounds"]
                    assert status()["selected"] == expected, status()
                    assert bounds and not bounds["highlightHidden"], bounds
                    assert bounds["top"] >= -1 and bounds["bottom"] <= bounds["viewportHeight"] + 1, bounds
                run("test-query", ""); settled("", 15)
                time.sleep(.2)
                pointer_event("s -150"); time.sleep(.1)
                wait_for(lambda: status()["scrollY"] == 0)
                pitch = status()["rowPitch"]
                assert pitch and pitch > 100, pitch
                pointer_event("s 15")
                wait_for(lambda: abs(status()["scrollY"]-pitch) < 1)
                pointer_event("r 2 1"); time.sleep(.12)
                assert abs(status()["scrollY"]-pitch) < 1, status()
                pointer_event("s 15")
                wait_for(lambda: abs(status()["scrollY"]-2*pitch) < 1)
                pointer_event("r 2 1"); time.sleep(.12)
                assert abs(status()["scrollY"]-2*pitch) < 1, status()
                pointer_event("s -15")
                wait_for(lambda: abs(status()["scrollY"]-pitch) < 1)
                pointer_event("s -15")
                wait_for(lambda: status()["scrollY"] == 0)

                pointer_event("s 1")
                wait_for(lambda: abs(status()["scrollY"]-pitch) < 1)
                pointer_event("s -1")
                wait_for(lambda: status()["scrollY"] == 0)

                pointer_event("s 15")
                time.sleep(.3)
                selected = status()
                assert selected["selected"] == 3, selected["selected"]
                assert not selected["selectedBounds"]["highlightHidden"], selected["selectedBounds"]
                expected_id = selected["rows"][selected["selected"]]["id"]
                subprocess.run(["wtype", "-k", "Return"], check=True)
                wait_for(lambda: status()["lastActivation"] == expected_id)
                wait_for(lambda: not status()["busy"])

                config.write_text(original)
                run("close"); run("show"); settled("",15)
                private_hypr("output", "create", "headless", "TRANSFER")
                private_hypr("eval", 'hl.monitor({output="TRANSFER",mode="1920x1440@60",position="1000x0",scale=1.5})')
                time.sleep(.3)
                origin = layer()
                pointer_event(f'm {int((origin["x"]+32)*10)} {int((origin["y"]+42)*10000/680)}')
                time.sleep(.08)
                pointer_event("b 1"); time.sleep(.08); pointer_event("r 1000 120"); time.sleep(.1); pointer_event("b 0")
                def transferred():
                    data = json.loads(private_hypr("-j", "layers")).get("TRANSFER", {})
                    return next((c for rows in data.get("levels", {}).values() for c in rows if c["namespace"] == "runtjik-gtk"), None)
                destination = wait_for(transferred)
                cursor = json.loads(private_hypr("-j", "cursorpos"))
                assert abs(destination["x"]-(cursor["x"]-32)) <= 3, (origin, destination, cursor)
                assert abs(destination["y"]-(cursor["y"]-42)) <= 3, (origin, destination, cursor)
                time.sleep(.3)
                subprocess.run(["grim", "-o", "TRANSFER", str(root / "gtk-transfer.png")], check=True)
                private_hypr("eval", 'hl.monitor({output="TRANSFER",disabled=true})')
                run("close"); run("show"); settled("",15)
            finally:
                pointer.stdin.close(); pointer.wait(timeout=5)
            # Sum proportional memory, including the native backend, rather than RSS.
            pids = [pid]
            # The backend is spawned by a worker thread, so inspect every task.
            for task in Path(f"/proc/{pid}/task").iterdir():
                pids.extend(int(p) for p in (task / "children").read_text().split())
            pss = 0
            for process in set(pids):
                for line in Path(f"/proc/{process}/smaps_rollup").read_text().splitlines():
                    if line.startswith("Pss:"):
                        pss += int(line.split()[1])
            print(json.dumps({"cold_with_four_concurrent_clients_ms":cold_ms, "warm_control_roundtrip_ms":warm,
                              "combined_pss_kib":pss, "renderer":"cairo", "fixture":"15 synthetic apps"}))
            print("PASS: GTK search, stale results, commands, launch/window actions, real keyboard input, warm reopen and isolated control")
            print("Synthetic screenshot: " + str(image))
            run("close")
            qml_config = root / "runtjik.conf"
            qml_config.write_text(qml_config.read_text().replace("[ui]", "[ui]\nclose_on_outside_click=false\nwidth=500\nheight=480\ncolumns=3"))
            qml_env = {**os.environ, "RUNTJIK_DEMO":"1", "QT_QUICK_BACKEND":"software", "QT_QUICK_CONTROLS_STYLE":"Basic"}
            with (root / "quickshell.log").open("w") as log:
                qml = subprocess.Popen(["quickshell", "--path", str(target / "qml")], env=qml_env, stdout=log, stderr=log)
                def ipc(action, *args):
                    return subprocess.check_output(["quickshell", "ipc", "--path", str(target / "qml"), "call", "runtjik", action, *args], env=qml_env, text=True, stderr=subprocess.DEVNULL)
                try:
                    def qml_ready():
                        try:
                            return json.loads(ipc("status"))["ready"]
                        except (subprocess.CalledProcessError, json.JSONDecodeError):
                            return False
                    wait_for(qml_ready)
                    ipc("capture", str(root / "quickshell-launcher.png"))
                    wait_for(lambda: (root / "quickshell-launcher.png").exists())
                    print("Quickshell comparison:", root / "quickshell-launcher.png")
                    time.sleep(.2)
                    def qml_pointer(commands):
                        subprocess.run([str(root / "pointer"), "GTK"], input=commands, text=True, stdout=subprocess.DEVNULL, check=True)
                    bounds = json.loads(ipc("status"))["geometry"]
                    qml_pointer(f'm {int((bounds["x"]+130)*10)} {int((bounds["y"]+140)*10000/680)}\ns -150\n')
                    wait_for(lambda: abs(json.loads(ipc("testState"))["scrollY"]) < 1)
                    qml_pointer("s 15\n")
                    wait_for(lambda: abs(json.loads(ipc("testState"))["scrollY"]-122) < 1)
                    qml_pointer("s -15\n")
                    wait_for(lambda: abs(json.loads(ipc("testState"))["scrollY"]) < 1)
                    qml_pointer("m 50 50\nb 1\nb 0\n")
                    wait_for(lambda: not json.loads(ipc("status"))["keyboardOwned"])
                    assert json.loads(ipc("status"))["visible"] and not json.loads(ipc("status"))["pinned"]
                    bounds = json.loads(ipc("status"))["geometry"]
                    qml_pointer(f'm {int((bounds["x"]+130)*10)} {int((bounds["y"]+42)*10000/680)}\nb 1\nb 0\n')
                    wait_for(lambda: json.loads(ipc("status"))["keyboardOwned"])
                    time.sleep(.1)  # Complete the compositor keyboard handoff.
                    subprocess.run(["wtype", "fire"], check=True)
                    wait_for(lambda: json.loads(ipc("testState"))["resultQuery"] == "fire")
                    subprocess.run(["wtype", "-k", "Escape"], check=True)
                    wait_for(lambda: qml.poll() is not None)
                    print("PASS: both frontends stay open unpinned, release/reacquire typing focus, and still close on Escape")
                finally:
                    qml.terminate()
                    qml.wait(timeout=10)

        except Exception:
            failed = status()
            failed["rows"] = len(failed.get("rows", []))
            print("Failure state:", json.dumps(failed), flush=True)
            subprocess.run(["grim", str(root / "failure.png")], check=False)
            print("Failure screenshot:", root / "failure.png", flush=True)
            raise
        finally:
            run("quit")
            def stopped():
                try:
                    return not status()["running"]
                except subprocess.CalledProcessError:
                    # The socket can close between connecting and reading status.
                    return False
            wait_for(stopped)


if __name__ == "__main__":
    main()
