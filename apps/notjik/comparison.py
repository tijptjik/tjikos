"""Optional isolated notify-send comparison. Never replaces the desktop bus owner."""
import argparse
from concurrent.futures import ThreadPoolExecutor
import fcntl
import hashlib
from urllib.parse import unquote, urlparse
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import threading
import time

CASES = {
    "basic": ["Hello", "A normal desktop notification"],
    "low": ["-u", "low", "Low urgency", "Background work completed"],
    "normal": ["-u", "normal", "Normal urgency", "A message arrived"],
    "critical": ["-u", "critical", "-t", "0", "Critical urgency", "Requires attention; remains until dismissed"],
    "timeout": ["-t", "2500", "Short timeout", "Expires after 2.5 seconds"],
    "sticky": ["-t", "0", "Until dismissed", "Click outside the actions to dismiss"],
    "icon": ["-i", "network-wireless", "Connected", "Wi-Fi connection established"],
    "progress": ["-h", "int:value:67", "Uploading", "67 percent complete"],
    "multiline": ["Multiple lines", "First line\nSecond line\nThird line"],
    "long": ["A longer notification title that needs to wrap cleanly", "This is a long notification body. " * 18],
    "unicode": ["同步完成 · Sync complete", "繁體中文 · café · 日本語 · ✓"],
    "markup": ["Markup comparison", "<b>Bold</b>, <i>italic</i> and <a href='https://example.com'>a link</a>"],
    "transient": ["-e", "Transient notice", "Should not be retained in history"],
    "actions": ["-t", "30000", "-A", "default=Open", "-A", "ignore=Ignore", "Choose an action", "Click an action in each server; the selected names print in the terminal"],
    "resident": ["-t", "30000", "-h", "boolean:resident:true", "-A", "ack=Acknowledge", "Resident action", "Should remain visible after the action"],
}


def runtime_dir():
    if os.environ.get("NOTJIK_DEMO_CYCLE_DIR"):
        return Path(os.environ["NOTJIK_DEMO_CYCLE_DIR"])
    base = Path(os.environ.get("XDG_RUNTIME_DIR", str(Path.home() / ".cache/notjik/runtime")))
    return base / ("notjik-compare-" + str(os.getuid()))


def birth(pid):
    try:
        # The command name may itself contain spaces/parentheses.
        return Path(f"/proc/{pid}/stat").read_text().rsplit(")", 1)[1].split()[19]
    except (OSError, IndexError):
        return None


def state():
    try:
        value = json.loads((runtime_dir() / "session.json").read_text())
    except (OSError, ValueError):
        return None
    return value if birth(value["pid"]) == value["birth"] else None


def env_for(value, name):
    env = os.environ.copy()
    env["DBUS_SESSION_BUS_ADDRESS"] = value["buses"][name]
    env.pop("DBUS_STARTER_ADDRESS", None)
    env.pop("DBUS_STARTER_BUS_TYPE", None)
    if name == "notjik":
        env["NOTJIK_CONFIG"] = str(runtime_dir() / "config.json")
        env["NOTJIK_STATE_DIR"] = str(runtime_dir() / "history")
    return env


def ready(address):
    result = subprocess.run(["busctl", "--address=" + address, "call", "org.freedesktop.DBus", "/org/freedesktop/DBus",
        "org.freedesktop.DBus", "NameHasOwner", "s", "org.freedesktop.Notifications"], capture_output=True, text=True)
    return result.returncode == 0 and "true" in result.stdout


def presentation_signature(root):
    """Invalidate isolated UI copies when installed code or desktop config changes."""
    from notjik import load_config
    digest = hashlib.sha256(json.dumps(load_config(root), sort_keys=True).encode())
    for path in sorted([*root.glob("qml/**/*"), *root.glob("themes/**/*"), *root.glob("*.py")]):
        if path.is_file():
            digest.update(str(path.relative_to(root)).encode())
            digest.update(path.read_bytes())
    return digest.hexdigest()


