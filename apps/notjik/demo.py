"""Synthetic visual fixtures for interactive notification review."""
import argparse
import json
from pathlib import Path
import sys
import subprocess
import time
import tempfile

APP = "notjik demo"
TYPES = {
    "standard": "Title and body with the default robot",
    "title-only": "Title without a description",
    "icon": "Sender-provided symbolic Wi-Fi icon",
    "missing-icon": "Unknown icon name falls back to the robot",
    "image": "Sender-provided artwork overrides its icon",
    "progress": "Progress bar at 67 percent",
    "progress-empty": "Progress bar at zero",
    "progress-complete": "Progress bar at 100 percent",
    "status": "Informational status and detail",
    "success": "Successful status",
    "warning": "Warning status",
    "error": "Critical status with an action",
    "timer": "Running countdown",
    "timer-done": "Completed countdown",
    "actions": "One centered action",
    "actions-multiple": "Two centered actions",
    "actions-wrapped": "Overflow actions slide between pages using chevrons",
    "resident": "Action leaves the card visible",
    "urgency": "Low, normal and critical side by side in the stack",
    "multiline": "Explicit line breaks",
    "long": "Long title and body with wrapping and truncation",
    "unicode": "Multilingual text and symbols",
    "markup": "Literal markup: notjik currently uses plain text",
    "timeout": "Expires after 2.5 seconds with the outro animation",
    "update": "Progress updates in place with a stable sender ID",
    "grouped": "Three notices sharing one group",
    "stack": "Four separate cards, newest at the bottom",
    "telegram-text": "Telegram text with reply controls (simulated)",
    "telegram-photo": "Telegram photo with a caption (bundled artwork)",
    "telegram-voice": "Telegram voice with waveform, playback, seeking and speed",
    "telegram-sticker": "Static WebP sticker",
    "telegram-animated-sticker": "Animated TGS sticker rendered locally",
    "telegram-gif": "Silent looping Telegram animation",
    "telegram-animated-emoji": "Large animated emoji",
    "telegram-custom-emoji": "Inline animated custom emoji in text",
    "telegram-grouped": "Text, photo and voice from the same Telegram chat",
}
OVERVIEW = ("icon", "progress", "error", "timer")
GROUPED_TYPES = ("grouped", "telegram-grouped")


def actions(*labels):
    return [{"label": label, "command": ["true"]} for label in labels]


