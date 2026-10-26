"""CLI, validated configuration and bounded local history for the QML server."""
import argparse
import copy
import datetime as dt
import json
import math
import os
from pathlib import Path
import re
import select
import sqlite3
import subprocess
import sys
import tempfile
import time
from lock_watch import LockObserver


DEFAULTS = {
    "theme": "monochrome", "style": {},
    "placement": {"monitors": "primary", "primaryMonitor": "", "edge": "b", "position": "c",
                  "offsetX": 0, "offsetY": -24, "margin": 16, "perMonitor": {}},
    "timeouts": {"low": 5, "normal": 8, "critical": 0},
    "pauseOnHover": True, "maxVisible": 4, "maxQueued": 100,
    "history": {"enabled": True, "maxEvents": 10000, "maxAgeDays": 0, "logTransient": False, "drawerLimit": 100},
    "grouping": {"enabled": True, "windowSeconds": 120},
    "privacy": {"lockScreen": "hide"},
    "stack": {"animationMs": 220, "heightPercent": 75},
    "actions": {"blacklist": []},
    "components": {}, "handlers": [],
}


def config_path():
    return Path(os.environ.get("NOTJIK_CONFIG", str(Path(os.environ.get(
        "XDG_CONFIG_HOME", str(Path.home() / ".config"))) / "notjik/config.json")))


def state_path():
    return Path(os.environ.get("NOTJIK_STATE_DIR", str(Path(os.environ.get(
        "XDG_STATE_HOME", str(Path.home() / ".local/state"))) / "notjik")))


def merge(base, patch):
    result = copy.deepcopy(base)
    for key, value in patch.items():
        result[key] = merge(result[key], value) if isinstance(value, dict) and isinstance(result.get(key), dict) else value
    return result


def number(value, label, minimum=0, maximum=1000000):
    if isinstance(value, bool) or not isinstance(value, (float, int)) or not math.isfinite(value) or not minimum <= value <= maximum:
        raise ValueError(f"{label} must be between {minimum} and {maximum}")


def validate_placement(p):
    if p.get("edge", "b") not in ("t", "r", "b", "l"):
        raise ValueError("edge must be t, r, b or l")
    if p.get("position", "c") not in ("l", "r", "c"):
        raise ValueError("position must be l, r or c")
    for key in ("offsetX", "offsetY"):
        value = p.get(key, 0)
        if isinstance(value, str) and re.fullmatch(r"-?\d+(?:\.\d+)?%", value):
            number(float(value[:-1]), key, -100, 100)
        else:
            number(value, key, -100000, 100000)
    number(p.get("margin", 16), "margin", 0, 1000)
    if "enabled" in p and not isinstance(p["enabled"], bool):
        raise ValueError("enabled must be boolean")


