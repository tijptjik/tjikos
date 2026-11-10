"""Exercise GTK callback lifecycle on a private display placed on workspace 7."""
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[3]
from display import rendering_display

# Build before opening the short-lived test display. The caller owns dev-check.
subprocess.run(["bash", str(ROOT / "packages/tjik-gtk/scripts/test.sh"), "--no-run"], check=True)
with rendering_display():
    subprocess.run([
        "bash", str(ROOT / "packages/tjik-gtk/scripts/test.sh"),
        "closing_pulse_cancels_on_rebind_unmap_and_drop", "--", "--ignored",
    ], env={**os.environ, "GTK_A11Y": "none"}, check=True)