def fixtures(root, name):
    """Fresh dictionaries on every call; fixtures contain no real user data."""
    if name == "telegram-grouped":
        messages = []
        for index, kind in enumerate(("text", "photo", "voice"), start=1):
            notice = fixtures(root, "telegram-" + kind)[0]
            notice["data"]["messageId"] = str(index)
            messages.append(notice)
        return messages
    if name.startswith("telegram-"):
        kind = name.removeprefix("telegram-")
        if kind not in ("text", "photo", "voice", "sticker", "animated-sticker", "gif", "animated-emoji", "custom-emoji"):
            raise ValueError("Unknown demo type: " + name)
        data = {"kind": kind, "demo": True, "chatId": "1", "messageId": "1", "source": "", "status": ""}
        body = "Meet me on deck 7. The stars are doing it again."
        if kind == "photo":
            data["source"] = (root / "examples/demo-artwork.png").as_uri()
            body = "First light on Kepler-186f. No filter. Two suns."
        elif kind == "voice":
            data.update(source=(root / "examples/demo-voice.wav").as_uri(), duration=8,
                        waveform=[7, 19, 29, 31, 30, 23, 12, 7, 16, 26, 31, 31, 27, 16, 7, 12, 23, 30, 31, 29, 20, 10, 9, 19, 28, 31, 31, 24, 13, 6, 15, 25, 31, 31, 27, 17, 7, 11, 22, 30, 31, 29, 21, 10, 8, 18, 28, 31, 31, 24, 14, 6, 14, 25, 31, 31, 28, 18, 8, 11, 21, 30, 31, 30, 21, 11, 8, 18, 28, 31, 31, 25, 14, 6, 14, 24, 31, 31, 28, 18, 8, 10, 21, 29, 31, 30, 22, 11, 7, 17, 27, 31, 31, 25, 15, 6, 13, 24, 30, 24])
            body = ""
        if kind in ("sticker", "animated-sticker", "gif", "animated-emoji"):
            filename, format, media_kind = {
                "sticker":("demo-sticker.png","png","sticker"),
                "animated-sticker":("demo-animated-sticker.gif","gif","sticker"),
                "gif":("demo-robot-dog.gif","gif","animation"),
                "animated-emoji":("demo-animated-sticker.gif","gif","emoji"),
            }[kind]
            data.update(kind=media_kind,source=(root/"examples"/filename).as_uri(),format=format,width=128,height=128)
            body = ""
        elif kind == "custom-emoji":
            data.update(kind="text",parts=[{"kind":"text","text":"Hello "},
                {"kind":"emoji","text":"🙂","source":(root/"examples/demo-animation.gif").as_uri(),"format":"gif"},
                {"kind":"text","text":" from Telegram"}])
            body = "Hello 🙂 from Telegram"
        return [{"title": "Maya", "body": body, "type": "telegram", "icon": "org.telegram.desktop",
                 "image": (root / "qml/icons/robots/robot-84-dream-translucent.png").as_uri(), "data": data}]
    if name == "standard":
        return [{"title": "Flight recorder secured", "body": "Black box synced. Even the bit with the asteroid."}]
    if name == "title-only":
        return [{"title": "All decks reporting green"}]
    if name in ("icon", "missing-icon"):
        return [{"title": "Relay link established", "body": "Deep-space relay · signal 79% · Earth, eventually",
                 "icon": "network-wireless-symbolic" if name == "icon" else "notjik-demo-missing-icon"}]
    if name == "image":
        return [{"title": "Oceans of Kepler-186f", "body": "Surface survey · first light",
                 "image": (root / "examples/demo-artwork.png").as_uri(), "icon": "network-wireless-symbolic"}]
    if name.startswith("progress") or name == "update":
        value = {"progress-empty": 0, "progress-complete": 100, "update": 10}.get(name, 67)
        return [{"title": "Star chart transfer", "body": "Unknown sector · 1.6 / 2.4 GiB mapped", "type": "progress",
                 "data": {"value": value, "label": "Receiving navigation data"}}]
    if name in ("status", "success", "warning", "error"):
        state, tone, detail = {
            "status": ("Connected", "normal", "Deep-space relay online"),
            "success": ("Complete", "success", "Flight recorder secured"),
            "warning": ("Needs attention", "warning", "Auxiliary power required"),
            "error": ("Failed", "critical", "Containment field unstable"),
        }[name]
        notice = {"title": {"status":"Relay link established", "success":"Flight recorder secured", "warning":"Reactor running hot", "error":"Reactor fault"}[name], "body": "Engineering · deck 4", "type": "status",
                  "data": {"state": state, "tone": tone, "detail": detail}}
        if name == "error":
            notice.update(urgency="critical", actions=actions("Open diagnostics ↗"))
        return [notice]
    if name in ("timer", "timer-done"):
        return [{"title": "Jump window", "type": "timer", "data": {"seconds": 90 if name == "timer" else 0,
                 "label": "Spooling the jump drive", "doneLabel": "Jump coordinates locked"}}]
    if name in ("actions", "actions-multiple", "actions-wrapped", "resident"):
        labels = {
            "actions": ("Open diagnostics ↗",),
            "actions-multiple": ("Scan cargo", "Jettison"),
            "actions-wrapped": ("Open this notification in the application", "View backup details", "Retry", "Dismiss"),
            "resident": ("Acknowledge",),
        }[name]
        return [{"title": "Unidentified cargo aboard", "body": "It is warm. It is humming. It has no label.",
                 "actions": actions(*labels), "resident": name == "resident"}]
    if name == "urgency":
        return [{"title": {"low":"Hydroponics report", "normal":"Incoming docking request", "critical":"Hull breach · deck 4"}[level], "body": {"low":"The tomatoes survived hyperspace.", "normal":"The courier insists the package is not alive.", "critical":"Pressure falling. Seal the bulkhead."}[level], "urgency": level}
                for level in ("low", "normal", "critical")]
    if name == "multiline":
        return [{"title": "Shuttle ready", "body": "✓ Fuel loaded\n✓ Airlock sealed\nTry to bring the shuttle back this time."}]
    if name == "long":
        return [{"title": "Survey notes from the far side of the nebula",
                 "body": "The signal repeats every eleven seconds. There are no registered stations in this sector. " * 12}]
    if name == "unicode":
        return [{"title": "同步完成 · Sync complete", "body": "繁體中文 · 日本語 · café · ✓ · →"}]
    if name == "markup":
        return [{"title": "A note from the bridge", "body": '<b>Bring your pressure suit</b> · Meet at airlock 3.'}]
    if name == "timeout":
        return [{"title": "Saved", "body": "Coordinates stored in the flight computer.", "timeout": 2500}]
    if name in ("grouped", "stack"):
        return [{"title": ("Bridge channel" if name == "grouped" else "Surface scan " + str(i + 1)),
                 "body": ("Maya: That moon was not on the chart.", "IO: Neither was the second one.", "Maya: Tell me those are moons.")[i] if name == "grouped" else "New surface scans received from the lander."}
                for i in range(3 if name == "grouped" else 4)]
    raise ValueError("Unknown demo type: " + name)