def load_config(root):
    path = config_path()
    patch = json.loads(path.read_text()) if path.exists() else {}
    if not isinstance(patch, dict):
        raise ValueError("config must be an object")
    cfg = merge(DEFAULTS, patch)
    for key in ("style", "placement", "timeouts", "history", "components", "grouping", "privacy", "stack", "actions"):
        if not isinstance(cfg[key], dict):
            raise ValueError(f"{key} must be an object")
    if not isinstance(cfg["placement"]["perMonitor"], dict):
        raise ValueError("placement.perMonitor must be an object")
    validate_placement(cfg["placement"])
    monitors = cfg["placement"]["monitors"]
    if not isinstance(cfg["placement"]["primaryMonitor"], str):
        raise ValueError("placement.primaryMonitor must be a connector name")
    if monitors not in ("all", "primary"):
        if not isinstance(monitors, list) or not monitors:
            raise ValueError("monitors must be 'primary', 'all', or a nonempty array of connector names / 1-based indexes")
        for monitor in monitors:
            if isinstance(monitor, bool) or not isinstance(monitor, (str, int)) or (isinstance(monitor, int) and monitor < 1):
                raise ValueError("invalid monitor selector")
    for value in cfg["placement"]["perMonitor"].values():
        if not isinstance(value, dict):
            raise ValueError("each per-monitor placement must be an object")
        validate_placement(value)
    for key in ("low", "normal", "critical"):
        number(cfg["timeouts"][key], f"timeouts.{key}", 0, 604800)
    for key in ("maxVisible", "maxQueued"):
        number(cfg[key], key, 1, 1000)
        if not isinstance(cfg[key], int):
            raise ValueError(f"{key} must be an integer")
    if cfg["maxQueued"] < cfg["maxVisible"]:
        raise ValueError("maxQueued must be at least maxVisible")
    number(cfg["history"]["maxEvents"], "history.maxEvents", 0, 1000000)
    if not isinstance(cfg["history"]["maxEvents"], int):
        raise ValueError("history.maxEvents must be an integer")
    number(cfg["history"]["maxAgeDays"], "history.maxAgeDays", 0, 3650)
    if not cfg["history"]["maxEvents"] and not cfg["history"]["maxAgeDays"]:
        raise ValueError("history requires maxEvents or maxAgeDays to bound retention")
    for value in (cfg["pauseOnHover"], cfg["history"]["enabled"], cfg["history"]["logTransient"]):
        if not isinstance(value, bool):
            raise ValueError("pauseOnHover and history flags must be boolean")
    if cfg["privacy"]["lockScreen"] not in ("hide", "redact", "show"):
        raise ValueError("privacy.lockScreen must be hide, redact or show")
    if not isinstance(cfg["grouping"]["enabled"], bool):
        raise ValueError("grouping.enabled must be boolean")
    number(cfg["grouping"]["windowSeconds"], "grouping.windowSeconds", 0, 86400)
    number(cfg["history"]["drawerLimit"], "history.drawerLimit", 0, 1000000)
    if not isinstance(cfg["history"]["drawerLimit"], int):
        raise ValueError("history.drawerLimit must be an integer")
    for key, bounds in {"animationMs": (0, 1000), "heightPercent": (20, 95)}.items():
        number(cfg["stack"][key], "stack." + key, *bounds)
    hooks = cfg["actions"].get("nativeHooks", [])
    if not isinstance(hooks, list):
        raise ValueError("actions.nativeHooks must be an array")
    for hook in hooks:
        if not isinstance(hook, dict) or any(not isinstance(hook.get(k), str) or not hook[k].strip() for k in ("caller", "identifier")):
            raise ValueError("actions.nativeHooks needs caller and identifier")
        command = hook.get("command")
        if not isinstance(command, list) or not command or not command[0] or any(not isinstance(arg, str) or "\0" in arg for arg in command):
            raise ValueError("actions.nativeHooks command must be an argument array")
    blacklist = cfg["actions"]["blacklist"]
    if not isinstance(blacklist, list) or any(not isinstance(caller, str) or not caller.strip() for caller in blacklist):
        raise ValueError("actions.blacklist must be an array of nonempty caller names")
    handlers = cfg["handlers"]
    if not isinstance(handlers, list) or len(handlers) > 64:
        raise ValueError("handlers must be an array of at most 64 rules")
    ids = set()
    for rule in handlers:
        if not isinstance(rule, dict) or not isinstance(rule.get("id"), str) or not rule["id"].strip() or rule["id"] in ids:
            raise ValueError("handlers require unique nonempty ids")
        ids.add(rule["id"])
        match = rule.get("match")
        if not isinstance(match, dict) or not match or set(match) - {"app", "telegram", "title", "chatId"}:
            raise ValueError("handlers.match requires a source selector")
        if not match.get("app") and match.get("telegram") is not True:
            raise ValueError("handlers.match requires app or telegram: true")
        for key, value in match.items():
            if (key == "telegram" and value is not True) or (key != "telegram" and (not isinstance(value, str) or not value.strip())):
                raise ValueError("invalid handlers.match selector")
        pattern = rule.get("pattern")
        if not isinstance(pattern, str) or not pattern or len(pattern) > 4096:
            raise ValueError("handlers.pattern must be a nonempty template regex")
        try:
            compiled = re.compile(pattern)
        except re.error as error:
            raise ValueError("invalid handlers.pattern") from error
        if not isinstance(rule.get("card"), str) or (rule["card"] != "key-value" and rule["card"] not in cfg["components"]):
            raise ValueError("handlers.card must be key-value or a configured component")
        captures = rule.get("captures")
        if not isinstance(captures, dict) or not captures:
            raise ValueError("handlers.captures must map names to capture numbers")
        for key, value in captures.items():
            if not re.fullmatch(r"[a-zA-Z][a-zA-Z0-9_]*", key) or key in {"fields", "__proto__", "constructor", "prototype"}:
                raise ValueError("invalid handlers.captures name")
            if isinstance(value, bool) or not isinstance(value, int) or not 1 <= value <= compiled.groups:
                raise ValueError("handlers.captures references a missing capture")
        fields = rule.get("fields", [])
        if not isinstance(fields, list) or len(fields) > 16:
            raise ValueError("handlers.fields must be an array of at most 16 fields")
        for field in fields:
            if not isinstance(field, dict) or not isinstance(field.get("label"), str) or not field["label"].strip() or not isinstance(field.get("key"), str) or field["key"] not in captures:
                raise ValueError("handlers.fields requires label and captured key")
        if rule["card"] == "key-value" and ("heading" not in captures or not fields):
            raise ValueError("key-value handlers require heading and fields")
    theme_name = cfg["theme"]
    if theme_name in ("monochrome", "rose-punk"):
        theme_path = root / "themes" / f"{theme_name}.json"
    else:
        theme_path = path.parent / os.path.expanduser(theme_name)
    cfg["style"] = merge(json.loads(theme_path.read_text()), cfg["style"])
    for key in ("background", "surface", "border", "text", "muted", "accent", "critical", "success"):
        if not re.fullmatch(r"#[0-9a-fA-F]{6}(?:[0-9a-fA-F]{2})?", cfg["style"][key]):
            raise ValueError(f"style.{key} must be a hex color")
    for key, bounds in {"fontSize": (8, 48), "radius": (0, 100), "padding": (4, 64), "gap": (0, 64),
                        "borderWidth": (0, 8), "iconSize": (16, 256), "width": (240, 1200), "maxBodyLines": (1, 30)}.items():
        number(cfg["style"][key], f"style.{key}", *bounds)
    if not isinstance(cfg["style"]["font"], str):
        raise ValueError("style.font must be a string")
    for name, source in cfg["components"].items():
        if not re.fullmatch(r"[a-zA-Z][a-zA-Z0-9_-]*", name):
            raise ValueError(f"invalid component name: {name}")
        source_path = (path.parent / os.path.expanduser(source)).resolve()
        if source_path.suffix != ".qml" or not source_path.is_file():
            raise ValueError(f"component {name} must point to a local QML file")
        cfg["components"][name] = source_path.as_uri()
    return cfg


