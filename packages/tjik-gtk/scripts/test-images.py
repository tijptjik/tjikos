"""Check burst binding, request coalescing and weak targets on a private display."""
import os
from pathlib import Path
import subprocess

from display import rendering_display

ROOT = Path(__file__).resolve().parents[3]
script = str(ROOT / "packages/tjik-gtk/scripts/test.sh")
subprocess.run(["bash", script, "--no-run"], check=True)
with rendering_display():
    subprocess.run([
        "bash", script, "local_png_load_completes_without_mapping", "--", "--ignored",
    ], env={**os.environ, "GTK_A11Y": "none"}, check=True)
