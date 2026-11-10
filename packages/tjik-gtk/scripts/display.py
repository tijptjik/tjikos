"""Private GTK rendering that keeps frames flowing while the parent is hidden."""
from contextlib import contextmanager
import json
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "apps/runtjik/tests"))
from gtk_ui import silent_compositor
from private_compositor import wait_for


@contextmanager
def rendering_display():
    with silent_compositor():
        instance = os.environ["HYPRLAND_INSTANCE_SIGNATURE"]

        def hypr(*args):
            return subprocess.check_output(
                ["hyprctl", "--instance", instance, *args], text=True
            )

        hypr("output", "create", "headless", "GTK")
        hypr("eval", 'hl.monitor({output="WAYLAND-1",disabled=true}); '
             'hl.monitor({output="GTK",mode="1000x680@60",position="0x0",scale=1}); '
             'hl.dispatch(hl.dsp.focus({monitor="GTK"}))')
        wait_for(lambda: any(m["name"] == "GTK" for m in json.loads(hypr("-j", "monitors"))))
        result = hypr("eval", '_G.tjik_collection_fixture=hl.window_rule({'
                      'name="tjik-collection-fixture",float=true,no_anim=true,'
                      'match={initial_title="^Tasktjik (reading|column|task) fixture$"}})')
        assert "error" not in result.lower(), result
        yield
