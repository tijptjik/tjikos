"""Measure synthetic task/reading collections on a silent workspace-7 display."""
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "packages/tjik-gtk/scripts"))
from display import rendering_display

native = ROOT / "packages/tjik-gtk/target/native/lib"
env = {**os.environ, "GTK_A11Y": "none", "GSK_RENDERER": "cairo",
       "PKG_CONFIG_PATH": str(native / "pkgconfig"), "LD_LIBRARY_PATH": str(native)}
command = ["cargo", "test", "--manifest-path", str(ROOT / "packages/tasktjik-gtk/Cargo.toml"),
           "--release", "--locked", "--lib", "rendered_collection_profile"]
subprocess.run([*command, "--no-run"], env=env, check=True)
with rendering_display():
    subprocess.run([*command, "--", "--ignored", "--nocapture"], env={**os.environ, **{k: env[k] for k in
                   ("GTK_A11Y", "GSK_RENDERER", "PKG_CONFIG_PATH", "LD_LIBRARY_PATH")}}, check=True)