def comparison_config(root, monitor):
    from notjik import load_config
    cfg = load_config(root)
    width = cfg["style"]["width"]
    cfg["placement"] = {**cfg["placement"], "monitors":"primary", "primaryMonitor":monitor,
        "offsetX":width + 40, "perMonitor":{}}
    cfg["grouping"]["enabled"] = False
    cfg["history"]["enabled"] = True
    cfg["history"]["logTransient"] = False
    # load_config resolves component URLs; serialized config expects filesystem paths.
    cfg["components"] = {name:unquote(urlparse(source).path) for name, source in cfg["components"].items()}
    return cfg


def serve(root, dunst):
    from notjik import load_config
    path = runtime_dir()
    children = []
    done = threading.Event()
    signal.signal(signal.SIGTERM, lambda *_: done.set())
    signal.signal(signal.SIGINT, lambda *_: done.set())
    session_file = path / "session.json"
    try:
        app = path / "app"
        for directory in ("qml", "themes"):
            shutil.copytree(root / directory, app / directory, dirs_exist_ok=True)
        for filename in ("notjik.py", "lock_watch.py", "telegram_bridge.py", "telegram_recording.py", "telegram_media_render.py"):
            shutil.copy2(root / filename, app / filename)
        # Resolve the same randomly assigned fallback artwork for both servers.
        shell = app / "qml/shell.qml"
        shell.write_text(shell.read_text().replace('case "status": break;', '''case "reference-artwork": {
            const notice = find(request.id);
            if (!notice) throw Error("Notification not found");
            const fallback = Qt.resolvedUrl(Logic.robotArtwork(notice)).toString();
            const value = notice.image || notice.icon || fallback;
            const artwork = value.startsWith("/") || value.startsWith("file:") || value.startsWith("image:") ? value
                : value.startsWith("network-wireless") ? Qt.resolvedUrl("icons/wifi-cover.png").toString()
                : Quickshell.iconPath(value, true) || fallback;
            return {ok:true, image:artwork};
        }
        case "status": break;''', 1))
        if os.environ.get("NOTJIK_DEMO_CYCLE_DIR"):
            from demo_cycle import prepare
            prepare(app)
        cfg = load_config(root)
        monitor = os.environ.get("NOTJIK_DEMO_CYCLE_MONITOR") or cfg["placement"].get("primaryMonitor", "")
        if not monitor:
            try:
                monitor = json.loads(subprocess.check_output(["hyprctl", "-j", "monitors"], text=True))[0]["name"]
            except (OSError, ValueError, subprocess.CalledProcessError, IndexError):
                monitor = ""
        cfg = comparison_config(root, monitor)
        if os.environ.get("NOTJIK_DEMO_CYCLE_DIR"):
            cfg["style"]["width"] = 420
            cfg["placement"].update(offsetX=230, offsetY=-140, margin=0)
            cfg["stack"]["heightPercent"] = 65
            cfg["history"]["enabled"] = True
            cfg["history"]["logTransient"] = True
            cfg["privacy"]["lockScreen"] = "hide"
        reference_origin = "bottom-center"
        reference_offset = "(0, 40)"
        if os.environ.get("NOTJIK_DEMO_CYCLE_DIR"):
            monitors = json.loads(subprocess.check_output(["hyprctl", "-j", "monitors"], text=True))
            selected = next(item for item in monitors if item["name"] == monitor)
            logical_width = selected["width"] / selected.get("scale", 1)
            reference_origin = "left-center"
            reference_offset = f"({max(0, round(logical_width / 2 - 440))}, 55)"
        width = cfg["style"]["width"]
        (path / "config.json").write_text(json.dumps(cfg, indent=2))
        # Dunst's Wayland output order can differ from hyprctl's monitor order.
        # Quote the connector name so both backends target the same physical output.
        monitor_selector = json.dumps(monitor) if monitor else "0"
        default_icon = json.dumps(str(app / "qml/icons/vintage-robot.png"))
        (path / "dunstrc").write_text(f'''[global]
monitor = {monitor_selector}
follow = none
origin = {reference_origin}
offset = {reference_offset}
width = {width}
height = (0, 350)
notification_limit = 5
font = tijpset 10
format = "<b>%s</b>\\n%b"
markup = full
padding = 16
horizontal_padding = 16
icon_position = left
min_icon_size = 48
max_icon_size = 48
default_icon = {default_icon}
frame_width = 1
frame_color = "#393546"
separator_height = 0
gap_size = 10
corner_radius = 14
transparency = 0
stack_duplicates = false
sort = false
progress_bar = true
mouse_left_click = close_current
mouse_middle_click = do_action
mouse_right_click = close_all
[urgency_low]
background = "#1f1d2e"
foreground = "#a5a0ba"
timeout = 5
[urgency_normal]
background = "#1f1d2e"
foreground = "#e5e5e5"
timeout = 8
[urgency_critical]
background = "#1f1d2e"
foreground = "#eb6f92"
frame_color = "#393546"
timeout = 0
''')
        value = {"pid":os.getpid(), "birth":birth(os.getpid()), "buses":{}, "app":str(app), "dunst":str(dunst), "monitor":monitor, "offsetX":cfg["placement"]["offsetX"], "width":width, "signature":presentation_signature(root)}
        for name in ("dunst", "notjik"):
            bus = subprocess.Popen(["dbus-daemon", "--nofork", "--config-file=" + str(root / "examples/comparison-bus.conf"), "--print-address=1"], stdout=subprocess.PIPE, text=True)
            children.append(bus)
            value["buses"][name] = bus.stdout.readline().strip()
            bus.stdout.close()
            if not value["buses"][name]:
                raise RuntimeError("Failed to start private " + name + " bus")
            command = [str(dunst), "-conf", str(path / "dunstrc")] if name == "dunst" else ["quickshell", "--path", str(app / "qml"), "--no-color"]
            children.append(subprocess.Popen(command, env=env_for(value, name)))
        deadline = time.monotonic() + 12
        while not all(ready(address) for address in value["buses"].values()):
            if done.wait(.1) or time.monotonic() > deadline or any(p.poll() is not None for p in children):
                raise RuntimeError("Comparison startup failed; inspect " + str(path / "session.log"))
        pending = path / "session.tmp"
        pending.write_text(json.dumps(value))
        pending.replace(session_file)
        while not done.wait(.5):
            if any(p.poll() is not None for p in children):
                break
    finally:
        for child in reversed(children):
            if child.poll() is None:
                child.terminate()
                try:
                    child.wait(timeout=3)
                except subprocess.TimeoutExpired:
                    child.kill()
                    child.wait()
        session_file.unlink(missing_ok=True)


