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
    output_height = 1400 if os.environ.get("GMAILTJIK_DEMO_SCENE") in ("inline", "signature") else 1000
    subprocess.run(["hyprctl", "output", "create", "headless", "GMAIL"], check=True, stdout=subprocess.DEVNULL)
    subprocess.run(["hyprctl", "eval", f'hl.monitor({{output="GMAIL",mode="1440x{output_height}@60",position="-3000x0",scale=1}})'], check=True, stdout=subprocess.DEVNULL)
    subprocess.run(["hyprctl", "eval", 'hl.dispatch(hl.dsp.focus({monitor="GMAIL"}))'], check=True, stdout=subprocess.DEVNULL)
    pointer_source = APP.parent / "runtjik/tests"
    subprocess.run(["wayland-scanner", "client-header", str(pointer_source / "virtual_pointer.xml"), str(root / "pointer.h")], check=True)
    subprocess.run(["wayland-scanner", "private-code", str(pointer_source / "virtual_pointer.xml"), str(root / "protocol.c")], check=True)
    flags = subprocess.check_output(["pkg-config", "--cflags", "--libs", "wayland-client"], text=True).split()
    subprocess.run(["cc", str(pointer_source / "pointer.c"), str(root / "protocol.c"), "-I" + str(root), "-o", str(root / "pointer"), *flags], check=True)
    binary = Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else APP / "target/debug/gmailtjik"
    with (root / "gmailtjik.log").open("w") as log:
        process = subprocess.Popen([str(binary), "--demo"], stdout=log, stderr=log)
        process_started = time.monotonic()
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
                move = f"m {round(x / 1440 * 10000)} {round(y / output_height * 10000)}"
                for operation in [move, move, "b 1", "b 0"]:
                    click_pointer.stdin.write(operation + "\n")
                    click_pointer.stdin.flush()
                    assert click_pointer.stdout.readline().strip() == "ok"
                    time.sleep(.15)

            def capture(name):
                time.sleep(.3)
                subprocess.run(["grim", "-o", "GMAIL", str(root / name)], check=True)

            if os.environ.get("GMAILTJIK_DEMO_SCENE") == "reader-indents":
                capture("reader-indents.png")
                subprocess.run(["wl-copy", "--", "selection-pending"], check=True)
                # Focus the selectable readable body, then copy its full content.
                for operation in ["m 4306 2900", "b 1", "m 4792 2900", "b 0"]:
                    click_pointer.stdin.write(operation + "\n")
                    click_pointer.stdin.flush()
                    assert click_pointer.stdout.readline().strip() == "ok"
                    time.sleep(.15)
                subprocess.run(["wtype", "-M", "ctrl", "-k", "a", "-k", "c", "-m", "ctrl"], check=True)
                def copied_indents():
                    value = subprocess.check_output(["wl-paste", "--no-newline"], text=True, timeout=5)
                    return value if value != "selection-pending" else None
                copied = wait_for(copied_indents, timeout=5)
                assert "Renáta Exampleová" in copied and "你好" in copied, copied
                assert "Studio 12" in copied and "Markdown" in copied, copied
                click(1241, 257)
                time.sleep(1)
                capture("reader-indents-original.png")
                from PIL import Image, ImageChops
                with Image.open(root / "reader-indents.png") as readable, Image.open(root / "reader-indents-original.png") as original:
                    assert ImageChops.difference(readable.convert("RGB").crop((600, 280, 1300, 620)), original.convert("RGB").crop((600, 280, 1300, 620))).getbbox()
                assert "panicked" not in (root / "gmailtjik.log").read_text()
                sys.exit(0)

            if os.environ.get("GMAILTJIK_DEMO_SCENE") == "rich-reader":
                capture("rich-original.png")
                click(1241, 253)
                time.sleep(1)
                capture("rich-colours.png")
                click(1241, 253)
                click(1210, 253)
                capture("rich-readable.png")
                from PIL import Image
                def light_fraction(name):
                    with Image.open(root / name) as shot:
                        pixels = list(shot.convert("RGB").crop((620, 300, 1350, 750)).getdata())
                    return sum(min(pixel) > 220 for pixel in pixels) / len(pixels)
                assert light_fraction("rich-original.png") < .05
                assert light_fraction("rich-colours.png") > .5
                assert light_fraction("rich-readable.png") < .05
                assert (root / "rich-original.png").read_bytes() != (root / "rich-readable.png").read_bytes()
                assert "panicked" not in (root / "gmailtjik.log").read_text()
                sys.exit(0)

            if os.environ.get("GMAILTJIK_DEMO_SCENE") == "empty-style":
                click(90, 234)
                capture("no-mail-empty-states.png")
                assert "panicked" not in (root / "gmailtjik.log").read_text()
                sys.exit(0)

            if os.environ.get("GMAILTJIK_EXPAND_CHECK") == "1":
                capture("thread-initial.png")
                click(1330, 205)
                click(590, 200)
                capture("thread-all-expanded.png")
                click(1330, 205)
                click(590, 200)
                capture("thread-all-collapsed.png")
                assert (root / "thread-all-expanded.png").read_bytes() != (root / "thread-all-collapsed.png").read_bytes()
                assert "panicked" not in (root / "gmailtjik.log").read_text()
                sys.exit(0)

            if os.environ.get("GMAILTJIK_ICON_CHECK") == "1":
                capture("message-toggle-icons.png")
                click(1136, 253)
                click(590, 200)
                capture("signature-icon-expanded.png")
                click(1160, 253)
                click(590, 200)
                capture("both-icons-expanded.png")
                click(1136, 253)
                click(1160, 253)
                click(590, 200)
                capture("icons-recollapsed.png")
                from PIL import Image
                def active_icon(name, x):
                    with Image.open(root / name) as shot:
                        return any(r > 200 and b > 100 and g < 150 for r, g, b in shot.convert("RGB").crop((x - 8, 246, x + 8, 261)).get_flattened_data())
                assert active_icon("signature-icon-expanded.png", 1136)
                assert active_icon("both-icons-expanded.png", 1160)
                assert not active_icon("icons-recollapsed.png", 1136)
                assert not active_icon("icons-recollapsed.png", 1160)
                assert "panicked" not in (root / "gmailtjik.log").read_text()
                sys.exit(0)

            if os.environ.get("GMAILTJIK_DEMO_SCENE") == "optimistic":
                from PIL import Image

                def selected_body():
                    subprocess.run(["wl-copy", "--", "selection-not-copied"], check=True)
                    click(650, 310)
                    subprocess.run(["wtype", "-M", "ctrl", "-k", "a", "-k", "c", "-m", "ctrl"], check=True)
                    return wait_for(lambda: (value if (value := subprocess.check_output(
                        ["wl-paste", "--no-newline"], text=True, timeout=5)) != "selection-not-copied" else None), timeout=5)

                started = time.monotonic()
                subprocess.run(["wtype", "-s", "70", "-k", "Escape", "a", "a"], check=True)
                capture("optimistic-archived-before-api.png")
                assert time.monotonic() - started < 2, "UI waited for synthetic 6-second delivery"
                assert selected_body() == "Full content of synthetic conversation 03."
                click(80, 132)
                capture("optimistic-starred-during-api.png")
                assert selected_body() == "Full content of synthetic conversation 05."
                click(80, 98)
                capture("optimistic-return-during-api.png")
                assert selected_body() == "Full content of synthetic conversation 03."
                # First archive fails, second succeeds. The failed row returns
                # without moving selection or undoing the later archive.
                time.sleep(max(0, started + 8 - time.monotonic()))
                capture("optimistic-failure.png")
                assert selected_body() == "Full content of synthetic conversation 03."
                with Image.open(root / "optimistic-failure.png") as shot:
                    pixels = shot.convert("RGB").crop((960, 620, 1405, 950)).get_flattened_data()
                    assert sum(pixel == (235, 111, 146) for pixel in pixels) > 20, "Failure notice missing"
                # Rapid opposite toggles are visible immediately and preserve
                # their final state while delivery remains in progress.
                click(957, 106)
                click(957, 106)
                click(890, 106)
                click(890, 106)
                capture("optimistic-opposite-stars.png")
                click(80, 132)
                capture("optimistic-starred-final.png")
                assert selected_body() == "Full content of synthetic conversation 05."
                click(80, 98)
                time.sleep(3)
                capture("optimistic-inbox-final.png")
                assert selected_body() == "Full content of synthetic conversation 03."
                with Image.open(root / "optimistic-failure.png") as before, Image.open(root / "optimistic-inbox-final.png") as after:
                    def badge_ink(shot):
                        return [pixel == (165, 160, 186) for pixel in shot.convert("RGB").crop((193, 90, 214, 105)).get_flattened_data()]
                    assert badge_ink(before) == badge_ink(after), "Unread count drifted after opposite toggles"
                assert "panicked" not in (root / "gmailtjik.log").read_text()
                sys.exit(0)

            if os.environ.get("GMAILTJIK_DEMO_SCENE") == "message-header":
                def copied_body():
                    subprocess.run(["wl-copy", "--", "header-selection-pending"], check=True)
                    for operation in [f"m 4306 {round(290 / output_height * 10000)}", "b 1", f"m 4792 {round(290 / output_height * 10000)}", "b 0"]:
                        click_pointer.stdin.write(operation + "\n")
                        click_pointer.stdin.flush()
                        assert click_pointer.stdout.readline().strip() == "ok"
                        time.sleep(.15)
                    subprocess.run(["wtype", "-M", "ctrl", "-k", "a", "-k", "c", "-m", "ctrl"], check=True)
                    return wait_for(lambda: (value if (value := subprocess.check_output(
                        ["wl-paste", "--no-newline"], text=True, timeout=5)) != "header-selection-pending" else None), timeout=5)

                capture("message-header.png")
                assert "+1 202 555 0118" not in copied_body()
                click(1160, 253)  # Signature toggle in the header.
                capture("header-signature-visible.png")
                assert "+1 202 555 0118" in copied_body()
                click(1160, 253)
                assert "+1 202 555 0118" not in copied_body()
                for operation in ["m 5181 2530", "m 5181 2530"]:
                    click_pointer.stdin.write(operation + "\n")
                    click_pointer.stdin.flush()
                    assert click_pointer.stdout.readline().strip() == "ok"
                time.sleep(1)
                capture("header-to-tooltip.png")
                click(1136, 253)  # Original layout icon.
                time.sleep(2)
                capture("header-original-layout.png")
                click(1136, 253)
                time.sleep(1)
                capture("header-readable-layout.png")
                click(1330, 253)  # Reply All belongs to this message.
                capture("header-reply-all.png")
                for x, y, expected in [(750, 145, "mira@example.test"), (750, 183, "jun@example.test")]:
                    click(x, y)
                    subprocess.run(["wtype", "-M", "ctrl", "-k", "a", "-k", "c", "-m", "ctrl"], check=True)
                    assert subprocess.check_output(["wl-paste", "--no-newline"], text=True).strip() == expected

                click(1320, 106)  # Close and preserve the synthetic draft.
                subprocess.run(["hyprctl", "eval", 'hl.monitor({output="GMAIL",mode="1000x1000@60",position="-3000x0",scale=1})'], check=True, stdout=subprocess.DEVNULL)
                time.sleep(1)
                capture("header-narrow-reader.png")
                assert "panicked" not in (root / "gmailtjik.log").read_text()
                sys.exit(0)

            if os.environ.get("GMAILTJIK_DEMO_SCENE") == "empty-folders":
                from PIL import Image

                def rows(name, count):
                    with Image.open(root / name) as shot:
                        shot = shot.convert("RGB")
                        for index in range(5):
                            pixels = shot.crop((244, 152 + 92 * index, 515, 224 + 92 * index)).get_flattened_data()
                            ink = sum(pixel in ((229, 229, 229), (165, 160, 186)) for pixel in pixels)
                            assert (ink > 20) if index < count else (ink == 0), (name, index, ink)

                def empty_button(name, required=True):
                    with Image.open(root / name) as shot:
                        shot = shot.convert("RGB")
                        points = [(x, y) for y in range(82, 120) for x in range(235, 555)
                                  if shot.getpixel((x, y)) == (235, 111, 146)]
                        if not required:
                            assert not points, name
                            return
                        assert len(points) > 5, name
                        return ((min(x for x, _ in points) + max(x for x, _ in points)) // 2,
                                (min(y for _, y in points) + max(y for _, y in points)) // 2)

                def unread_button():
                    # The header action precedes Unread in Spam and Trash.
                    click(470, 101)

                rows("inbox.png", 5)
                empty_button("inbox.png", required=False)
                click(80, 234)
                capture("empty-spam-before.png")
                rows("empty-spam-before.png", 3)
                empty_button("empty-spam-before.png")
                unread_button()  # Opening the sole unread message has marked it read.
                capture("empty-spam-unread.png")
                rows("empty-spam-unread.png", 0)
                click(*empty_button("empty-spam-unread.png"))
                capture("empty-spam-done.png")
                rows("empty-spam-done.png", 0)
                unread_button()  # Read messages must also have been deleted.
                capture("empty-spam-all-done.png")
                rows("empty-spam-all-done.png", 0)
                click(80, 268)
                capture("empty-trash-before.png")
                rows("empty-trash-before.png", 2)
                click(*empty_button("empty-trash-before.png"))
                capture("empty-trash-done.png")
                rows("empty-trash-done.png", 0)
                click(80, 98)
                capture("empty-inbox-preserved.png")
                rows("empty-inbox-preserved.png", 5)
                empty_button("empty-inbox-preserved.png", required=False)
                assert "panicked" not in (root / "gmailtjik.log").read_text()
                sys.exit(0)

            if os.environ.get("GMAILTJIK_DEMO_SCENE") == "cache-failure":
                from PIL import Image

                def cached_rows(name, selected=True):
                    with Image.open(root / name) as shot:
                        shot = shot.convert("RGB")
                        for index in range(7):
                            pixels = shot.crop((245, 190 + 104 * index, 530, 278 + 104 * index)).get_flattened_data()
                            ink = sum(pixel in ((229, 229, 229), (165, 160, 186)) for pixel in pixels)
                            assert (ink > 20) if index < 6 else (ink == 0), (name, index, ink)
                        if selected:
                            assert shot.getpixel((235, 310)) == (57, 53, 75), name

                def selected_body():
                    subprocess.run(["wl-copy", "--", "selection-not-copied"], check=True)
                    click(650, 310)
                    subprocess.run(["wtype", "-M", "ctrl", "-k", "a", "-k", "c", "-m", "ctrl"], check=True)

                    def copied():
                        value = subprocess.check_output(["wl-paste", "--no-newline"], text=True, timeout=5)
                        return value if value != "selection-not-copied" else None

                    return wait_for(copied, timeout=5)

                def failure_visible(name):
                    with Image.open(root / name) as shot:
                        pixels = shot.convert("RGB").crop((230, 945, 1405, 974)).get_flattened_data()
                        assert sum(pixel == (235, 111, 146) for pixel in pixels) > 10, name

                cached_rows("inbox.png", selected=False)
                click(400, 330)
                time.sleep(max(0, process_started + 3 - time.monotonic()))
                capture("cache-after-failure.png")
                cached_rows("cache-after-failure.png")
                failure_visible("cache-after-failure.png")
                assert selected_body() == "Full content of synthetic conversation 02."
                # Retrying must retain the cached list and opened message even
                # while Gmail is unavailable, and when the retry fails as well.
                click(546, 98)
                capture("cache-refreshing.png")
                cached_rows("cache-refreshing.png")
                assert selected_body() == "Full content of synthetic conversation 02."
                time.sleep(2)
                capture("cache-retry-failed.png")
                cached_rows("cache-retry-failed.png")
                failure_visible("cache-retry-failed.png")
                assert selected_body() == "Full content of synthetic conversation 02."
                click(80, 132)
                capture("cache-other-folder.png")
                click(80, 98)
                capture("cache-folder-return.png")
                cached_rows("cache-folder-return.png")
                assert selected_body() == "Full content of synthetic conversation 02."
                assert "panicked" not in (root / "gmailtjik.log").read_text()
                sys.exit(0)

            if os.environ.get("GMAILTJIK_DEMO_SCENE") == "quick-folder":
                from PIL import Image

                def row_ink(name, index):
                    with Image.open(root / name) as shot:
                        pixels = shot.convert("RGB").crop((245, 190 + 104 * index, 530, 278 + 104 * index)).get_flattened_data()
                        return sum(pixel in ((229, 229, 229), (165, 160, 186)) for pixel in pixels)

                def selected_body():
                    subprocess.run(["wl-copy", "--", "selection-not-copied"], check=True)
                    click(650, 310)
                    subprocess.run(["wtype", "-M", "ctrl", "-k", "a", "-k", "c", "-m", "ctrl"], check=True)

                    def copied():
                        value = subprocess.check_output(["wl-paste", "--no-newline"], text=True, timeout=5)
                        return value if value != "selection-not-copied" else None

                    return wait_for(copied, timeout=5)

                # The first six rows are usable while the remaining summaries
                # are still delayed by the synthetic folder source.
                for index in range(6):
                    assert row_ink("inbox.png", index) > 20, index
                assert row_ink("inbox.png", 6) == 0
                click(400, 330)
                capture("quick-selected-early.png")
                assert selected_body() == "Full content of synthetic conversation 02."
                time.sleep(max(0, process_started + 6.5 - time.monotonic()))
                capture("quick-complete.png")
                for name in ("quick-selected-early.png", "quick-complete.png"):
                    with Image.open(root / name) as shot:
                        assert shot.convert("RGB").getpixel((235, 310)) == (57, 53, 75), name
                assert selected_body() == "Full content of synthetic conversation 02."
                assert row_ink("quick-complete.png", 6) > 20
                with Image.open(root / "inbox.png") as first, Image.open(root / "quick-complete.png") as complete:
                    assert first.crop((244, 151, 550, 177)).tobytes() != complete.crop((244, 151, 550, 177)).tobytes()
                # From row 2, exactly 23 moves reach row 25. Further Down cannot
                # select a duplicate row appended by a later progressive batch.
                subprocess.run(["wtype", "-s", "20", "-k", "Escape", *sum((["-k", "Down"] for _ in range(23)), [])], check=True)
                capture("quick-last-thread.png")
                assert selected_body() == "Full content of synthetic conversation 25."
                subprocess.run(["wtype", "-k", "Escape", "-k", "Down"], check=True)
                assert selected_body() == "Full content of synthetic conversation 25."
                assert "panicked" not in (root / "gmailtjik.log").read_text()
                sys.exit(0)

            if os.environ.get("GMAILTJIK_DEMO_SCENE") == "threads":
                # Six messages form three conversations, with counts 3, 1 and 2.
                from PIL import Image, ImageChops

                def region(name, box):
                    with Image.open(root / name) as shot:
                        return shot.convert("RGB").crop(box)

                click(1100, 200)
                capture("thread-before.png")
                # Both the newest and selected message must allow an explicit
                # collapse, overriding their default-open state, and reopen.
                click(618, 667)
                capture("thread-newest-collapsed.png")
                assert ImageChops.difference(region("thread-before.png", (600, 700, 1350, 820)),
                                            region("thread-newest-collapsed.png", (600, 700, 1350, 820))).getbbox()
                click(700, 667)
                capture("thread-newest-reopened.png")
                assert not ImageChops.difference(region("thread-before.png", (600, 700, 1350, 820)),
                                                region("thread-newest-reopened.png", (600, 700, 1350, 820))).getbbox()
                click(618, 330)
                capture("thread-selected-collapsed.png")
                assert ImageChops.difference(region("thread-before.png", (600, 390, 1350, 620)),
                                            region("thread-selected-collapsed.png", (600, 390, 1350, 620))).getbbox()
                click(700, 330)
                capture("thread-selected-reopened.png")
                assert not ImageChops.difference(region("thread-before.png", (600, 390, 1350, 620)),
                                                region("thread-selected-reopened.png", (600, 390, 1350, 620))).getbbox()
                # The enlarged list avatars show both senders, while the
                # message header retains the same sender's smaller identicon.
                from collections import Counter

                def avatar_ink(box):
                    pixels = region("thread-before.png", box).get_flattened_data()
                    colors = Counter(pixel for pixel in pixels if min(pixel) > 80 and max(pixel) - min(pixel) > 15)
                    assert colors
                    return colors.most_common(1)[0][0]

                assert avatar_ink((244, 195, 284, 235)) == avatar_ink((639, 317, 667, 345))
                assert avatar_ink((244, 195, 284, 235)) != avatar_ink((292, 195, 332, 235))
                click(885, 106)
                capture("thread-marked-read.png")
                subprocess.run(["wtype", "-s", "150", "-k", "Escape", "r"], check=True)
                capture("thread-reply.png")
                click(1320, 106)
                subprocess.run(["wtype", "-s", "150", "a"], check=True)
                capture("thread-archived.png")
                # Move down by one conversation, not by one underlying message.
                subprocess.run(["wtype", "-s", "150", "-k", "Down"], check=True)
                capture("thread-keyboard-next.png")
                subprocess.run(["wtype", "-s", "150", "d"], check=True)
                capture("thread-trashed.png")
                click(80, 234)
                capture("thread-trash-folder.png")
                assert "panicked" not in (root / "gmailtjik.log").read_text()
                sys.exit(0)

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

            if os.environ.get("GMAILTJIK_DEMO_SCENE") == "auto-read":
                time.sleep(1)
                capture("auto-read-keeps-viewer.png")
                # The current message has left the unread list; the reader stays put.
                subprocess.run(["wl-copy", "--", "not-copied"], check=True)
                click(670, 350)
                subprocess.run(["wtype", "-M", "ctrl", "-k", "a", "-k", "c", "-m", "ctrl"], check=True)
                body = subprocess.check_output(["wl-paste", "--no-newline"], text=True)
                assert "first notes for our new mail client" in body
                time.sleep(1)
                capture("auto-read-no-cascade.png")
                click(885, 106)
                time.sleep(1)
                capture("manual-unread-stays.png")
                assert "panicked" not in (root / "gmailtjik.log").read_text()
                sys.exit(0)

            if os.environ.get("GMAILTJIK_DEMO_SCENE") == "footer-actions":
                time.sleep(2)
                capture("footer-actions.png")
                assert "panicked" not in (root / "gmailtjik.log").read_text()
                sys.exit(0)

            if os.environ.get("GMAILTJIK_DEMO_SCENE") == "inline-toolbar":
                capture("inline-toolbar.png")
                click(476, 95)
                capture("inline-unread.png")
                click(527, 95)
                capture("inline-mark-read.png")
                for operation in ["m 3972 4500", "m 3972 4500", "b 1"] + [f"m {x} 4500" for x in range(3972, 3071, -100)] + ["b 0"]:
                    click_pointer.stdin.write(operation + "\n")
                    click_pointer.stdin.flush()
                    assert click_pointer.stdout.readline().strip() == "ok"
                    time.sleep(.2)
                capture("inline-toolbar-narrow.png")
                assert (root / "inline-toolbar.png").read_bytes() != (root / "inline-unread.png").read_bytes()
                assert (root / "inline-unread.png").read_bytes() != (root / "inline-mark-read.png").read_bytes()
                assert "panicked" not in (root / "gmailtjik.log").read_text()
                sys.exit(0)

            if os.environ.get("GMAILTJIK_DEMO_SCENE") == "avatar-stack":
                capture("avatar-stack.png")
                click(425, 176)
                capture("stack-contact.png")
                click(920, 533)
                assert subprocess.check_output(["wl-paste", "--no-newline"], text=True) == "mira@example.test"
                subprocess.run(["wtype", "-k", "Escape"], check=True)
                for operation in ["m 3972 4500", "m 3972 4500", "b 1"] + [f"m {x} 4500" for x in range(3972, 3071, -100)] + ["b 0"]:
                    click_pointer.stdin.write(operation + "\n")
                    click_pointer.stdin.flush()
                    assert click_pointer.stdout.readline().strip() == "ok"
                    time.sleep(.2)
                capture("avatar-stack-narrow.png")
                assert "panicked" not in (root / "gmailtjik.log").read_text()
                sys.exit(0)

            if os.environ.get("GMAILTJIK_DEMO_SCENE") == "sender-details":
                click(650, 255)
                capture("reader-contact-modal.png")
                click(920, 533)
                assert subprocess.check_output(["wl-paste", "--no-newline"], text=True) == "mira@example.test"
                subprocess.run(["wtype", "-k", "Escape"], check=True)
                click(618, 255)
                capture("collapsed-message.png")
                click(650, 255)
                capture("collapsed-contact-modal.png")
                click(920, 533)
                assert subprocess.check_output(["wl-paste", "--no-newline"], text=True) == "mira@example.test"
                subprocess.run(["wtype", "-k", "Escape"], check=True)
                capture("still-collapsed-message.png")
                click(800, 255)
                click(509, 216)
                capture("list-contact-modal.png")
                click(920, 533)
                assert subprocess.check_output(["wl-paste", "--no-newline"], text=True) == "mira@example.test"
                subprocess.run(["wtype", "-k", "Escape"], check=True)
                click(670, 322)
                capture("unsubscribe-modal.png")
                click(700, 540)
                capture("unsubscribe-demo-open.png")
                assert "panicked" not in (root / "gmailtjik.log").read_text()
                sys.exit(0)

            if os.environ.get("GMAILTJIK_DEMO_SCENE") == "all-mail":
                click(80, 268)
                capture("all-mail.png")
                click(90, 950)
                capture("manage-labels-icon.png")
                assert "panicked" not in (root / "gmailtjik.log").read_text()
                sys.exit(0)

            if os.environ.get("GMAILTJIK_DEMO_SCENE") == "label-counts":
                click(885, 106)
                capture("counts-after-read.png")
                click(90, 950)
                capture("manage-labels.png")
                click(195, 98)
                capture("label-visibility.png")
                assert (root / "inbox.png").read_bytes() != (root / "counts-after-read.png").read_bytes()
                assert "panicked" not in (root / "gmailtjik.log").read_text()
                sys.exit(0)

            if os.environ.get("GMAILTJIK_DEMO_SCENE") == "action-buttons":
                click(805, 106)
                capture("delete-button.png")
                click(80, 234)
                capture("trash-folder-after-button.png")
                assert (root / "inbox.png").read_bytes() != (root / "delete-button.png").read_bytes()
                assert "panicked" not in (root / "gmailtjik.log").read_text()
                sys.exit(0)

            if os.environ.get("GMAILTJIK_DEMO_SCENE") == "handling-groups":
                capture("handling-overview.png")
                click(550, 250)
                capture("handling-editor.png")
                click(830, 452)
                capture("handling-new-condition-ready.png")
                click(550, 455)
                subprocess.run(["wtype", "from:c@example.test"], check=True)
                capture("handling-added-or.png")
                click(830, 550)
                capture("handling-saved.png")
                click(550, 250)
                capture("handling-reopened.png")
                for y, expected in [(379, "from:(a@example.test)"), (417, "from:(b@example.test)"), (455, "from:c@example.test")]:
                    click(550, y)
                    subprocess.run(["wtype", "-M", "ctrl", "-k", "a", "-k", "c", "-m", "ctrl"], check=True)
                    assert subprocess.check_output(["wl-paste", "--no-newline"], text=True) == expected
                click(1374, 417)
                capture("handling-removed-or.png")
                click(830, 511)
                capture("handling-resaved.png")
                assert "panicked" not in (root / "gmailtjik.log").read_text()
                sys.exit(0)

            if os.environ.get("GMAILTJIK_DEMO_SCENE") == "filter-labels":
                click(1000, 105)
                capture("filter-labels-ready.png")
                click(400, 240)
                subprocess.run(["wtype", "co"], check=True)
                capture("filter-labels-matching.png")
                subprocess.run(["wtype", "-k", "Return"], check=True)
                capture("filter-labels-enter.png")

                def label_value():
                    subprocess.run(["wtype", "-M", "ctrl", "-k", "a", "-k", "c", "-m", "ctrl"], check=True)
                    value = subprocess.check_output(["wl-paste", "--no-newline"], text=True)
                    subprocess.run(["wtype", "-k", "End"], check=True)
                    return value

                assert label_value() == "Mail/Code, "
                subprocess.run(["wtype", "de"], check=True)
                capture("filter-labels-second-match.png")
                click(400, 273)
                assert label_value() == "Mail/Code, Mail/Design, "
                subprocess.run(["wtype", "rec"], check=True)
                capture("filter-labels-tab-match.png")
                subprocess.run(["wtype", "-k", "Tab"], check=True)
                capture("filter-labels-tab.png")
                assert label_value() == "Mail/Code, Mail/Design, Finances/Receipts, "
                subprocess.run(["wtype", "-k", "Down", "-k", "Escape"], check=True)
                capture("filter-labels-dismissed.png")
                assert label_value() == "Mail/Code, Mail/Design, Finances/Receipts, "
                assert "panicked" not in (root / "gmailtjik.log").read_text()
                sys.exit(0)

            if os.environ.get("GMAILTJIK_DEMO_SCENE") == "filters":
                click(1000, 105)
                capture("sender-filter.png")
                click(378, 273)
                click(272, 443)
                capture("filter-apply-checked.png")
                click(830, 473)
                capture("filter-saved.png")
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
                click(980, 200)
                capture("saved-reply-restored.png")
                click(80, 234)
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

            if os.environ.get("GMAILTJIK_DEMO_SCENE") == "reader":
                capture("reader-normalized.png")
                # Selection and clipboard stay inside this synthetic compositor.
                subprocess.run(["wl-copy", "--", "selection-not-copied"], check=True)
                operations = ["m 4271 3410", "m 4271 3410", "b 1"]
                operations += [f"m {x} 3410" for x in range(4290, 4620, 40)]
                operations += ["b 0"]
                for operation in operations:
                    click_pointer.stdin.write(operation + "\n")
                    click_pointer.stdin.flush()
                    assert click_pointer.stdout.readline().strip() == "ok"
                    time.sleep(.15)
                subprocess.run(["wtype", "-M", "ctrl", "-k", "c", "-m", "ctrl"], check=True)
                def copied_selection():
                    value = subprocess.check_output(["wl-paste", "--no-newline"], text=True, timeout=5)
                    return value if value != "selection-not-copied" else None
                copied = wait_for(copied_selection, timeout=5)
                capture("reader-selection.png")
                assert copied == "ello,", copied
                click(990, 690)
                capture("reader-quotes.png")
                click(990, 762)
                capture("reader-quotes-hidden.png")
                click(655, 307)
                time.sleep(1)
                capture("reader-original-dark.png")
                click(760, 307)
                time.sleep(1)
                capture("reader-original-colours.png")
                click(655, 307)
                capture("reader-restored.png")
                from PIL import Image
                def light_surface_fraction(name):
                    with Image.open(root / name) as shot:
                        pixels = list(shot.convert("RGB").crop((610, 330, 1370, 650)).get_flattened_data())
                    return sum(r > 220 and g > 220 and b > 210 for r, g, b in pixels) / len(pixels)
                assert light_surface_fraction("reader-normalized.png") < .05
                assert light_surface_fraction("reader-original-dark.png") < .05
                assert light_surface_fraction("reader-original-colours.png") > .5
                assert light_surface_fraction("reader-restored.png") < .05
                for before, after in [("reader-normalized.png", "reader-quotes.png"), ("reader-quotes.png", "reader-quotes-hidden.png"), ("reader-original-dark.png", "reader-original-colours.png")]:
                    assert (root / before).read_bytes() != (root / after).read_bytes(), (before, after)
                assert "panicked" not in (root / "gmailtjik.log").read_text()
                sys.exit(0)

            if os.environ.get("GMAILTJIK_DEMO_SCENE") == "label-search":
                click(95, 506)
                capture("label-selected.png")
                click(500, 48)
                subprocess.run(["wtype", "-M", "ctrl", "-k", "a", "-k", "c", "-m", "ctrl"], check=True)
                time.sleep(.4)
                scope = subprocess.check_output(["wl-paste", "--no-newline"], text=True)
                assert scope == "label:Mail/Design", scope
                subprocess.run(["wtype", "label:Mail/Design quiet", "-k", "Return"], check=True)
                capture("label-scoped-search.png")
                assert "panicked" not in (root / "gmailtjik.log").read_text()
                sys.exit(0)

            if os.environ.get("GMAILTJIK_DEMO_SCENE") == "contact-signature":
                capture("contact-signature-hidden.png")
                assert "panicked" not in (root / "gmailtjik.log").read_text()
                sys.exit(0)

            if os.environ.get("GMAILTJIK_DEMO_SCENE") == "mart-signoff":
                capture("mart-folded.png")
                click(1240, 253)
                capture("mart-revealed.png")
                assert (root / "mart-folded.png").read_bytes() != (root / "mart-revealed.png").read_bytes()
                assert "panicked" not in (root / "gmailtjik.log").read_text()
                sys.exit(0)

            if os.environ.get("GMAILTJIK_DEMO_SCENE") == "german-signoff":
                capture("german-folded.png")
                click(1215, 253)
                capture("german-signature.png")
                click(1240, 253)
                capture("german-history.png")
                assert (root / "german-folded.png").read_bytes() != (root / "german-signature.png").read_bytes()
                assert (root / "german-signature.png").read_bytes() != (root / "german-history.png").read_bytes()
                assert "panicked" not in (root / "gmailtjik.log").read_text()
                sys.exit(0)

            if os.environ.get("GMAILTJIK_DEMO_SCENE") == "signoff":
                capture("signoff-hidden.png")
                click(655, 720)
                capture("signoff-expanded.png")
                assert (root / "signoff-hidden.png").read_bytes() != (root / "signoff-expanded.png").read_bytes()
                assert "panicked" not in (root / "gmailtjik.log").read_text()
                sys.exit(0)

            if os.environ.get("GMAILTJIK_DEMO_SCENE") == "signature":
                capture("signature-hidden.png")
                from PIL import Image
                from collections import Counter
                def signature_pixels(name):
                    with Image.open(root / name) as shot:
                        return Counter(shot.convert("RGB").crop((590, 320, 1390, output_height - 145)).get_flattened_data())
                def signature_control(name, kind):
                    if kind == "signature":
                        click(1238, 253)
                        return
                    with Image.open(root / name) as shot:
                        shot = shot.convert("RGB")
                        border_rows = [y for y in range(550, output_height - 200)
                                       if shot.getpixel((1000, y)) == (57, 53, 70)
                                       and shot.getpixel((1300, y)) == (57, 53, 70)]
                    assert border_rows, name
                    click(675, border_rows[-1] - 25)
                def selected_body():
                    subprocess.run(["wl-copy", "--", "signature-selection-pending"], check=True)
                    for operation in [f"m 4306 {round(290 / output_height * 10000)}", "b 1", f"m 4792 {round(290 / output_height * 10000)}", "b 0"]:
                        click_pointer.stdin.write(operation + "\n")
                        click_pointer.stdin.flush()
                        assert click_pointer.stdout.readline().strip() == "ok"
                        time.sleep(.15)
                    subprocess.run(["wtype", "-M", "ctrl", "-k", "a", "-k", "c", "-m", "ctrl"], check=True)
                    def copied():
                        value = subprocess.check_output(["wl-paste", "--no-newline"], text=True, timeout=5)
                        return value if value != "signature-selection-pending" else None
                    return wait_for(copied, timeout=5)
                colours = signature_pixels("signature-hidden.png")
                assert colours[(240, 190, 50)] == 0
                assert colours[(255, 83, 166)] == 120 * 60
                assert colours[(70, 160, 220)] == 120 * 60
                # The phone number inside the request is message content;
                # only the trailing contact block should be collapsed.
                copied = selected_body()
                assert "+1 202 555 0142" in copied, copied
                assert "+1 202 555 0118" not in copied, copied
                assert "Mira Chen" not in copied, copied
                signature_control("signature-hidden.png", "signature")
                capture("signature-expanded.png")
                assert signature_pixels("signature-expanded.png")[(240, 190, 50)] == 96 * 64
                copied = selected_body()
                assert "+1 202 555 0142" in copied and "+1 202 555 0118" in copied, copied
                signature_control("signature-expanded.png", "quote")
                capture("signature-and-quotes.png")
                assert signature_pixels("signature-and-quotes.png")[(240, 190, 50)] == 96 * 64
                copied = selected_body()
                assert "Older synthetic delivery notes" in copied and "+1 202 555 0118" in copied, copied
                signature_control("signature-and-quotes.png", "signature")
                capture("signature-hidden-quotes-open.png")
                assert signature_pixels("signature-hidden-quotes-open.png")[(240, 190, 50)] == 0
                copied = selected_body()
                assert "Older synthetic delivery notes" in copied and "+1 202 555 0118" not in copied, copied
                signature_control("signature-hidden-quotes-open.png", "quote")
                capture("signature-recollapsed.png")
                click(1214, 253)
                time.sleep(1)
                capture("signature-original-hidden.png")
                assert signature_pixels("signature-original-hidden.png")[(240, 190, 50)] == 0
                signature_control("signature-original-hidden.png", "signature")
                time.sleep(1)
                capture("signature-original-expanded.png")
                assert signature_pixels("signature-original-expanded.png")[(240, 190, 50)] == 96 * 64
                click(1214, 253)
                capture("signature-readable-expanded.png")
                assert signature_pixels("signature-readable-expanded.png")[(240, 190, 50)] == 96 * 64
                signature_control("signature-readable-expanded.png", "signature")
                capture("signature-final-hidden.png")
                assert signature_pixels("signature-final-hidden.png")[(240, 190, 50)] == 0
                for name in ("signature-expanded.png", "signature-and-quotes.png", "signature-hidden-quotes-open.png", "signature-original-hidden.png", "signature-original-expanded.png", "signature-final-hidden.png"):
                    colours = signature_pixels(name)
                    assert colours[(255, 83, 166)] == 120 * 60, name
                    assert colours[(70, 160, 220)] == 120 * 60, name
                assert "panicked" not in (root / "gmailtjik.log").read_text()
                sys.exit(0)

            if os.environ.get("GMAILTJIK_DEMO_SCENE") == "inline":
                capture("inline-normalized.png")
                from PIL import Image, ImageChops
                palette = {
                    "pink": (255, 83, 166),
                    "blue": (70, 160, 220),
                    "orange": (255, 150, 40),
                    "purple": (170, 90, 220),
                    "green": (40, 210, 100),
                    "yellow": (240, 210, 40),
                }
                def image_patch(name, colour):
                    with Image.open(root / name) as shot:
                        body = shot.convert("RGB").crop((590, 320, 1390, output_height - 145))
                    difference = ImageChops.difference(body, Image.new("RGB", body.size, palette[colour]))
                    red, green, blue = difference.split()
                    mask = ImageChops.lighter(ImageChops.lighter(red, green), blue).point(lambda value: 255 if value == 0 else 0)
                    return mask.getbbox(), mask.histogram()[255]
                bounds = {}
                for colour in ("pink", "blue", "orange", "purple"):
                    box, count = image_patch("inline-normalized.png", colour)
                    assert box and count == 120 * 60, (colour, box, count)
                    assert (box[2] - box[0], box[3] - box[1]) == (120, 60), (colour, box)
                    bounds[colour] = box
                assert bounds["pink"][3] < bounds["orange"][1]
                assert image_patch("inline-normalized.png", "green")[1] == 0
                yellow, count = image_patch("inline-normalized.png", "yellow")
                assert yellow and count > 1000 and yellow[1] > bounds["orange"][3]
                click(990, 664)
                capture("inline-quotes.png")
                green, count = image_patch("inline-quotes.png", "green")
                assert green and count == 80 * 30, (green, count)
                assert green[1] > bounds["orange"][3]
                click(990, 776)
                capture("inline-quotes-hidden.png")
                assert image_patch("inline-quotes-hidden.png", "green")[1] == 0
                click(655, 307)
                time.sleep(1)
                capture("inline-original.png")
                for colour in ("pink", "blue", "orange", "purple"):
                    box, count = image_patch("inline-original.png", colour)
                    assert box and count >= 7000, (colour, box, count)
                click(655, 307)
                capture("inline-restored.png")
                for colour in ("pink", "blue", "orange", "purple"):
                    assert image_patch("inline-restored.png", colour)[1] == 120 * 60
                # Resize the actual reader while retaining native-size images
                # and the order of the paragraphs surrounding them.
                y = round(450 / output_height * 10000)
                operations = [f"m 3972 {y}", f"m 3972 {y}", "b 1"]
                operations += [f"m {x} {y}" for x in range(3972, 5801, 100)]
                operations += ["b 0"]
                for operation in operations:
                    click_pointer.stdin.write(operation + "\n")
                    click_pointer.stdin.flush()
                    assert click_pointer.stdout.readline().strip() == "ok"
                    time.sleep(.1)
                capture("inline-resized.png")
                resized = {}
                for colour in ("pink", "blue", "orange", "purple"):
                    box, count = image_patch("inline-resized.png", colour)
                    assert box and count == 120 * 60, (colour, box, count)
                    assert (box[2] - box[0], box[3] - box[1]) == (120, 60)
                    resized[colour] = box
                assert resized["pink"][3] < resized["orange"][1]
                assert image_patch("inline-resized.png", "green")[1] == 0
                assert "panicked" not in (root / "gmailtjik.log").read_text()
                sys.exit(0)

            if os.environ.get("GMAILTJIK_DEMO_SCENE") == "html":
                time.sleep(2)
                capture("html-before.png")
                # The synthetic thread has three older, multiline messages.
                # Their collapsed sender/preview rows must retain their height
                # above the latest message, even when its HTML is much taller
                # than the viewport.
                capture("html-dark.png")
                click(760, 541)
                time.sleep(1)
                capture("html-light.png")
                click(760, 541)
                time.sleep(1)
                pointer = subprocess.Popen([str(root / "pointer"), "GMAIL"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
                try:
                    operations = ["m 3972 4500", "m 3972 4500", "b 1"]
                    operations += [f"m {x} 4500" for x in range(3972, 5801, 100)]
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
                # Scroll the actual reader, including while the pointer is over
                # HTML. A horizontal table viewport must not trap vertical wheel
                # input or crop the end of the body.
                for operation in ["m 7600 7000", "m 7600 7000", "s 1600"]:
                    click_pointer.stdin.write(operation + "\n")
                    click_pointer.stdin.flush()
                    assert click_pointer.stdout.readline().strip() == "ok"
                    time.sleep(.2)
                capture("html-bottom.png")
                # This table has a minimum width, unlike the responsive image.
                # Drag the visible horizontal scrollbar to reveal its final
                # column, then return to the start before exercising quotes.
                operations = ["m 7083 7160", "m 7083 7160", "b 1"]
                operations += [f"m {x} 7160" for x in range(7283, 9500, 200)]
                operations += ["b 0"]
                for operation in operations:
                    click_pointer.stdin.write(operation + "\n")
                    click_pointer.stdin.flush()
                    assert click_pointer.stdout.readline().strip() == "ok"
                    time.sleep(.1)
                capture("html-right.png")
                operations = ["m 8542 7160", "m 8542 7160", "b 1"]
                operations += [f"m {x} 7160" for x in range(8342, 5800, -200)]
                operations += ["b 0"]
                for operation in operations:
                    click_pointer.stdin.write(operation + "\n")
                    click_pointer.stdin.flush()
                    assert click_pointer.stdout.readline().strip() == "ok"
                    time.sleep(.1)
                capture("html-left.png")
                from PIL import Image
                from collections import Counter
                # Complex mail defaults to a genuinely dark document, while
                # the explicit original-colours view restores light panels.
                def html_light_fraction(name):
                    with Image.open(root / name) as shot:
                        pixels = list(shot.convert("RGB").crop((620, 565, 1360, 840)).get_flattened_data())
                    return sum(r > 220 and g > 220 and b > 210 for r, g, b in pixels) / len(pixels)
                assert html_light_fraction("html-dark.png") < .05
                assert html_light_fraction("html-light.png") > .5
                with Image.open(root / "html-bottom.png") as shot:
                    colours = Counter(shot.convert("RGB").get_flattened_data())
                assert colours[(255, 83, 166)] > 25000
                assert colours[(70, 160, 220)] > 25000
                # Compare the table itself, excluding the pointer and scrollbar:
                # the final column must move into view and dragging back must
                # restore exactly the original content, not merely move a thumb.
                def table_pixels(name):
                    with Image.open(root / name) as capture_image:
                        return capture_image.crop((880, 300, 1340, 385)).tobytes()
                assert table_pixels("html-right.png") != table_pixels("html-bottom.png")
                assert table_pixels("html-left.png") == table_pixels("html-bottom.png")
                click(1115, 775)
                time.sleep(1)
                for operation in ["m 7600 7000", "m 7600 7000", "s 1600"]:
                    click_pointer.stdin.write(operation + "\n")
                    click_pointer.stdin.flush()
                    assert click_pointer.stdout.readline().strip() == "ok"
                    time.sleep(.2)
                capture("html-quotes.png")
                click(1115, 775)
                time.sleep(1)
                for operation in ["s -1600"]:
                    click_pointer.stdin.write(operation + "\n")
                    click_pointer.stdin.flush()
                    assert click_pointer.stdout.readline().strip() == "ok"
                    time.sleep(.2)
                capture("html-top.png")
                click(915, 541)
                capture("html-text.png")
                click(915, 541)
                time.sleep(1)
                click(1050, 254)
                capture("html-expanded-message.png")
                for before, after in [("html-dark.png", "html-light.png"), ("html-resized.png", "html-bottom.png"), ("html-bottom.png", "html-right.png"), ("html-right.png", "html-left.png"), ("html-bottom.png", "html-quotes.png"), ("html-top.png", "html-text.png"), ("html-top.png", "html-expanded-message.png")]:
                    assert (root / before).read_bytes() != (root / after).read_bytes(), (before, after)
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
