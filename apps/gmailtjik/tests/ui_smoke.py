"""Real GPUI smoke capture on an isolated Wayland output; synthetic mail only."""
import json
import os
from pathlib import Path
import subprocess
import sys
import time

APP = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(APP.parent / "runtjik/tests"))
from private_compositor import private_compositor, wait_for


with private_compositor(isolate_pointer=True):
    root = Path(os.environ["XDG_RUNTIME_DIR"]).parent
    subprocess.run(["hyprctl", "output", "create", "headless", "GMAIL"], check=True, stdout=subprocess.DEVNULL)
    subprocess.run(["hyprctl", "eval", 'hl.monitor({output="GMAIL",mode="1440x1000@60",position="-3000x0",scale=1})'], check=True, stdout=subprocess.DEVNULL)
    subprocess.run(["hyprctl", "eval", 'hl.dispatch(hl.dsp.focus({monitor="GMAIL"}))'], check=True, stdout=subprocess.DEVNULL)
    pointer_source = APP.parent / "runtjik/tests"
    subprocess.run(["wayland-scanner", "client-header", str(pointer_source / "virtual_pointer.xml"), str(root / "pointer.h")], check=True)
    subprocess.run(["wayland-scanner", "private-code", str(pointer_source / "virtual_pointer.xml"), str(root / "protocol.c")], check=True)
    flags = subprocess.check_output(["pkg-config", "--cflags", "--libs", "wayland-client"], text=True).split()
    subprocess.run(["cc", str(pointer_source / "pointer.c"), str(root / "protocol.c"), "-I" + str(root), "-o", str(root / "pointer"), *flags], check=True)
    binary = Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else APP / "target/debug/gmailtjik"
    with (root / "gmailtjik.log").open("w") as log:
        process = subprocess.Popen([str(binary), "--demo"], stdout=log, stderr=log)
        try:
            def client():
                if process.poll() is not None:
                    raise RuntimeError((root / "gmailtjik.log").read_text())
                return next((c for c in json.loads(subprocess.check_output(["hyprctl", "-j", "clients"], text=True)) if c["title"] == "Gmailtjik"), None)

            window = wait_for(client, timeout=30)
            subprocess.run(["hyprctl", "eval", 'hl.dispatch(hl.dsp.focus({window="title:Gmailtjik"}))'], check=True, stdout=subprocess.DEVNULL)
            time.sleep(1)
            subprocess.run(["hyprctl", "dismissnotify", "-1"], stdout=subprocess.DEVNULL)
            subprocess.run(["grim", "-o", "GMAIL", str(root / "inbox.png")], check=True)
            print("GMAILTJIK_PREVIEW=" + str(root), flush=True)
            print("WINDOW=" + json.dumps(window), flush=True)
            click_pointer = subprocess.Popen([str(root / "pointer"), "GMAIL"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
            def click(x, y):
                move = f"m {round(x / 1440 * 10000)} {round(y / 1000 * 10000)}"
                for operation in [move, move, "b 1", "b 0"]:
                    click_pointer.stdin.write(operation + "\n")
                    click_pointer.stdin.flush()
                    assert click_pointer.stdout.readline().strip() == "ok"
                    time.sleep(.15)

            def capture(name):
                time.sleep(.3)
                subprocess.run(["grim", "-o", "GMAIL", str(root / name)], check=True)

            if os.environ.get("GMAILTJIK_DEMO_SCENE") == "saved-draft":
                # Startup must show the reader even with a recovered local draft.
                click(1320, 48)
                capture("saved-draft-open.png")
                click(1320, 106)
                capture("saved-draft-closed.png")
                subprocess.run(["wtype", "-s", "150", "-M", "ctrl", "-k", "n", "-m", "ctrl"], check=True)
                capture("saved-draft-resumed.png")
                assert "panicked" not in (root / "gmailtjik.log").read_text()
                sys.exit(0)

            if os.environ.get("GMAILTJIK_DEMO_SCENE") == "list-actions":
                click(285, 145)
                capture("unread-only.png")
                click(400, 145)
                capture("all-read.png")
                click(285, 145)
                capture("filter-off.png")
                for before, after in [("inbox.png", "unread-only.png"), ("unread-only.png", "all-read.png"), ("all-read.png", "filter-off.png")]:
                    assert (root / before).read_bytes() != (root / after).read_bytes(), (before, after)
                assert "panicked" not in (root / "gmailtjik.log").read_text()
                sys.exit(0)

            if os.environ.get("GMAILTJIK_DEMO_SCENE") == "label-counts":
                click(885, 106)
                capture("counts-after-read.png")
                click(100, 912)
                capture("manage-labels.png")
                click(195, 137)
                capture("label-visibility.png")
                assert (root / "inbox.png").read_bytes() != (root / "counts-after-read.png").read_bytes()
                assert "panicked" not in (root / "gmailtjik.log").read_text()
                sys.exit(0)

            if os.environ.get("GMAILTJIK_DEMO_SCENE") == "action-buttons":
                click(805, 106)
                capture("delete-button.png")
                click(80, 342)
                capture("trash-folder-after-button.png")
                assert (root / "inbox.png").read_bytes() != (root / "delete-button.png").read_bytes()
                assert "panicked" not in (root / "gmailtjik.log").read_text()
                sys.exit(0)

            if os.environ.get("GMAILTJIK_DEMO_SCENE") == "actions":
                subprocess.run(["wtype", "-s", "150", "-k", "Escape", "r", "Reply body: r a d stay as text."], check=True)
                capture("reply-focused.png")
                subprocess.run(["wtype", "-s", "150", "-k", "Escape", "r", " Resumed."], check=True)
                capture("reply-resumed.png")
                subprocess.run(["wtype", "-s", "150", "-k", "Escape", "a"], check=True)
                capture("archived.png")
                subprocess.run(["wtype", "-s", "150", "d"], check=True)
                capture("trashed.png")
                # Draft protection: replying to the next message must retain the old draft.
                subprocess.run(["wtype", "-s", "150", "r"], check=True)
                capture("draft-protected.png")
                click(80, 342)
                capture("trash-folder.png")
                for before, after in [("reply-focused.png", "reply-resumed.png"), ("archived.png", "trashed.png")]:
                    assert (root / before).read_bytes() != (root / after).read_bytes(), (before, after)
                assert "panicked" not in (root / "gmailtjik.log").read_text()
                sys.exit(0)

            if os.environ.get("GMAILTJIK_DEMO_SCENE") == "settings-layout":
                capture("mail-sidebar.png")
                click(55, 945)
                capture("settings-rules.png")
                click(110, 205)
                capture("settings-reading.png")
                click(110, 240)
                capture("settings-sync.png")
                assert "panicked" not in (root / "gmailtjik.log").read_text()
                sys.exit(0)

            if os.environ.get("GMAILTJIK_DEMO_SCENE") == "rules":
                capture("rules.png")
                click(650, 485)
                time.sleep(.5)
                subprocess.run(["hyprctl", "eval", 'hl.dispatch(hl.dsp.focus({window="title:Gmailtjik"}))'], check=True, stdout=subprocess.DEVNULL)
                subprocess.run(["wl-copy", "--", "login.example.test"], check=True)
                subprocess.run(["wtype", "-s", "150", "-M", "ctrl", "-k", "v", "-m", "ctrl"], check=True)
                capture("rules-typed.png")
                click(375, 170)
                capture("rules-saved.png")
                click(370, 282)
                capture("rules-inbox.png")
                click(370, 247)
                capture("rules-retained.png")
                assert (root / "rules.png").read_bytes() != (root / "rules-retained.png").read_bytes()
                assert "panicked" not in (root / "gmailtjik.log").read_text()
                sys.exit(0)

            if os.environ.get("GMAILTJIK_DEMO_SCENE") == "html":
                time.sleep(2)
                capture("html-before.png")
                pointer = subprocess.Popen([str(root / "pointer"), "GMAIL"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
                try:
                    operations = ["m 3972 4500", "m 3972 4500", "b 1"]
                    operations += [f"m {x} 4500" for x in range(3972, 5001, 100)]
                    operations += ["b 0"]
                    for operation in operations:
                        pointer.stdin.write(operation + "\n")
                        pointer.stdin.flush()
                        assert pointer.stdout.readline().strip() == "ok"
                        time.sleep(.18)
                finally:
                    pointer.stdin.close()
                    pointer.wait(timeout=5)
                time.sleep(2)
                capture("html-resized.png")
                click(1120, 735)
                time.sleep(1)
                capture("html-quotes.png")
                click(1280, 203)
                time.sleep(1)
                capture("html-light.png")
                click(1060, 311)
                capture("html-text.png")
                assert "panicked" not in (root / "gmailtjik.log").read_text()
                sys.exit(0)

            if os.environ.get("GMAILTJIK_DEMO_SCENE") == "labels":
                subprocess.run(["wtype", "-s", "150", "-k", "Escape", "-k", "Left", "s"], check=True)
                capture("label-visibility-menu.png")
                subprocess.run(["wtype", "-s", "150", "-k", "Down", "-k", "Down", "-k", "Return"], check=True)
                capture("label-hidden.png")
                assert (root / "label-visibility-menu.png").read_bytes() != (root / "label-hidden.png").read_bytes()
                assert "panicked" not in (root / "gmailtjik.log").read_text()
                sys.exit(0)

            if os.environ.get("GMAILTJIK_DEMO_SCENE") == "contacts":
                subprocess.run(["wtype", "-s", "150", "Mira"], check=True)
                time.sleep(.6)
                capture("contacts-people.png")
                subprocess.run(["wtype", "-s", "150", "-k", "Down", "-k", "Return"], check=True)
                capture("contacts-addresses.png")
                subprocess.run(["wtype", "-s", "150", "-k", "Down", "d"], check=True)
                capture("contact-default.png")
                subprocess.run(["wtype", "-s", "150", "d"], check=True)
                capture("contact-default-cleared.png")
                subprocess.run(["wtype", "-s", "150", "d", "-k", "Down", "-k", "Return"], check=True)
                capture("contact-selected.png")
                subprocess.run(["wtype", "-s", "150", "-M", "ctrl", "-k", "a", "-m", "ctrl", "Mira"], check=True)
                time.sleep(.6)
                subprocess.run(["wtype", "-s", "150", "-k", "Down", "-k", "Return", "-k", "Return"], check=True)
                capture("contact-default-selected.png")
                assert (root / "contacts-people.png").read_bytes() != (root / "contacts-addresses.png").read_bytes()
                assert (root / "contacts-addresses.png").read_bytes() != (root / "contact-selected.png").read_bytes()
                assert "panicked" not in (root / "gmailtjik.log").read_text()
                sys.exit(0)

            subprocess.run(["wtype", "-s", "150", "-k", "Escape", "-k", "Right", *sum((["-k", "Down"] for _ in range(12)), [])], check=True)
            capture("thread-reply.png")
            subprocess.run(["wtype", "-s", "150", *sum((["-k", "Up"] for _ in range(12)), []), "-k", "Left"], check=True)
            subprocess.run(["wtype", "-s", "150", "-k", "Escape", "-k", "Left", *sum((["-k", "Down"] for _ in range(5)), []), "-k", "Return"], check=True)
            capture("folders-collapsed.png")
            subprocess.run(["wtype", "-s", "150", "-k", "Return"], check=True)
            capture("folders-expanded.png")
            subprocess.run(["wtype", "-s", "150", *sum((["-k", "Up"] for _ in range(5)), []), "-k", "Right"], check=True)
            subprocess.run(["wtype", "-s", "150", "?"], check=True)
            capture("shortcuts.png")
            subprocess.run(["wtype", "-s", "150", "-k", "Escape", "-k", "Down"], check=True)
            capture("keyboard-next.png")
            subprocess.run(["wtype", "-s", "150", "-k", "Left", "-k", "Down"], check=True)
            capture("keyboard-label.png")
            subprocess.run(["wtype", "-s", "150", "-k", "Up", "-k", "Right", "-k", "Up"], check=True)
            subprocess.run(["wtype", "-s", "150", "-M", "ctrl", "-k", "f", "-m", "ctrl", "sea", "-k", "Left", "X"], check=True)
            capture("input-arrows.png")
            subprocess.run(["wtype", "-s", "150", "-M", "ctrl", "-k", "a", "-m", "ctrl", "-k", "BackSpace", "-k", "Escape"], check=True)
            subprocess.run(["wtype", "-s", "150", "-M", "ctrl", "-k", "n", "-m", "ctrl"], check=True)
            capture("keyboard-compose.png")
            click(115, 118)
            capture("composer.png")
            click(780, 910)
            subprocess.run(["wtype", "-s", "150", "Reply warmly and promise notes before Thursday.", "-k", "Return"], check=True)
            capture("proposal.png")
            click(980, 904)
            capture("applied-draft.png")
            click(1320, 106)
            click(500, 48)
            subprocess.run(["wtype", "-s", "150", "sea", "-k", "Return"], check=True)
            capture("search.png")
            click(115, 118)
            capture("retained-draft.png")
            # These captures are intended for visual QA, not semantic proof of Gmail sync.
            assert (root / "inbox.png").read_bytes() != (root / "search.png").read_bytes()
            # Keep the isolated window alive for additional manual inspection.
            if os.environ.get("GMAILTJIK_PREVIEW_HOLD") == "1":
                time.sleep(600)
            assert "panicked" not in (root / "gmailtjik.log").read_text()
        finally:
            if "click_pointer" in locals() and click_pointer.poll() is None:
                click_pointer.stdin.close()
                click_pointer.wait(timeout=5)
            process.terminate()
            process.wait(timeout=8)
