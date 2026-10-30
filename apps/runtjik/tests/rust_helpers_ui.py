"""Private countdown/lightbox UI and synthetic desktop Trash roundtrip."""

import os, sys, json, subprocess, time, tempfile
from pathlib import Path

from private_compositor import private_compositor
from rust_parity import APP, Backend, fixture

with private_compositor():
    root = Path(os.environ["XDG_RUNTIME_DIR"]).parent
    marker = root / "cancelled-countdown-must-not-exist"
    env = {
        **os.environ,
        "RUNTJIK_DEMO": "1",
        "RUNTJIK_COUNTDOWN_SECONDS": "8",
        "RUNTJIK_COUNTDOWN_SUCCESS": str(marker),
    }
    with (root / "countdown.log").open("w") as log:
        p = subprocess.Popen(
            ["quickshell", "--path", str(APP / "qml/countdown")],
            env=env,
            stdout=log,
            stderr=log,
        )
        try:
            for _ in range(100):
                layers = subprocess.check_output(["hyprctl", "-j", "layers"], text=True)
                if "runtjik-countdown" in layers:
                    break
                time.sleep(0.03)
            else:
                raise AssertionError("Countdown did not map")
            subprocess.run(["wtype", "-k", "Escape"], check=True)
            assert p.wait(timeout=3) == 0
            assert not marker.exists()
            print("Countdown Escape cancellation passed")
        finally:
            if p.poll() is None:
                p.terminate()
                p.wait(timeout=3)
    with (root / "hyprland.lua").open("a") as config:
        config.write(
            '\nhl.window_rule({name="image-viewer-lightbox",float=true,center=true,match={class="^imv$"}})\n'
        )
    subprocess.run(["hyprctl", "reload"], check=True, stdout=subprocess.DEVNULL)
    image = root / "synthetic-lightbox.png"
    subprocess.run(["magick", "-size", "160x90", "xc:#41365c", str(image)], check=True)
    viewer = subprocess.Popen(
        [str(APP / "runtjik-core"), "image", str(image)],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    try:
        deadline = time.monotonic() + 8
        client = None
        while time.monotonic() < deadline:
            clients = json.loads(
                subprocess.check_output(["hyprctl", "-j", "clients"], text=True)
            )
            client = next(
                (c for c in clients if c.get("class") == "imv" and c.get("mapped")),
                None,
            )
            if client and client["size"] == [932, 612]:
                break
            time.sleep(0.05)
        else:
            raise AssertionError(("Lightbox size did not settle", client))
        subprocess.run(
            [
                "hyprctl",
                "dispatch",
                "hl.dsp.window.close({window="
                + json.dumps("address:" + client["address"])
                + "})",
            ],
            check=True,
            stdout=subprocess.DEVNULL,
        )
        assert viewer.wait(timeout=5) == 0
        print("Native image viewer sizing and targeted close passed")
    finally:
        if viewer.poll() is None:
            viewer.terminate()
            viewer.wait(timeout=3)
        viewer.stdout.close()
        viewer.stderr.close()
with tempfile.TemporaryDirectory(
    prefix="runtjik-trash-fixture-", dir=Path.home() / ".cache"
) as temp:
    root = Path(temp)
    env = fixture(root, 1)
    env["XDG_DATA_HOME"] = os.environ.get(
        "XDG_DATA_HOME", str(Path.home() / ".local/share")
    )
    captures = root / "captures"
    captures.mkdir()
    target = captures / "Synthetic-trash-fixture.png"
    target.write_bytes(b"synthetic fixture, not a capture")
    Path(env["RUNTJIK_CONFIG"]).write_text(
        '[plugins]\nenabled=["Screenshots"]\n[plugin_settings.Screenshots]\ndirectory='
        + json.dumps(str(captures))
        + "\n"
    )
    backend = Backend("rust", env)
    try:
        rows = backend.request(op="query", query="/grab history")["results"]
        reply = backend.request(op="activate", id=rows[0]["id"], mode="close")
        assert reply["kind"] != "error", reply
        assert not target.exists()
        reply = backend.request(op="undo-trash", query="/grab history")
        assert reply["kind"] != "error", reply
        assert target.read_bytes() == b"synthetic fixture, not a capture"
        print("Native desktop Trash and undo roundtrip passed")
    finally:
        backend.close()
