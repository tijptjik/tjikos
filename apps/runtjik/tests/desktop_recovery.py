"""Push updates and complete compositor/GTK restart, on workspace 7 privately."""
import json
import os
from pathlib import Path
import select
import signal
import subprocess
import time
from gtk_ui import APP, silent_compositor
from private_compositor import wait_for

PROBE = APP.parents[1] / "packages/tjik-desktop/target/release/examples/probe"
host = dict(os.environ)
with silent_compositor():
    runtime = Path(os.environ["XDG_RUNTIME_DIR"])
    root = runtime.parent
    assert str(runtime).startswith("/tmp/bc-")
    signature = os.environ["HYPRLAND_INSTANCE_SIGNATURE"]
    display = os.environ["WAYLAND_DISPLAY"]
    lock = runtime / "hypr" / signature / "hyprland.lock"
    compositor_pid = int(lock.read_text().splitlines()[0])
    config = root / "runtjik.conf"
    config.write_text('[index]\npath_commands=false\n[theme]\nicon_theme="Adwaita"\n[ui]\nauto_launch_applications=false\nauto_launch_commands=false\n')
    env = {**os.environ, "RUNTJIK_HOME": str(APP / "dist"), "RUNTJIK_CONFIG": str(config),
           "XDG_STATE_HOME": str(root / "state"), "TJIK_COMPOSITOR": "hyprland", "GTK_A11Y": "none", "GSK_RENDERER": "cairo"}
    binary = Path(os.environ.get("RUNTJIK_GTK_TEST_BINARY", str(APP / "dist/runtjik-gtk")))
    def run(action, *args):
        return json.loads(subprocess.check_output([str(binary), action, *args, "--demo"], env=env, text=True) or '{}')
    def status():
        return run("status")
    watcher = subprocess.Popen([str(PROBE), "watch"], env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, bufsize=0)
    buffered = b""
    observed = []
    def event_until(predicate, timeout=15):
        global buffered
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            while b"\n" in buffered:
                line, buffered = buffered.split(b"\n", 1)
                event = json.loads(line)
                assert not observed or event["sequence"] > observed[-1]["sequence"], event
                observed.append(event)
                if predicate(event):
                    return event
            assert watcher.poll() is None, watcher.stderr.read().decode()
            if select.select([watcher.stdout], [], [], .1)[0]:
                buffered += os.read(watcher.stdout.fileno(), 65536)
        raise AssertionError(("Event timeout", observed[-3:]))
    def hypr(*args, sig=signature):
        return subprocess.check_output(["hyprctl", "--instance", sig, *args], env=env, text=True)
    replacement = None
    try:
        initial = event_until(lambda e: "Ready" in e["state"])
        generation = initial["state"]["Ready"]["generation"]
        hypr("output", "create", "headless", "EVENT")
        hypr("eval", 'hl.monitor({output="EVENT",mode="1920x1080@60",position="1000x0",scale=1.5,transform=1})')
        changed = event_until(lambda e: any(o["name"] == "EVENT" and o["transform"] == 1 for o in e["state"].get("Ready", {}).get("outputs", [])))
        output = next(o for o in changed["state"]["Ready"]["outputs"] if o["name"] == "EVENT")
        assert output["logical"]["width"] == 720 and output["logical"]["height"] == 1280, output
        run("show")
        wait_for(lambda: status().get("ready"))
        run("test-query", "fire")
        wait_for(lambda: status().get("resultQuery") == "fire" and not status().get("busy"))
        run("test-request", json.dumps({"op": "activate", "id": "/pin"}))
        wait_for(lambda: status().get("pinned"))
        run("test-query", "fire")
        wait_for(lambda: status().get("resultQuery") == "fire" and not status().get("busy"))
        before = status()
        old_pid = before["pid"]
        old_children = (Path("/proc") / str(old_pid) / "task" / str(old_pid) / "children").read_text().split()
        time.sleep(.2)
        os.kill(compositor_pid, signal.SIGTERM)
        event_until(lambda e: "Unavailable" in e["state"])
        wait_for(lambda: status().get("recovering"), timeout=15)
        parent_display = Path(host["WAYLAND_DISPLAY"])
        if not parent_display.is_absolute():
            parent_display = Path(host["XDG_RUNTIME_DIR"]) / parent_display
        replacement_env = {**env, "WAYLAND_DISPLAY": str(parent_display)}
        replacement_env.pop("HYPRLAND_INSTANCE_SIGNATURE", None)
        def restart_compositor():
            with (root / "replacement.log").open("a") as log:
                return subprocess.Popen(["Hyprland", "--config", str(root / "hyprland.lua")], env=replacement_env, stdout=log, stderr=log)
        replacement = restart_compositor()
        def new_session():
            for candidate in (runtime / "hypr").glob("*/hyprland.lock"):
                if candidate.parent.name == signature:
                    continue
                lines = candidate.read_text().splitlines()
                if len(lines) >= 2 and lines[0] == str(replacement.pid) and lines[1] == display and (candidate.parent / ".socket.sock").is_socket():
                    return candidate.parent.name
        new_signature = wait_for(new_session, timeout=20)
        # The launch rule was installed before either parent window was created.
        parents = json.loads(subprocess.check_output(["hyprctl", "-j", "clients"], env=host, text=True))
        nested = [c for c in parents if c["initialClass"] == "aquamarine"]
        assert nested and all(c["workspace"]["id"] == 7 for c in nested), nested
        recovered = event_until(lambda e: e["state"].get("Ready", {}).get("generation") not in (None, generation), timeout=20)
        assert recovered["resync"], recovered
        after = wait_for(lambda: (s if s.get("ready") and s.get("pid") != old_pid and s.get("resultQuery") == "fire" and not s.get("busy") else None) if (s := status()) else None, timeout=25)
        assert after["visible"] and after["pinned"], after
        for pid in old_children:
            wait_for(lambda: not (Path("/proc") / pid / "stat").exists() or (Path("/proc") / pid / "stat").read_text().rsplit(")", 1)[1].split()[0] == "Z")
        # A hidden launcher must not map or take focus while restoring.
        run("close")
        wait_for(lambda: not status().get("visible"))
        time.sleep(.2)
        replacement.terminate()
        replacement.wait(timeout=10)
        event_until(lambda e: "Unavailable" in e["state"])
        wait_for(lambda: status().get("recovering"), timeout=15)
        replacement = restart_compositor()
        new_signature = wait_for(new_session, timeout=20)
        event_until(lambda e: e["state"].get("Ready", {}).get("generation") not in (None, generation, recovered["state"]["Ready"]["generation"]), timeout=20)
        def hidden_recovered():
            state = status()
            assert not state.get("visible"), state
            return state if state.get("ready") and state.get("pid") != after["pid"] else None
        wait_for(hidden_recovered, timeout=25)
        layers = json.loads(hypr("-j", "layers", sig=new_signature))
        assert not any(c["namespace"] == "runtjik-gtk" for m in layers.values() for rows in m["levels"].values() for c in rows), layers
        # Quit during downtime must cancel recovery, not leave a future relaunch.
        replacement.terminate()
        replacement.wait(timeout=10)
        wait_for(lambda: status().get("recovering"), timeout=15)
        run("quit")
        wait_for(lambda: not status().get("running"))
        (root / "events.json").write_text(json.dumps(observed, indent=2))
        print("PASS: ordered output push, generation recovery, GTK relaunch with query/pin state, hidden restoration, backend teardown and quit while offline")
        print("Artifacts:", root)
    except Exception:
        print("Recovery failure status:", {k: v for k, v in status().items() if k in ("pid", "query", "resultQuery", "ready", "recovering", "pinned", "visible", "busy", "backend_error")}, flush=True)
        raise
    finally:
        try:
            run("quit")
        except (subprocess.SubprocessError, json.JSONDecodeError):
            pass
        watcher.terminate()
        watcher.wait(timeout=5)
        if replacement and replacement.poll() is None:
            replacement.terminate()
            replacement.wait(timeout=10)