def start(root, executable):
    existing = state()
    if existing:
        if existing.get("signature") == presentation_signature(root):
            return existing
        stop()
    binary = executable or shutil.which("dunst") or str(Path(os.environ.get("XDG_CACHE_HOME", str(Path.home()/".cache"))) / "notjik/comparison-reference/usr/bin/dunst")
    binary = Path(binary).expanduser().resolve()
    if not binary.is_file():
        raise ValueError("Dunst comparison binary missing; pass --dunst /path/to/dunst (production does not require it)")
    path = runtime_dir()
    (path / "session.json").unlink(missing_ok=True)
    with (path / "session.log").open("w") as log:
        proc = subprocess.Popen([sys.executable, "-B", str(Path(__file__).resolve()), "_serve", str(root), str(binary)], stdout=log, stderr=log, start_new_session=True)
    deadline = time.monotonic() + 15
    while time.monotonic() < deadline:
        value = state()
        if value:
            return value
        if proc.poll() is not None:
            raise ValueError("Comparison failed to start; see " + str(path / "session.log"))
        time.sleep(.1)
    proc.terminate()
    raise ValueError("Comparison startup timed out; see " + str(path / "session.log"))


def stop():
    value = state()
    if not value:
        return
    os.kill(value["pid"], signal.SIGTERM)
    for _ in range(100):
        if not state():
            return
        time.sleep(.1)
    raise ValueError("Comparison did not stop; inspect " + str(runtime_dir() / "session.log"))


