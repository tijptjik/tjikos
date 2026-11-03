"""Render synthetic PTY jobs with Kitty's real parser and reflow engine.

Run from this app: kitty +runpy 'import runpy; runpy.run_path("tests/terminal_reflow.py", run_name="__main__")'
TJIKUP_TEST_BINARY can select the release build instead of target/debug/tjikup.
No display server, accounts, package managers or real update jobs are used.
"""
import fcntl
import json
import os
from pathlib import Path
import pty
import select
import struct
import subprocess
import tempfile
import termios
import time

from kitty.fast_data_types import Screen

APP = Path(__file__).resolve().parents[1]
BINARY = Path(os.environ.get("TJIKUP_TEST_BINARY", APP / "target/debug/tjikup"))
FRAME_END = b"\x1b[?2026l"
SPINNERS = set("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")


def require(condition, message):
    # Kitty embeds optimized Python, so ordinary assert statements are disabled.
    if not condition:
        raise AssertionError(message)


def check(graphics, compact, repositories):
    with tempfile.TemporaryDirectory(prefix="tjikup-reflow-") as directory:
        root = Path(directory)
        worker = ["sh", "-c", "printf x >> starts; while [ ! -e release ]; do sleep 0.05; done"]
        recipes = []
        if repositories:
            recipes.append({"id": "repo", "label": "Repository", "apply": worker,
                            "display": {"repository": "dotjik", "operation": "pull"}})
        if repositories != "only":
            recipes.append({"id": "rpm", "label": "Fedora (RPM packages)", "apply": worker,
                            "display": {"icon": "Fedora"}})
        config = {"version": 2, "jobs": 2, "recipes": recipes,
                  "presentation": {"icons": str(APP / "tests/fixtures/icons32.json"),
                                   "glyphs": {"Fedora": "F"}}}
        (root / "config.json").write_text(json.dumps(config))
        master, slave = pty.openpty()
        screen = Screen(None, 40, 100, 1000, 10, 20)

        def resize(columns, rows):
            screen.resize(rows, columns)
            fcntl.ioctl(master, termios.TIOCSWINSZ,
                        struct.pack("HHHH", rows, columns, columns * 10, rows * 20))

        resize(100, 40)
        env = dict(os.environ, TERM="xterm-kitty", TJIKUP_COLOR="1", TJIKUP_NOTIFY="0",
                   TJIKUP_KITTY_ICONS=str(int(graphics)), TJIKUP_COMPACT=str(int(compact)),
                   XDG_STATE_HOME=str(root / "state"), HERDR_ENV="0", TERM_PROGRAM="kitty")
        for key in ("TMUX", "STY", "TJIKUP_NO_SPIN", "TJIKUP_ICONS"):
            env.pop(key, None)

        def session():
            os.setsid()
            fcntl.ioctl(0, termios.TIOCSCTTY, 0)

        child = subprocess.Popen([str(BINARY), "--config", str(root / "config.json")],
                                 cwd=root, env=env, stdin=slave, stdout=slave, stderr=slave,
                                 preexec_fn=session)
        os.close(slave)
        pending = bytearray()
        frames = 0

        def render(data):
            while data:
                buffer = screen.test_create_write_buffer()
                count = screen.test_commit_write_buffer(data, buffer)
                data = data[count:]
                screen.test_parse_written_data()

        def poll():
            nonlocal frames
            if select.select([master], [], [], 0.02)[0]:
                try:
                    pending.extend(os.read(master, 65536))
                except OSError:
                    return
                # Inspect completed synchronized frames, as the terminal UI does.
                # A PTY read can split an escape or stop halfway through a frame.
                while (end := pending.find(FRAME_END)) >= 0:
                    end += len(FRAME_END)
                    render(bytes(pending[:end]))
                    del pending[:end]
                    frames += 1

        def wait_frames():
            target = frames + 3
            deadline = time.monotonic() + 5
            while frames < target:
                poll()
                require(child.poll() is None, 'synthetic worker exited before release')
                require(time.monotonic() < deadline, 'missing activity frames')

        def text():
            return "\n".join(str(screen.line(i)) for i in range(screen.lines))

        def transcript():
            history = [str(screen.historybuf.line(i)) for i in reversed(range(screen.historybuf.count))]
            return "\n".join(history + [text()])

        try:
            wait_frames()
            # Width, height only, short panes, and repeated expansion/contraction.
            for step, (columns, rows) in enumerate([(100, 40), (48, 40), (120, 40), (32, 12),
                                                    (32, 8), (100, 8), (100, 40), (48, 40), (100, 40)]):
                resize(columns, rows)
                wait_frames()
                visible = text()
                context = f"graphics={graphics}, compact={compact}, repos={repositories}, {columns}x{rows}\n{visible}"
                require(screen.cursor.x == 0, context)
                # Reflow leaves exactly one historical table; no new table is
                # patched into it. Only the last activity row keeps changing.
                require(transcript().count("dotjik") == int(bool(repositories)), context)
                activity = "\n".join(str(screen.line(i)) for i in range(screen.cursor.y, screen.lines))
                has_activity = repositories != "only" or step > 0
                label = "Repository" if repositories and step > 0 else "Fedora"
                require(activity.count(label) == int(has_activity), context)
                require(sum(activity.count(c) for c in SPINNERS) == int(has_activity), context)
                if graphics and has_activity:
                    expected_rows = 1
                    require(sum("\U0010eeee" in line for line in activity.splitlines()) == expected_rows, context)
            (root / "release").touch()
            deadline = time.monotonic() + 5
            while child.poll() is None:
                poll()
                require(time.monotonic() < deadline, 'worker did not finish')
            poll()
            render(bytes(pending))
            require(child.returncode == 0, text())
            require((root / 'starts').read_text() == 'x' * len(recipes), 'resize restarted a job')
            require('ALL SYSTEMS GO' in text(), text())
            require("Repository" not in transcript(), transcript())
            require(transcript().count("dotjik") == 2 * int(bool(repositories)), transcript())
            require(transcript().count("Fedora") == int(repositories != "only"), transcript())
            if repositories:
                require("✓ PULL" in text(), text())
            else:
                require(not any(c in transcript() for c in SPINNERS), transcript())
            print(f"PASS graphics={graphics} compact={compact} repositories={repositories}")
        finally:
            if child.poll() is None:
                child.kill()
            child.wait()
            os.close(master)


if __name__ == "__main__":
    for graphics, compact in [(False, True), (True, True), (True, False)]:
        for repositories in (False, "only", "mixed"):
            check(graphics, compact, repositories)