def clear(call):
    """Never dismiss real notifications or change service settings."""
    count = 0
    for notice in call({"op": "list"})["notices"]:
        if notice.get("app") == APP:
            try:
                call({"op": "dismiss", "id": notice["id"]})
                count += 1
            except ValueError as error:
                if "Notification not found" not in str(error):
                    raise
    return count


def show(root, name, call, timeout=None, log=False):
    ids = []
    for index, fixture in enumerate(fixtures(root, name)):
        notice = {"app": APP, "id": f"demo-{name}-{index}", "group": name if name in GROUPED_TYPES else f"{name}-{index}",
                  "timeout": 0, "transient": not log, **fixture}
        if timeout is not None:
            notice["timeout"] = timeout
        ident = call({"op": "send", "notice": notice})["id"]
        ids.append(ident)
        if name == "update":
            for value in (25, 50, 75, 100):
                time.sleep(.35)
                notice = {**notice, "data": {"value": value, "label": "Saving your encrypted archive"}}
                assert call({"op": "send", "notice": notice})["id"] == ident
    return ids


def robot_images(root):
    icons = root / "qml/icons"
    return sorted(icons.glob("robot-[0-9][0-9]-*.png")) + sorted((icons / "robots").glob("robot-[0-9][0-9]-*.png"))


def show_robots(root, call, interval, timeout=None, log=False):
    images = robot_images(root)
    if not images:
        raise ValueError("No robot artwork is installed")
    for index, image in enumerate(images):
        _, number, name = image.stem.split("-", 2)
        name = name.replace("-", " ").title()
        label = f"{int(number)}. {name}"
        notice = {"app": APP, "id": "demo-robots", "group": "robots",
                  "title": label,
                  "body": "", "image": image.as_uri(),
                  "urgency": "critical" if "-alert-" in image.stem else "low" if "-dream-" in image.stem else "normal",
                  "data": {"demoRobot": True}, "timeout": 0 if timeout is None else timeout,
                  "transient": not log}
        call({"op": "send", "notice": notice})
        print(label, flush=True)
        if index < len(images) - 1:
            time.sleep(interval / 1000)


def reference_note(name):
    if name == "telegram-grouped":
        return "Dunst: separate text baselines; notjik groups three Telegram messages"
    if name.startswith("telegram-"):
        return "Dunst: text baseline; media player and Telegram controls are notjik-only"
    if name in ("status", "success", "warning", "error"):
        return "Dunst: status as text; inline state layout is notjik-only"
    if name in ("timer", "timer-done"):
        return "Dunst: static time as text; live countdown is notjik-only"
    if name == "grouped":
        return "Dunst: separate notices; grouped history/count is notjik-only"
    if name.startswith("actions") or name == "resident":
        return "Dunst: native actions via middle click/context menu; footer is notjik-only"
    return "native reference"