def set_grouping(value, enabled):
    """Configure only the isolated notjik instance, then wait for its live reload."""
    from notjik import ipc
    path = runtime_dir() / "config.json"
    cfg = json.loads(path.read_text())
    if cfg["grouping"]["enabled"] != enabled:
        cfg["grouping"]["enabled"] = enabled
        path.write_text(json.dumps(cfg, indent=2))
    deadline = time.monotonic() + 5
    while time.monotonic() < deadline:
        if ipc(Path(value["app"]), {"op":"status"}).get("groupingEnabled") == enabled:
            return
        time.sleep(.05)
    raise ValueError("Comparison grouping configuration did not reload")


def demo_notify(value, notice, replaces=0):
    """Native D-Bus reference without notify-send's action wait or executable commands."""
    body = notice.get("body", "")
    data = notice.get("data", {})
    kind = notice.get("type", "standard")
    if kind == "status":
        extra = " · ".join(str(data[key]) for key in ("state", "detail") if data.get(key))
    elif kind == "timer":
        seconds = int(data.get("seconds", 0))
        extra = f"{seconds // 60:02d}:{seconds % 60:02d} · " + data.get("label" if seconds else "doneLabel", "Remaining" if seconds else "Ready")
    elif kind == "progress":
        extra = data.get("label", "")
    elif kind == "telegram":
        extra = {"text":"Telegram message", "photo":"Photo preview", "voice":"Voice message · 0:08", "sticker":"Sticker", "animation":"Animation", "emoji":"Animated emoji"}.get(data.get("kind"), "Telegram message")
    else:
        extra = ""
    if extra:
        body += ("\n" if body else "") + extra
    actions = []
    for index, action in enumerate(notice.get("actions", [])):
        actions.extend(["default" if index == 0 else "action-" + str(index), action["label"]])
    hints = [["urgency", "y", str({"low":0, "normal":1, "critical":2}[notice.get("urgency", "normal")])],
             ["transient", "b", str(notice.get("transient", True)).lower()],
             ["resident", "b", str(notice.get("resident", False)).lower()]]
    if kind == "progress":
        hints.append(["value", "i", str(round(data["value"]))])
    if notice.get("image"):
        hints.append(["image-path", "s", notice["image"]])
    command = ["busctl", "--address=" + value["buses"]["dunst"], "call", "org.freedesktop.Notifications",
               "/org/freedesktop/Notifications", "org.freedesktop.Notifications", "Notify", "susssasa{sv}i",
               notice["app"], str(replaces), notice.get("icon", ""), notice["title"], body,
               str(len(actions)), *actions, str(len(hints)), *(item for hint in hints for item in hint), str(round(notice.get("timeout", 0)))]
    result = subprocess.run(command, capture_output=True, text=True, timeout=10)
    if result.returncode:
        raise ValueError("Dunst demo: " + result.stderr.strip())
    return int(result.stdout.split()[1])


def demo_close(value, ident):
    result = subprocess.run(["busctl", "--address=" + value["buses"]["dunst"], "call", "org.freedesktop.Notifications",
        "/org/freedesktop/Notifications", "org.freedesktop.Notifications", "CloseNotification", "u", str(ident)],
        capture_output=True, text=True, timeout=10)
    if result.returncode:
        raise ValueError("Dunst demo: " + result.stderr.strip())


def send(value, arguments, replacements=None):
    if "app" in value:
        set_grouping(value, False)
    def one(name):
        args = ["notify-send", "--print-id", *arguments]
        if replacements:
            args[1:1] = ["--replace-id", str(replacements[name])]
        result = subprocess.run(args, env=env_for(value, name), text=True, capture_output=True, timeout=120)
        if result.returncode:
            message = result.stderr.strip()
            if "Invalid number of options" in message:
                message += " Send one notification per command; put multiple notjikctl compare send commands on separate lines."
            raise ValueError(name + ": " + message)
        lines = result.stdout.strip().splitlines()
        return name, {"id":int(lines[0]), "action":lines[1:]}
    with ThreadPoolExecutor(max_workers=2) as pool:
        return dict(pool.map(one, ("dunst", "notjik")))


