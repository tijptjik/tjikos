"""Render a synthetic tray menu silently on workspace 7 without native providers."""
import json
import os
import signal
from pathlib import Path
import subprocess
import time

APP = Path(__file__).resolve().parents[1]
TITLE = "Bartjik Tray Menu Fixture"


def hypr(*args):
    return subprocess.check_output(["hyprctl", *args], text=True)


rule = f"bartjik_tray_fixture_{os.getpid()}"
result = hypr("eval", f'_G.{rule}=hl.window_rule({{name="{rule}",workspace="7 silent",float=true,no_initial_focus=true,no_focus=true,match={{initial_title="^{TITLE}$"}}}})')
assert "error" not in result.lower(), result
process = None
try:
    env = {
        **os.environ,
        "GTK_A11Y": "none",
        "GSK_RENDERER": "cairo",
        "PKG_CONFIG_PATH": str(APP / "../../packages/tjik-gtk/target/native/lib/pkgconfig"),
    }
    process = subprocess.Popen(
        ["cargo", "test", "--release", "--locked", "--features", "gtk-ui", "--bin", "bartjik-gtk", "tray_menu_render", "--", "--ignored", "--nocapture"],
        cwd=APP, env=env, start_new_session=True,
    )
    placed = False
    deadline = time.monotonic() + 90
    while process.poll() is None:
        assert time.monotonic() < deadline, "Synthetic menu fixture timed out"
        for client in json.loads(hypr("-j", "clients")):
            if client["title"] == TITLE:
                assert client["workspace"]["id"] == 7, client
                assert json.loads(hypr("-j", "activewindow")).get("address") != client["address"], "Fixture stole focus"
                placed = True
        time.sleep(.1)
    assert process.returncode == 0, process.returncode
    assert placed, "Fixture placement was not observed"
    print("PASS: themed menu spacing, disabled rows, submenu and command dispatch; /tmp/bartjik-tray-menu.png")
finally:
    if process is not None and process.poll() is None:
        os.killpg(process.pid, signal.SIGTERM)
        process.wait(timeout=5)
    result = hypr("eval", f"if _G.{rule} then _G.{rule}:set_enabled(false); _G.{rule}=nil end")
    assert "error" not in result.lower(), result