def database():
    directory = state_path()
    directory.mkdir(mode=0o700, parents=True, exist_ok=True)
    os.chmod(directory, 0o700)
    db = sqlite3.connect(directory / "history.sqlite3", timeout=5)
    os.chmod(directory / "history.sqlite3", 0o600)
    db.execute("CREATE TABLE IF NOT EXISTS events (id INTEGER PRIMARY KEY, time TEXT NOT NULL, event TEXT NOT NULL, notice_id TEXT, app TEXT, payload TEXT NOT NULL)")
    db.execute("CREATE INDEX IF NOT EXISTS events_time ON events(time)")
    return db


def prune_history(db, history, now=None):
    """Apply retention inside the caller's transaction; disabled logging keeps history."""
    if not history["enabled"]:
        return False
    before = db.total_changes
    if history["maxAgeDays"]:
        cutoff = ((now or dt.datetime.now(dt.timezone.utc)) - dt.timedelta(days=history["maxAgeDays"]))
        db.execute("DELETE FROM events WHERE time < ?", (cutoff.isoformat(timespec="milliseconds"),))
    if history["maxEvents"]:
        db.execute("DELETE FROM events WHERE id <= COALESCE((SELECT id FROM events ORDER BY id DESC LIMIT 1 OFFSET ?), 0)",
                   (history["maxEvents"],))
    return db.total_changes != before


def record(db, cfg, event):
    history = cfg["history"]
    notice = event.get("notice", {})
    if not history["enabled"] or (notice.get("transient") and not history["logTransient"]):
        return
    # Log display data and labels; never store runnable command arrays or binary artwork.
    event = copy.deepcopy(event)
    for action in event.get("notice", {}).get("actions", []):
        action.pop("command", None)
    event["time"] = dt.datetime.now(dt.timezone.utc).isoformat(timespec="milliseconds")
    with db:
        db.execute("INSERT INTO events(time,event,notice_id,app,payload) VALUES(?,?,?,?,?)",
                   (event["time"], event["event"], notice.get("id"), notice.get("app", ""), json.dumps(event, ensure_ascii=False)))
        prune_history(db, history)


