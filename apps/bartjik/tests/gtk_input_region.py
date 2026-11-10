"""Check committed Wayland input regions after hide, resize and reveal on workspace 7."""
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
import time

APP = Path(__file__).resolve().parents[1]
TITLE = "Bartjik Input Region Fixture"


def hypr(*args):
    return subprocess.check_output(["hyprctl", *args], text=True)


rule = f"bartjik_input_fixture_{os.getpid()}"
result = hypr("eval", f'_G.{rule}=hl.window_rule({{name="{rule}",workspace="7 silent",float=true,no_initial_focus=true,no_focus=true,match={{initial_title="^{TITLE}$"}}}})')
assert "error" not in result.lower(), result
try:
    env = {
        **os.environ,
        "GTK_A11Y": "none",
        "GSK_RENDERER": "cairo",
        "WAYLAND_DEBUG": "client",
        "PKG_CONFIG_PATH": str(APP / "../../packages/tjik-gtk/target/native/lib/pkgconfig"),
    }
    with tempfile.TemporaryFile(mode="w+") as log:
        process = subprocess.Popen(
            ["cargo", "test", "--release", "--locked", "--features", "gtk-ui", "--bin", "bartjik-gtk", "hidden_bar_input_protocol", "--", "--ignored", "--nocapture"],
            cwd=APP, env=env, stdout=log, stderr=log,
        )
        placed = False
        deadline = time.monotonic() + 180
        try:
            while process.poll() is None:
                assert time.monotonic() < deadline, "Input fixture timed out"
                for client in json.loads(hypr("-j", "clients")):
                    if client["title"] == TITLE:
                        assert client["workspace"]["id"] == 7, client
                        assert json.loads(hypr("-j", "activewindow")).get("address") != client["address"]
                        placed = True
                time.sleep(.1)
        finally:
            if process.poll() is None:
                process.terminate()
                process.wait(timeout=5)
        log.seek(0)
        trace = log.read()
    Path("/tmp/bartjik-input-region.trace").write_text(trace)
    assert process.returncode == 0, trace[-5000:]
    assert placed, "Fixture placement was not observed"
    pending = {}
    committed = {}
    phases = {}
    latest = None
    for line in trace.splitlines():
        if match := re.search(r"wl_surface[#@](\d+)\.attach\(([^,]+),", line):
            pending[match[1]] = match[2] != "nil"
        if match := re.search(r"wl_surface[#@](\d+)\.commit\(", line):
            if match[1] in pending:
                committed[match[1]] = pending[match[1]]
                latest = committed[match[1]]
        if "INPUT_PHASE " in line:
            phase = line.split("INPUT_PHASE ", 1)[1].strip()
            phases[phase] = latest
    assert phases == {
        "shown": True, "hidden": False, "hidden-resized": False, "restored": True,
    }, phases
    print("PASS: bar surface is unmapped after hiding and resizing, remapped on reveal")
finally:
    result = hypr("eval", f"if _G.{rule} then _G.{rule}:set_enabled(false); _G.{rule}=nil end")
    assert "error" not in result.lower(), result
