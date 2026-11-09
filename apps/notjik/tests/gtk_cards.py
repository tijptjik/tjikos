"""Synthetic GTK card acceptance in a private Wayland session on workspace 7."""
import json
import os
import resource
from pathlib import Path
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT.parent / "runtjik/tests"))
from private_compositor import private_compositor, wait_for


def main():
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    host = dict(os.environ)
    def hypr(*args):
        return subprocess.check_output(["hyprctl", *args], env=host, text=True)
    existing = {c["address"] for c in json.loads(hypr("-j", "clients"))}
    rule = f"notjik_cards_test_{os.getpid()}"
    result = hypr("eval", f'_G.{rule}=hl.window_rule({{name="{rule}",workspace="7 silent",no_initial_focus=true,no_focus=true,match={{initial_class="^aquamarine$"}}}})')
    assert "error" not in result.lower(), result
    artifacts = Path(os.environ.get("NOTJIK_GTK_ARTIFACTS", tempfile.mkdtemp(prefix="notjik-gtk-cards-")))
    artifacts.mkdir(parents=True, exist_ok=True)
    binary = Path(os.environ.get("NOTJIK_GTK_TEST_BINARY", ROOT / "dist/bin/notjik-gtk"))
    try:
        with private_compositor(isolate_pointer=True):
            nested = wait_for(lambda: next((c for c in json.loads(hypr("-j", "clients")) if c["initialClass"] == "aquamarine" and c["address"] not in existing), None))
            assert nested["workspace"]["id"] == 7, nested
            assert json.loads(hypr("-j", "activewindow")).get("address") != nested["address"], "Preview stole host focus"
            env = {**os.environ, "NOTJIK_HOME": os.environ.get("NOTJIK_GTK_TEST_HOME", str(ROOT)), "GTK_A11Y": "none", "GSK_RENDERER": "cairo"}
            # A hidden host workspace throttles nested Wayland frame callbacks.
            # Use the private compositor's headless output for deterministic frames.
            subprocess.run(["hyprctl", "output", "create", "headless", "NOTJIK"], check=True, capture_output=True)
            subprocess.run(["hyprctl", "eval", 'hl.monitor({output="WAYLAND-1",disabled=true}); hl.monitor({output="NOTJIK",mode="1000x900@60",position="0x0",scale=1}); hl.dispatch(hl.dsp.focus({monitor="NOTJIK"}))'], check=True, capture_output=True)
            wait_for(lambda: any(m["name"] == "NOTJIK" for m in json.loads(subprocess.check_output(["hyprctl", "-j", "monitors"], text=True))))
            subprocess.run(["hyprctl", "eval", 'hl.window_rule({name="notjik-gallery",float=true,no_anim=true,match={initial_class="^org.tjikos.notjik.cards$"}})'], check=True, capture_output=True)
            # Use only the installed/default bundled fixtures: no test-only media overrides.
            for theme, width in [("rose-punk", 460), ("monochrome", 380)]:
                folder = artifacts / theme
                folder.mkdir(exist_ok=True)
                result = subprocess.run([str(binary), "--demo", "--self-test", "--theme", theme, "--width", str(width), "--screenshot", str(folder / "gallery.png"), "--snapshot-dir", str(folder)], env=env, text=True, stdout=(folder / "stdout.log").open("w"), stderr=(folder / "stderr.log").open("w"), timeout=55)
                result.stdout = (folder / "stdout.log").read_text()
                result.stderr = (folder / "stderr.log").read_text()
                (folder / "run.log").write_text(result.stdout + result.stderr)
                assert result.returncode == 0, result.stdout + result.stderr
                assert "Gtk-CRITICAL" not in result.stderr and "Gtk-WARNING" not in result.stderr, result.stderr
                assert len(list(folder.glob("card-*.png"))) == 19
                print(result.stdout.strip())
            from gtk_card_reference import capture_reference
            capture_reference(ROOT, artifacts, env)
            print(f"GTK artifacts: {artifacts}")
    finally:
        result = hypr("eval", f'if _G.{rule} then _G.{rule}:set_enabled(false); _G.{rule}=nil end')
        assert "error" not in result.lower(), result


if __name__ == "__main__":
    main()