def history_notices(db, limit):
    """Latest snapshot for each session/id, including its current close state."""
    seen, result = set(), []
    for (payload,) in db.execute("SELECT payload FROM events ORDER BY id DESC"):
        event = json.loads(payload)
        notice = event.get("notice", {})
        if not notice.get("id"):
            continue
        key = str(event.get("session", "legacy")) + "/" + notice["id"]
        if key in seen:
            continue
        seen.add(key)
        item = {**notice, "historyKey": key, "session": event.get("session", "legacy"),
                "lastEvent": event["event"], "loggedAt": event["time"]}
        item["actions"] = []  # Historical snapshots never replay executable actions.
        if str(item.get("image", "")).startswith("image:"):
            item["image"] = ""
        result.append(item)
    result.sort(key=lambda item: item.get("received", 0), reverse=True)
    return result[:limit] if limit else result


def emit(value):
    print(json.dumps(value, ensure_ascii=False), flush=True)


def worker(root):
    os.umask(0o077)
    cfg, db, previous = None, None, None
    buffer = b""
    observer = LockObserver()
    lock_state = None
    dirty_history = True
    last_history = 0
    next_config = 0
    next_prune = 0
    try:
        while True:
            now = time.monotonic()
            observer.connect()
            if observer.state != lock_state:
                lock_state = observer.state
                emit({"lockState": lock_state})
            if now >= next_config:
                next_config = now + 1
                try:
                    current = load_config(root)
                    serial = json.dumps(current, sort_keys=True)
                    if serial != previous:
                        cfg, previous = current, serial
                        next_prune = 0
                        emit({"config": cfg})
                        dirty_history = True
                except (ValueError, OSError, TypeError, AttributeError, KeyError) as error:
                    message = str(error)
                    if previous != "error:" + message:
                        emit({"error": "Configuration: " + message})
                        previous = "error:" + message
            if cfg and now >= next_prune:
                next_prune = now + 60
                try:
                    if db is None:
                        db = database()
                    with db:
                        changed = prune_history(db, cfg["history"])
                    dirty_history = dirty_history or changed
                except (OSError, sqlite3.Error) as error:
                    emit({"error": "History: " + str(error)})
            if cfg and dirty_history and now - last_history >= 0.2:
                try:
                    if db is None:
                        db = database()
                    emit({"history": history_notices(db, cfg["history"]["drawerLimit"])})
                    dirty_history = False
                    last_history = now
                except (OSError, sqlite3.Error) as error:
                    emit({"error": "History: " + str(error)})
                    last_history = now
            readers = [sys.stdin]
            if observer.socket:
                readers.append(observer.socket)
            ready, _, _ = select.select(readers, [], [], 0.2)
            if observer.socket in ready:
                observer.read()
            if sys.stdin not in ready:
                continue
            data = os.read(sys.stdin.fileno(), 65536)
            if not data:
                break
            buffer += data
            while b"\n" in buffer:
                line, buffer = buffer.split(b"\n", 1)
                try:
                    event = json.loads(line)
                    if event.get("op") == "history":
                        dirty_history = True
                    elif cfg is not None:
                        if db is None:
                            db = database()
                        record(db, cfg, event)
                        dirty_history = True
                except (ValueError, OSError, sqlite3.Error) as error:
                    emit({"error": "History: " + str(error)})
    finally:
        observer.close()
        if db:
            db.close()


def ipc(root, request):
    payload = json.dumps(request, separators=(",", ":"))
    if len(payload) > 131072:
        raise ValueError("Request exceeds 128 KiB")
    # Process arguments are visible across user accounts on many Linux hosts.
    # Only a private, short-lived filename may enter the command line.
    with tempfile.TemporaryDirectory(prefix="notjik-ipc-", dir=os.environ.get("XDG_RUNTIME_DIR")) as directory:
        path = Path(directory) / "request.json"
        with open(path, "x", encoding="utf-8", opener=lambda name, flags: os.open(name, flags, 0o600)) as target:
            target.write(payload)
        result = subprocess.run(["quickshell", "ipc", "--path", str(root / "qml"), "call", "notjik", "requestFile",
                                 str(path)], text=True, capture_output=True, timeout=5)
    if result.returncode:
        raise ValueError(result.stderr.strip() or "notjik is not running; start it with notjik")
    try:
        response = json.loads(result.stdout)
    except ValueError as error:
        raise ValueError("Unexpected Quickshell IPC response: " + result.stdout.strip()) from error
    if not response.get("ok"):
        raise ValueError(response.get("error", "request failed"))
    return response