def clear(value):
    subprocess.run(["busctl", "--address=" + value["buses"]["dunst"], "call", "org.freedesktop.Notifications", "/org/freedesktop/Notifications", "org.dunstproject.cmd0", "NotificationCloseAll"], check=True, capture_output=True)
    subprocess.run(["quickshell", "ipc", "--path", str(Path(value["app"])/"qml"), "call", "notjik", "request", '{"op":"dismiss","id":"all"}'], env=env_for(value, "notjik"), check=True, capture_output=True)


def run_case(value, name):
    common = ["-a", "Parity test", "-t", "12000"]
    if name == "replace":
        first = send(value, common + ["-h", "int:value:10", "Upload", "Initial 10 percent"])
        time.sleep(1)
        return send(value, common + ["-h", "int:value:75", "Upload", "Same ID, updated to 75 percent"], {key:item["id"] for key,item in first.items()})
    if name == "image":
        art = runtime_dir() / "artwork.png"
        # Small synthetic bitmap; notify-send encodes image-data from this file.
        import struct
        import zlib
        def chunk(kind, data):
            return struct.pack("!I",len(data)) + kind + data + struct.pack("!I",zlib.crc32(kind+data))
        pixels = b"".join(b"\0" + b"".join(bytes((255,83,166,255) if abs(x-y)<8 else (31,29,46,255)) for x in range(64)) for y in range(64))
        art.write_bytes(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR",struct.pack("!IIBBBBB",64,64,8,6,0,0,0)) + chunk(b"IDAT",zlib.compress(pixels)) + chunk(b"IEND",b""))
        return send(value, common + ["-i", str(art), "Image artwork", "Synthetic bitmap image-data"])
    if name == "stack":
        return [send(value, common + [f"Stack card {i}", "All visible cards should stay full-size"]) for i in range(1,5)]
    return send(value, common + CASES[name])


def main(root, argv):
    os.umask(0o077)
    path = runtime_dir()
    path.mkdir(mode=0o700, parents=True, exist_ok=True)
    parser = argparse.ArgumentParser(prog="notjikctl compare", description=__doc__)
    sub = parser.add_subparsers(dest="op", required=True)
    begin = sub.add_parser("start")
    begin.add_argument("--dunst", help="optional reference binary; never installed as a system service")
    for op in ("stop", "status", "clear", "cases"):
        sub.add_parser(op)
    forwarded = sub.add_parser("send", help="send identical notify-send arguments to both test buses")
    forwarded.add_argument("arguments", nargs=argparse.REMAINDER)
    case = sub.add_parser("case")
    case.add_argument("name", choices=[*CASES,"replace","image","stack"])
    args = parser.parse_args(argv)
    try:
        if args.op in ("start", "stop"):
            with (path / "control.lock").open("w") as lock:
                fcntl.flock(lock, fcntl.LOCK_EX)
                if args.op == "stop":
                    stop()
                    print("Comparison stopped; desktop notification service unchanged")
                    return 0
                value = start(root, args.dunst)
        else:
            value = state()
        if args.op == "cases":
            print("\n".join([*CASES, "replace", "image", "stack"]))
        elif args.op in ("status", "start"):
            print(json.dumps({"running":bool(value), "monitor":value["monitor"] if value else None,
                "notjikOffsetX":value.get("offsetX") if value else None, "width":value.get("width") if value else None, "directory":str(path)}, indent=2))
        elif not value:
            raise ValueError("Start the comparison first: notjikctl compare start")
        elif args.op == "clear":
            clear(value)
        elif args.op == "case":
            print(json.dumps(run_case(value, args.name), indent=2))
        elif args.op == "send":
            arguments = args.arguments[1:] if args.arguments[:1] == ["--"] else args.arguments
            if not arguments:
                raise ValueError("Pass notify-send arguments after --")
            print(json.dumps(send(value, arguments), indent=2))
        return 0
    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
        print("notjik comparison: " + str(error), file=sys.stderr)
        return 1


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "_serve":
        serve(Path(sys.argv[2]), Path(sys.argv[3]))