class PairedDemo:
    def __init__(self, session):
        import comparison
        from notjik import ipc
        self.session = session
        self.path = comparison.runtime_dir() / "demo-ids.json"
        try:
            saved = json.loads(self.path.read_text())
        except (OSError, ValueError):
            saved = {}
        self.ids = saved.get("ids", {}) if saved.get("birth") == session["birth"] else {}
        deadline = time.monotonic() + 5
        while True:
            try:
                state = ipc(Path(session["app"]), {"op":"status"})
                if state.get("lockState") != "unknown":
                    break
            except ValueError:
                if time.monotonic() >= deadline:
                    raise
            if time.monotonic() >= deadline:
                raise ValueError("Comparison notjik did not become ready")
            time.sleep(.05)

    def save(self):
        pending = self.path.with_suffix(".tmp")
        pending.write_text(json.dumps({"birth":self.session["birth"], "ids":self.ids}))
        pending.replace(self.path)

    def __call__(self, request):
        import comparison
        from notjik import ipc
        result = ipc(Path(self.session["app"]), request)
        if request["op"] == "send":
            ident = result["id"]
            reference = dict(request["notice"])
            reference["image"] = ipc(Path(self.session["app"]),
                {"op":"reference-artwork", "id":ident})["image"]
            self.ids[ident] = comparison.demo_notify(self.session, reference, self.ids.get(ident, 0))
            self.save()
        elif request["op"] == "dismiss" and request["id"] in self.ids:
            comparison.demo_close(self.session, self.ids.pop(request["id"]))
            self.save()
        return result

    def clear(self):
        import comparison
        count = clear(self)
        # Either server may have expired or dismissed its half independently.
        for ident in list(self.ids):
            comparison.demo_close(self.session, self.ids.pop(ident))
        self.save()
        return count


def comparison_session(root, start=True):
    import comparison
    import fcntl
    path = comparison.runtime_dir()
    path.mkdir(mode=0o700, parents=True, exist_ok=True)
    with (path / "control.lock").open("w") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        existing = comparison.state()
        if not start:
            return existing
        try:
            return comparison.start(root, None)
        except ValueError as error:
            if "Dunst comparison binary missing" not in str(error):
                raise
            print("Dunst reference unavailable; showing notjik alone. Install the reference or use compare start --dunst PATH.", file=sys.stderr)
            return None


def cursor_path(paired):
    if paired:
        import comparison
        return comparison.runtime_dir() / "demo-cursor.json"
    from notjik import state_path
    return state_path() / "demo-cursor.json"


def next_type(path, call):
    previous = None
    try:
        saved = json.loads(path.read_text())
        previous = saved.get("last") if isinstance(saved, dict) else None
        if not isinstance(previous, str):
            previous = None
    except (OSError, ValueError):
        pass
    if previous not in TYPES:
        # Also recognize a demo displayed before cursor tracking was installed.
        for notice in call({"op":"list"})["notices"]:
            if notice.get("app") != APP:
                continue
            name = notice.get("id", "").rsplit("/demo-", 1)[-1].rsplit("-", 1)[0]
            if name in TYPES:
                previous = name
    names = list(TYPES)
    return names[(names.index(previous) + 1) % len(names)] if previous in TYPES else names[0]


def save_cursor(path, name):
    path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(mode="w", dir=path.parent, prefix=".demo-cursor-", delete=False) as pending:
        pending.write(json.dumps({"last":name}))
    temporary = Path(pending.name)
    try:
        temporary.replace(path)
    finally:
        temporary.unlink(missing_ok=True)