def launch(root, arguments):
    parser = argparse.ArgumentParser(prog="notjik", description="Quickshell notification server")
    parser.add_argument("--daemon", action="store_true")
    parser.add_argument("--config", type=Path)
    args = parser.parse_args(arguments)
    if args.config:
        os.environ["NOTJIK_CONFIG"] = str(args.config.resolve())
    try:
        load_config(root)
        # Read the owner without activating another notification daemon.
        check = subprocess.run(["busctl", "--user", "call", "org.freedesktop.DBus", "/org/freedesktop/DBus",
                                "org.freedesktop.DBus", "NameHasOwner", "s", "org.freedesktop.Notifications"],
                               capture_output=True, text=True)
        if check.returncode:
            raise ValueError("Cannot connect to the session bus: " + check.stderr.strip())
        if "true" in check.stdout:
            raise ValueError("org.freedesktop.Notifications already has an owner. Stop the existing notification daemon before starting notjik.")
        command = ["quickshell", "--path", str(root / "qml"), "--no-duplicate"]
        if args.daemon:
            command.append("--daemonize")
        os.execvp(command[0], command)
    except (OSError, ValueError, TypeError, KeyError) as error:
        print(f"notjik: {error}", file=sys.stderr)
        return 1


def duration(value):
    if value in ("never", "until-dismissed", "0"):
        return 0
    if value == "default":
        return -1
    match = re.fullmatch(r"(\d+(?:\.\d+)?)(ms|s|m|h)?", value)
    if not match:
        raise argparse.ArgumentTypeError("use default, never, or a duration such as 8s / 500ms / 2m")
    return float(match[1]) * {None: 1000, "ms": 1, "s": 1000, "m": 60000, "h": 3600000}[match[2]]


def clean_terminal(text):
    return re.sub(r"[\x00-\x1f\x7f-\x9f]", " ", str(text))


def show_logs(args):
    if not (state_path() / "history.sqlite3").exists():
        if not args.follow:
            return
    db = database()
    last_id = 0
    first = True
    try:
        while True:
            clauses, values = ["id > ?"], [last_id]
            for column, value in (("app", args.app), ("event", args.event)):
                if value:
                    clauses.append(column + " = ?")
                    values.append(value)
            if args.since:
                clauses.append("time >= ?")
                values.append(args.since)
            order = "DESC" if first else "ASC"
            rows = db.execute("SELECT id,payload FROM events WHERE " + " AND ".join(clauses) +
                              f" ORDER BY id {order} LIMIT ?", [*values, args.limit]).fetchall()
            if first:
                rows.reverse()
            for row_id, payload in rows:
                last_id = max(last_id, row_id)
                item = json.loads(payload)
                if args.json:
                    print(payload, flush=True)
                else:
                    notice = item.get("notice", {})
                    print(clean_terminal(f"{item['time']}  {item['event']:<10} {notice.get('id', ''):<14} "
                                         f"{notice.get('app', '')}: {notice.get('title', '')}"), flush=True)
                    if notice.get("body"):
                        print("  " + clean_terminal(notice["body"]), flush=True)
            first = False
            if not args.follow:
                break
            time.sleep(0.5)
    finally:
        db.close()


