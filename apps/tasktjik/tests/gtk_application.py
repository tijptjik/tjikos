"""Synthetic keyboard and lifecycle acceptance for both hosts of Tasktjik."""
import json
import os
from pathlib import Path
import subprocess
import sys
import time
from PIL import Image

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "packages/tjik-gtk/scripts"))
from display import rendering_display
from private_compositor import wait_for


def main():
    with rendering_display():
        directory = Path(os.environ["XDG_RUNTIME_DIR"]).parent
        instance = os.environ["HYPRLAND_INSTANCE_SIGNATURE"]
        def hypr(*args):
            return subprocess.check_output(["hyprctl", "--instance", instance, *args], text=True)
        def clients():
            return json.loads(hypr("-j", "clients"))
        def key(name):
            subprocess.run(["wtype", "-k", name], check=True)
            time.sleep(0.2)
        def snapshot(name):
            path = directory / name
            # Frame delivery on a hidden nested display is asynchronous; don't
            # mistake a background-only transition frame for the finished UI.
            for _ in range(40):
                subprocess.run(["grim", str(path)], check=True)
                with Image.open(path) as frame:
                    header = frame.convert("RGB").crop((25, 25, 200, 68))
                    if sum(r > 180 and g > 180 and b > 180 for r, g, b in header.getdata()) > 40:
                        return
                time.sleep(0.25)
            raise AssertionError(f"Application heading did not render: {path}")
        env = {**os.environ, "TASKTJIK_HOME": str(ROOT / "apps/tasktjik/dist"),
               "XDG_CONFIG_HOME": str(directory / "config"), "XDG_STATE_HOME": str(directory / "state"),
               "GTK_A11Y": "none", "GSK_RENDERER": "cairo"}
        # Rules precede launch; the containing test display is already on workspace 7.
        hypr("eval", 'hl.window_rule({name="tasktjik-host-fixture",float=true,no_anim=true,match={initial_title="^Tasktjik$"}})')
        binary = ROOT / "apps/tasktjik/dist/tasktjik-gtk"
        log = directory / "tasktjik-gtk.log"
        with log.open("w") as output:
            child = subprocess.Popen(["dbus-run-session", "--", str(binary), "--demo"], env=env, stdout=output, stderr=output)
            try:
                client = wait_for(lambda: next((c for c in clients() if c["title"] == "Tasktjik"), None))
                address = client["address"]
                hypr("eval", f'hl.dispatch(hl.dsp.focus({{window="address:{address}"}}))')
                hypr("dismissnotify")
                time.sleep(0.3)
                snapshot("tasktjik-initial.png")
                key("n")
                subprocess.run(["wtype", "Shared GTK synthetic task"], check=True)
                time.sleep(0.3)
                snapshot("tasktjik-editor.png")
                subprocess.run(["wtype", "-M", "ctrl", "-k", "Return", "-m", "ctrl"], check=True)
                time.sleep(0.5)
                hypr("dismissnotify")
                time.sleep(0.3)
                snapshot("tasktjik-standalone.png")
                key("slash")
                subprocess.run(["wtype", "n"], check=True)
                key("Escape")
                assert child.poll() is None, log.read_text()
                key("Escape")
                wait_for(lambda: not any(c["address"] == address for c in clients()))
                assert child.wait(timeout=5) == 0, log.read_text()
            finally:
                if child.poll() is None:
                    child.terminate()
                    child.wait(timeout=5)
        config = directory / "runtjik.conf"
        config.write_text('[plugins]\nenabled=["Todo"]\n[ui]\nauto_launch_applications=false\nauto_launch_commands=false\n')
        env.update(RUNTJIK_HOME=str(ROOT / "apps/runtjik/dist"), RUNTJIK_CONFIG=str(config))
        binary = ROOT / "apps/runtjik/dist/runtjik-gtk"
        def run(action, *args):
            return subprocess.check_output([str(binary), action, *args, "--demo"], env=env, text=True)
        def status():
            return json.loads(run("status"))
        try:
            run("show")
            for query in ["/todo", "/reading"]:
                run("test-query", query)
                wait_for(lambda: status().get("resultQuery") == query and not status().get("busy"))
                key("n")
                wait_for(lambda: status()["app_state"].get("gtk_view") == "editor")
                key("Escape")
                wait_for(lambda: status()["app_state"].get("gtk_view") != "editor")
                assert status()["visible"]
                key("Escape")
                wait_for(lambda: status().get("resultQuery") == "" and not status().get("busy"))
                assert status()["visible"], "Application Escape must return to the launcher"
            print(f"PASS: standalone and embedded task/reading keyboard lifecycle; artifacts {directory}")
        finally:
            run("quit")


if __name__ == "__main__":
    main()