def main(root, argv):
    if argv and argv[0] in ("cycle", "record"):
        from demo_cycle import main as cycle_main
        return cycle_main(root, argv)
    from notjik import duration, ipc
    parser = argparse.ArgumentParser(prog="notjikctl demo", description=__doc__)
    parser.add_argument("types", nargs="*", metavar="TYPE", help="list, overview (default), all, robots, next, clear, or one or more named types")
    parser.add_argument("--solo", action="store_true", help="preview on the normal notjik service instead of beside Dunst")
    parser.add_argument("--append", action="store_true", help="keep existing demo cards; named IDs still update in place")
    parser.add_argument("--interval", type=duration, default=None, help="time between previews (default: robots 1s; all 4s)")
    parser.add_argument("--timeout", type=duration, help="override card lifetime, e.g. 12s or never")
    parser.add_argument("--log", action="store_true", help="save synthetic demos to history (also lets grouped cards open their history)")
    args = parser.parse_args(argv)
    names = args.types or ["overview"]
    controls = {"list", "types", "overview", "all", "robots", "next", "clear"}
    if any(name not in TYPES and name not in controls for name in names):
        parser.error("Unknown demo type. Run 'notjikctl demo list' for available types.")
    if any(name in controls for name in names) and len(names) != 1:
        parser.error("Use list, overview, all, robots, next or clear on its own.")
    if args.interval is None:
        args.interval = 1000 if names[0] == "robots" else 4000
    if args.interval <= 0:
        parser.error("--interval must be a positive duration")
    if names[0] in ("all", "robots") and args.append:
        parser.error("all and robots cycle one preview at a time; --append is only for named types")
    if names[0] in ("list", "types"):
        print(f"{'robots':<20} Quickly preview every installed robot image")
        for name, description in TYPES.items():
            print(f"{name:<20} {description}")
        return 0
    call = lambda request: ipc(root, request)
    try:
        session = None if args.solo else comparison_session(root, start=names[0] != "clear")
        paired = PairedDemo(session) if session else None
        if paired:
            call = paired
            print(f"Dunst: left · notjik: right (+{session['offsetX']}px); isolated comparison sessions")
        cleanup = paired.clear if paired else lambda: clear(call)
        if names[0] == "clear":
            print(f"Cleared {cleanup()} demo cards")
            return 0
        status = call({"op": "status"})
        if status["paused"]:
            raise ValueError("Notifications are paused; run 'notjikctl resume' before previewing demos.")
        if status["lockState"] != "unlocked":
            raise ValueError("Unlock the desktop before previewing demos.")
        cursor = cursor_path(bool(paired))
        if names[0] == "next":
            names = [next_type(cursor, call)]
        if status["drawerOpen"]:
            call({"op": "history", "mode": "hide"})
        if not args.append:
            cleanup()
        if names[0] == "robots":
            if paired:
                import comparison
                comparison.set_grouping(session, False)
            show_robots(root, call, args.interval, args.timeout, args.log)
            print("Robot preview complete. Clear with: notjikctl demo clear" + (" --solo" if args.solo else ""))
            return 0
        cycle = names[0] == "all"
        selected = list(TYPES) if cycle else list(OVERVIEW) if names[0] == "overview" else names
        for index, name in enumerate(selected):
            if cycle and index:
                cleanup()
            if paired:
                import comparison
                comparison.set_grouping(session, name in GROUPED_TYPES or (not cycle and any(kind in GROUPED_TYPES for kind in selected)) or (args.append and status.get("groupingEnabled", False)))
            ids = show(root, name, call, args.timeout, args.log)
            save_cursor(cursor, name)
            print(f"{index + 1}/{len(selected)} {name}: " + ", ".join(ids) + (" · " + reference_note(name) if paired else ""), flush=True)
            if cycle and index < len(selected) - 1:
                time.sleep(args.interval / 1000)
        print("Demo cards remain until dismissed (except timed cards). Clear with: notjikctl demo clear" + (" --solo" if args.solo else ""))
        return 0
    except KeyboardInterrupt:
        print("\nPreview stopped; current demo cards remain. Clear with: notjikctl demo clear" + (" --solo" if args.solo else ""))
        return 130
    except (OSError, ValueError, KeyError, RuntimeError, subprocess.SubprocessError) as error:
        print("notjik demo: " + str(error), file=sys.stderr)
        return 1