def main(root):
    if len(sys.argv) > 1 and sys.argv[1] == "telegram":
        arguments = sys.argv[2:]
        if arguments and arguments[0] == "setup":
            script = root / "install-telegram.sh"
            if not script.exists():
                script = root / "scripts/install-telegram.sh"
            return subprocess.call(["bash", str(script)])
        if arguments and arguments[0] in ("login", "run", "doctor"):
            runtime = Path(os.environ.get("NOTJIK_TELEGRAM_PYTHON", str(Path.home() / ".local/share/notjik/telegram-venv/bin/python")))
            if runtime.is_file() and Path(sys.prefix) != runtime.parent.parent:
                os.execv(str(runtime), [str(runtime), "-B", str(root / "telegram_bridge.py"), *arguments])
        from telegram_bridge import main as telegram_main
        return telegram_main(root, arguments)
    if len(sys.argv) > 1 and sys.argv[1] == "demo":
        from demo import main as demo_main
        return demo_main(root, sys.argv[2:])
    if len(sys.argv) > 1 and sys.argv[1] == "compare":
        from comparison import main as compare_main
        return compare_main(root, sys.argv[2:])
    parser = argparse.ArgumentParser(prog="notjikctl")
    sub = parser.add_subparsers(dest="op", required=True)
    for name in ("pause", "resume", "toggle", "status", "list", "monitors", "reload"):
        sub.add_parser(name)
    send = sub.add_parser("send", help="show a card; prints its replacement ID")
    send.add_argument("title")
    send.add_argument("body", nargs="?", default="")
    send.add_argument("--app", default="notjikctl")
    send.add_argument("--type", default="standard", help="standard, progress, status, timer, telegram, results-table, or a registered component")
    send.add_argument("--urgency", choices=["low", "normal", "critical"], default="normal")
    send.add_argument("--timeout", type=duration, default=None)
    send.add_argument("--icon", default="")
    send.add_argument("--image", default="")
    identity = send.add_mutually_exclusive_group()
    identity.add_argument("--replace")
    identity.add_argument("--id", help="sender-chosen key, scoped by --app; create or update a persistent card")
    send.add_argument("--group", help="explicit grouping key; defaults to the application")
    send.add_argument("--data", default="{}", help="component data as a JSON object")
    send.add_argument("--actions", default="[]", help='JSON array: [{"label":"Open","command":["xdg-open","https://…"]}]')
    send.add_argument("--resident", action="store_true")
    send.add_argument("--transient", action="store_true")
    dismiss = sub.add_parser("dismiss")
    dismiss.add_argument("id", help="notification ID or all")
    action = sub.add_parser("action", help="invoke a card action by ID and zero-based index")
    action.add_argument("id")
    action.add_argument("index", type=int)
    history = sub.add_parser("history", help="control the graphical history drawer")
    history.add_argument("mode", choices=["show", "hide", "toggle"], nargs="?", default="toggle")
    navigate = sub.add_parser("navigate", help="browse older/newer notifications")
    navigate.add_argument("direction", choices=["previous", "next"])
    sub.add_parser("unfocus", help="release notification keyboard focus")
    logs = sub.add_parser("logs")
    logs.add_argument("--limit", type=int, default=50)
    logs.add_argument("--json", action="store_true")
    logs.add_argument("--follow", action="store_true")
    logs.add_argument("--app")
    logs.add_argument("--event")
    logs.add_argument("--since", help="ISO 8601 time, normalized to UTC")
    init = sub.add_parser("init", help="create a configuration without overwriting an existing one")
    init.add_argument("--theme", choices=["monochrome", "rose-punk"], default="monochrome")
    sub.add_parser("config", help="print the effective, validated configuration")
    sub.add_parser("demo", help="named visual fixtures; demo --help")
    sub.add_parser("compare", help="isolated side-by-side notify-send comparison; compare --help")
    args = parser.parse_args()
    try:
        if args.op == "init":
            path = config_path()
            path.parent.mkdir(parents=True, exist_ok=True)
            cfg = copy.deepcopy(DEFAULTS)
            cfg["theme"] = args.theme
            with path.open("x") as handle:
                handle.write(json.dumps(cfg, indent=2) + "\n")
            print(path)
        elif args.op == "config":
            print(json.dumps(load_config(root), indent=2))
        elif args.op == "logs":
            if args.limit < 1:
                raise ValueError("--limit must be positive")
            if args.since:
                args.since = dt.datetime.fromisoformat(args.since.replace("Z", "+00:00")).astimezone(dt.timezone.utc).isoformat(timespec="milliseconds")
            show_logs(args)
        elif args.op == "send":
            notice = vars(args).copy()
            notice.pop("op")
            if notice["timeout"] is None:
                notice.pop("timeout")
            notice["data"] = json.loads(args.data)
            notice["actions"] = json.loads(args.actions)
            for key in ("icon", "image"):
                if notice[key].startswith(("/", "~", "./", "../")):
                    notice[key] = Path(notice[key]).expanduser().resolve().as_uri()
            print(ipc(root, {"op": "send", "notice": notice})["id"])
        else:
            print(json.dumps(ipc(root, vars(args)), indent=2))
        return 0
    except (OSError, ValueError, TypeError, KeyError, sqlite3.Error) as error:
        print(f"notjikctl: {error}", file=sys.stderr)
        return 1
    except KeyboardInterrupt:
        return 130


if __name__ == "__main__":
    worker(Path(__file__).resolve().parent)
