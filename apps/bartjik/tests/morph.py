"""Exercise actual vector transitions without toggling any desktop services."""

# Test-only comparison oracle; production launches the Rust binary.
import sys as _sys
from pathlib import Path as _Path
_sys.path.insert(0, str(_Path(__file__).resolve().parent / "reference"))
_sys.path.insert(0, str(_Path(__file__).resolve().parent / "reference/qml"))

import os, json, shutil, subprocess, tempfile, time, sys
from pathlib import Path
from render import wait_for

ROOT = Path(__file__).resolve().parents[1]


def main():
    assert os.environ.get("DBUS_SESSION_BUS_ADDRESS", "").startswith("unix:path=/tmp/")
    destination = Path(sys.argv[1])
    destination.mkdir(exist_ok=True, parents=True)
    with tempfile.TemporaryDirectory(prefix="bartjik-morph-") as td:
        temp = Path(td)
        shutil.copytree(ROOT / "qml", temp / "qml")
        path = temp / "qml/MorphFixture.qml"
        path.write_text(
            (ROOT / "tests/MorphFixture.qml").read_text().replace('../qml', '.')
        )

        def call(symbol="", capture=""):
            p = subprocess.run(
                [
                    "quickshell",
                    "ipc",
                    "-p",
                    str(path),
                    "call",
                    "morph",
                    "request",
                    symbol,
                    capture,
                ],
                capture_output=True,
                text=True,
            )
            if p.returncode:
                raise ValueError("not ready")
            return json.loads(p.stdout)

        with (destination / "morph.log").open("w") as log:
            process = subprocess.Popen(
                ["quickshell", "-p", str(path)], stdout=log, stderr=log
            )
            try:
                wait_for(lambda: call()["symbol"] == "coffee")
                for a, b in [
                    ("coffee", "hotel"),
                    ("notifications", "notifications_off"),
                    ("nightlight", "light_mode"),
                    ("desktop_windows", "sports_esports"),
                    ("volume_up", "headphones"),
                ]:
                    call(a)
                    time.sleep(0.25)
                    assert call(b)["running"]
                    time.sleep(0.06)
                    s = call()
                    assert 0 < s["progress"] < 1, s
                    image = destination / (a + "-morph.png")
                    call("", str(image))
                    wait_for(
                        lambda: image.exists()
                        and image.read_bytes().endswith(b"IEND\xaeB`\x82")
                    )
                    call(a)
                    wait_for(lambda: not call()["running"])
                    assert call()["symbol"] == a
                    for step in range(5):
                        frame = destination / f"{a}-frame-{step}.png"
                        subprocess.run(
                            ["quickshell", "ipc", "-p", str(path), "call", "morph", "frame", a, b, str(step / 4), str(frame)],
                            check=True, capture_output=True, text=True,
                        )
                        wait_for(lambda: frame.exists() and frame.read_bytes().endswith(b"IEND\xaeB`\x82"))
            finally:
                process.terminate()
                process.wait(timeout=5)
        errors = [
            l
            for l in (destination / "morph.log").read_text().splitlines()
            if any(k in l for k in ("WARN scene", "ERROR", "Traceback", "Binding loop"))
        ]
        assert not errors, errors
    print(
        "Five toggle pairs: real vector morph, intermediate frames, rapid reversal and clean endpoints passed"
    )


if __name__ == "__main__":
    main()
