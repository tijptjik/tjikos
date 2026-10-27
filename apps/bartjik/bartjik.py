"""bartjik launcher and control interface."""

import argparse
import asyncio
import json
import os
from pathlib import Path
import subprocess
import sys
from bartjik_core import config

ROOT = Path(__file__).resolve().parent


def ipc(request):
    qml = Path(os.environ.get("BARTJIK_QML", str(ROOT / "qml")))
    result = subprocess.run(
        [
            "quickshell",
            "ipc",
            "--path",
            str(qml),
            "call",
            "bartjik",
            "request",
            json.dumps(request),
        ],
        capture_output=True,
        text=True,
        timeout=5,
    )
    if result.returncode:
        raise ValueError("bartjik is not running for this QML path")
    try:
        response = json.loads(result.stdout)
    except ValueError:
        raise ValueError("Invalid bartjik IPC response") from None
    if not response.get("ok"):
        raise ValueError(response.get("error", "IPC failed"))
    return response


def launch(arguments):
    parser = argparse.ArgumentParser(prog="bartjik")
    parser.add_argument("--config", type=Path)
    parser.add_argument("--demo", action="store_true")
    parser.add_argument("--daemon", action="store_true")
    args = parser.parse_args(arguments)
    if args.config:
        os.environ["BARTJIK_CONFIG"] = str(args.config.resolve())
    if args.demo:
        os.environ["BARTJIK_DEMO"] = "1"
    os.environ["BARTJIK_HOME"] = str(ROOT)
    config.load()  # Fail clearly before opening a surface.
    cmd = [
        "quickshell",
        "--path",
        os.environ.get("BARTJIK_QML", str(ROOT / "qml")),
        "--no-duplicate",
    ]
    if args.daemon:
        cmd.append("--daemonize")
    os.execvp(cmd[0], cmd)


def main(arguments=None):
    parser = argparse.ArgumentParser(prog="bartjikctl")
    parser.add_argument("--config", type=Path)
    sub = parser.add_subparsers(dest="op", required=True)
    for op in ("status", "monitors", "bars", "modules", "reload"):
        sub.add_parser(op)
    spotify = sub.add_parser("spotify", help="configure the optional Spotify lyrics source")
    spotify.add_argument("action", choices=("login", "logout", "status"))
    cfg = sub.add_parser("config")
    cfg.add_argument(
        "--validate",
        action="store_true",
        help="validate file offline instead of inspecting the running effective config",
    )
    for op in ("show", "hide", "toggle", "toggle-reveal", "show-reveal", "hide-reveal"):
        sub.add_parser(op).add_argument("bar", nargs="?", default="all")
    init = sub.add_parser("init")
    init.add_argument(
        "--preset", choices=("desktop", "laptop", "minimal"), default="minimal"
    )
    init.add_argument(
        "--theme", choices=("monochrome", "rose-punk"), default="monochrome"
    )
    args = parser.parse_args(arguments)
    if args.config:
        os.environ["BARTJIK_CONFIG"] = str(args.config.resolve())
    try:
        if args.op == "spotify":
            from bartjik_core.spotify_lyrics import configure
            result = configure(args.action)
        elif args.op == "init":
            p = config.path()
            if p.exists():
                raise ValueError("Config already exists; not overwriting it")
            value = json.loads((ROOT / "examples" / f"{args.preset}.json").read_text())
            value["theme"] = args.theme
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_text(json.dumps(value, indent=2) + "\n")
            result = {"ok": True, "path": str(p)}
        elif args.op == "config" and args.validate:
            result = {"ok": True, "config": config.load()}
        else:
            result = ipc(
                {"op": args.op, **({"bar": args.bar} if hasattr(args, "bar") else {})}
            )
        print(json.dumps(result, ensure_ascii=False, indent=2))
        return 0
    except (OSError, ValueError, subprocess.SubprocessError) as e:
        print(json.dumps({"ok": False, "error": str(e)}), file=sys.stderr)
        return 1


if __name__ == "__main__":
    if sys.argv[1:2] == ["serve"]:
        from bartjik_core.engine import serve

        asyncio.run(serve("--demo" in sys.argv))
    else:
        raise SystemExit(main())
