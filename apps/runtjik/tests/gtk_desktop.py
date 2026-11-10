"""Synthetic IME protocol, scaled outputs, startup motion and lifecycle."""
import json
import os
from pathlib import Path
import subprocess
import time
from gtk_ui import APP, silent_compositor
from private_compositor import wait_for


def main():
    with silent_compositor():
        runtime = Path(os.environ["XDG_RUNTIME_DIR"])
        root = runtime.parent
        signature = os.environ["HYPRLAND_INSTANCE_SIGNATURE"]
        assert str(runtime).startswith("/tmp/bc-")
        def hypr(*args):
            return subprocess.check_output(["hyprctl", "--instance", signature, *args], text=True)
        os.environ.pop("GTK_IM_MODULE", None)
        os.environ.update(RUNTJIK_HOME=str(APP / "dist"), RUNTJIK_CONFIG=str(root / "runtjik.conf"),
                          XDG_STATE_HOME=str(root / "state"), GTK_A11Y="none", GSK_RENDERER="cairo",
                          RUNTJIK_DEMO_SETUP_MS="1200")
        config = root / "runtjik.conf"
        config.write_text('[theme]\nicon_theme="Adwaita"\n[ui]\nauto_launch_applications=false\nauto_launch_commands=false\n')
        hypr("output", "create", "headless", "GTK")
        hypr("eval", 'hl.monitor({output="WAYLAND-1",disabled=true}); hl.monitor({output="GTK",mode="1700x1000@60",position="0x0",scale=1}); hl.dispatch(hl.dsp.focus({monitor="GTK"}))')
        hypr("eval", 'hl.layer_rule({name="gtk-fixture-static",match={namespace="^runtjik-gtk$"},no_anim=true})')
        time.sleep(.6)  # Allow the virtual output mode/configure cycle to settle.
        binary = APP / "dist/runtjik-gtk"
        def run(action, *args):
            return subprocess.check_output([str(binary), action, *args, "--demo"], text=True)
        def status():
            return json.loads(run("status"))
        def settled(query):
            return wait_for(lambda: (s if s.get("resultQuery") == query and not s.get("busy") else None) if (s := status()).get("running") else None)
        def layers():
            return [(name, c) for name, monitor in json.loads(hypr("-j", "layers")).items() for rows in monitor["levels"].values() for c in rows if c["namespace"] == "runtjik-gtk"]
        subprocess.run(["wayland-scanner", "client-header", str(APP / "tests/input-method-v2.xml"), str(root / "input-method.h")], check=True)
        subprocess.run(["wayland-scanner", "private-code", str(APP / "tests/input-method-v2.xml"), str(root / "im-protocol.c")], check=True)
        flags = subprocess.check_output(["pkg-config", "--cflags", "--libs", "wayland-client", "xkbcommon"], text=True).split()
        subprocess.run(["cc", str(APP / "tests/input_method.c"), str(root / "im-protocol.c"), "-I"+str(root), "-o", str(root / "ime"), *flags], check=True)
        ime = subprocess.Popen([str(root / "ime")], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
        def input_method(value):
            ime.stdin.write(value+"\n"); ime.stdin.flush()
            return json.loads(ime.stdout.readline())
        try:
            show = subprocess.Popen([str(binary), "serve", "--demo"], stdout=subprocess.DEVNULL)
            initial = wait_for(lambda: (s if s.get("running") and s.get("startup") else None) if (s := status()) else None)
            assert not initial["message"], initial
            time.sleep(.6)
            bounds = layers()[0][1]
            geometry = f'{bounds["x"]},{bounds["y"]} {bounds["w"]}x{bounds["h"]}'
            subprocess.run(["grim", "-g", geometry, str(root / "gtk-startup.png")], check=True)
            assert show.poll() is None
            settled("")
            assert not status()["startup"]
            wait_for(lambda: input_method("s")["active"])
            # Drain the initial pointer-enter caused by mapping results under
            # the stationary compositor cursor before measuring keyboard input.
            time.sleep(.3)
            before = status()["selected"]
            input_method("p zhong")
            wait_for(lambda: status()["composing"])
            subprocess.run(["wtype", "-k", "Down", "-k", "Tab"], check=True)
            assert status()["selected"] == before and status()["visible"], (before, status())
            assert status()["query"] == ""
            assert input_method("s")["keys"] >= 4
            subprocess.run(["wtype", "-k", "Return"], check=True)
            settled("中文")
            assert not status()["composing"] and status()["visible"]
            run("test-query", "")
            settled("")
            input_method("p qu")
            wait_for(lambda: status()["composing"])
            input_method("x")
            wait_for(lambda: not status()["composing"])
            assert status()["visible"]
            # Committed Unicode via the actual virtual-keyboard protocol.
            subprocess.run(["wtype", "é香港"], check=True)
            settled("é香港")
            run("test-query", "")
            settled("")
            rows = status()["rows"]
            run("test-response", json.dumps({"kind":"results", "results":rows[1:], "total":len(rows)-1}))
            time.sleep(.08)
            subprocess.run(["grim", "-o", "GTK", str(root / "gtk-removal.png")], check=True)
            run("test-query", "/grab history")
            settled("/grab history")
            gallery = status()["rows"]
            assert gallery, "Synthetic capture history fixture is empty"
            run("test-response", json.dumps({"kind":"results", "query":"/grab history", "results":gallery[1:], "total":len(gallery)-1}))
            time.sleep(.12)
            subprocess.run(["grim", "-o", "GTK", str(root / "gtk-gallery-removal.png")], check=True)
            run("test-query", "")
            settled("")
            # Virtual outputs exercise logical geometry and real compositor scaling.
            for name, scale in [("DPI150",1.5),("DPI200",2)]:
                hypr("output", "create", "headless", name)
                hypr("eval", f'hl.monitor({{output="{name}",mode="1920x1440@60",position="1700x0",scale={scale}}})')
                config.write_text(f'[theme]\nicon_theme="Adwaita"\n[ui]\nmonitor="{name}"\nauto_launch_applications=false\nauto_launch_commands=false\n')
                run("close"); run("show"); settled("")
                try:
                    wait_for(lambda: any(n == name and c["w"] == 720 and c["h"] == 600 for n,c in layers()))
                except ValueError:
                    raise AssertionError((name, layers(), json.loads(hypr("-j", "monitors"))))
                time.sleep(.5)
                monitors = json.loads(hypr("-j", "monitors"))
                assert next(m for m in monitors if m["name"] == name)["scale"] == scale
                subprocess.run(["grim", "-o", name, str(root / f"gtk-{name}.png")], check=True)
                hypr("eval", f'hl.monitor({{output="{name}",disabled=true}})')
            config.write_text('[theme]\nicon_theme="Adwaita"\n[ui]\nauto_launch_applications=false\nauto_launch_commands=false\n')
            run("close"); run("show"); settled("")
            print("PASS: Wayland IME preedit/commit/cancel, Unicode input, 1.5x/2x logical dimensions and startup")
            print("Artifacts:", root)
        finally:
            run("quit")
            ime.stdin.close(); ime.wait(timeout=5)


if __name__ == "__main__":
    main()
