"""Trusted argument arrays and registered actions. Context is JSON in an env var."""

import asyncio
import configparser
import json
import os
from pathlib import Path
import subprocess
from . import config
from .io import command, json_command, Unavailable
from .providers import audio_state, running


def choose(module, row, part, event):
    event = "leftClick" if event == "click" else event
    chosen = None

    def apply(actions):
        nonlocal chosen
        if event == "leftClick" and "click" in actions:
            chosen = actions["click"]
        if event in actions:
            chosen = actions[event]

    apply(row.get("actions", {}))
    apply(row.get(part, {}).get("actions", {}))
    for patch in (module, module.get("items", {}).get(row["id"], {})):
        apply(patch.get("actions", {}))
        apply(patch.get(part, {}).get("actions", {}))
    return chosen


def lua_string(value):
    # Lua quoted-string escape, not JSON's unsupported \uNNNN escape.
    return (
        '"'
        + "".join(
            "\\" + c if c in '\\"' else f"\\{ord(c):03d}" if ord(c) < 32 else c
            for c in str(value)
        )
        + '"'
    )


def workspace_commands(row, screen):
    # Explicit monitor selection followed by an absolute compositor identifier.
    monitor = "hl.dsp.focus({ monitor = " + lua_string(screen) + " })"
    workspace = row["workspace"]
    if row.get("special"):
        target = (
            "hl.dsp.workspace.toggle_special("
            + lua_string(str(workspace).removeprefix("special:"))
            + ")"
        )
    else:
        target = (
            "hl.dsp.focus({ workspace = "
            + (
                str(workspace)
                if isinstance(workspace, int)
                else lua_string("name:" + workspace)
            )
            + " })"
        )
    return [["hyprctl", "dispatch", monitor], ["hyprctl", "dispatch", target]]


class Executor:
    def __init__(self):
        self.locks = {}
        self.hover_panels = {}
        self.panel_closers = {}
        self.hover_active = set()

    def lock(self, name):
        return self.locks.setdefault(name, asyncio.Lock())

    async def close_hover_panels(self, except_kind=None):
        for kind, base in list(self.hover_panels.items()):
            if kind == except_kind:
                continue
            async with self.lock(kind + ".panel"):
                if self.hover_panels.pop(kind, None) is not None:
                    try:
                        await command([*base, "close"], timeout=2)
                    except Unavailable:
                        pass

    def leave_panel(self, kind):
        self.hover_active.discard(kind)
        previous = self.panel_closers.pop(kind, None)
        if previous:
            previous.cancel()
        async def close_after_pointer_leaves():
            try:
                await asyncio.sleep(.6)
                while kind in self.hover_panels:
                    base = self.hover_panels[kind]
                    try:
                        geometry = json.loads(await command([*base, "geometry"], timeout=2))
                        if geometry.get("dragging"):
                            self.hover_panels.pop(kind, None)
                            return  # A deliberate drag gives ownership to the user.
                        if geometry.get("hovered"):
                            await asyncio.sleep(.25)
                            continue
                    except (ValueError, Unavailable):
                        pass
                    await self.close_hover_panels()
                    break
            finally:
                if self.panel_closers.get(kind) is asyncio.current_task():
                    self.panel_closers.pop(kind, None)
        self.panel_closers[kind] = asyncio.create_task(close_after_pointer_leaves())

    async def close(self):
        tasks = list(self.panel_closers.values())
        for task in tasks:
            task.cancel()
        await asyncio.gather(*tasks, return_exceptions=True)
        await self.close_hover_panels()

    async def panel(self, kind, context, module):
        root = Path(
            module.get(
                "panelPath",
                str(
                    Path(
                        os.environ.get("XDG_CONFIG_HOME", str(Path.home() / ".config"))
                    )
                    / "quickshell"
                    / kind
                ),
            )
        ).expanduser()
        if not (root / "shell.qml").exists():
            raise Unavailable(f"{kind} panel is not installed")
        anchor = context["anchor"]
        payload = json.dumps(
            dict(
                screen=context["monitor"],
                x=anchor["x"] + anchor["width"] / 2,
                bottom=anchor["y"],
            )
        )
        base = ["quickshell", "ipc", "-p", str(root), "call", kind]
        closer = self.panel_closers.pop(kind, None)
        if closer:
            closer.cancel()
        await self.close_hover_panels(except_kind=kind)
        hovering = context.get("event") == "hoverEnter"
        if hovering:
            self.hover_active.add(kind)
        else:
            self.hover_active.discard(kind)
        async with self.lock(kind + ".panel"):
            if not hovering and kind in self.hover_panels:
                self.hover_panels.pop(kind, None)  # Clicking keeps an already-hovered panel pinned.
                return
            try:
                opened = await command([*base, "isOpen"], timeout=2)
                if hovering and opened.strip() == "true":
                    return
                await command([*base, "toggleAt", payload], timeout=2)
                if hovering:
                    self.hover_panels[kind] = base
                    if kind not in self.hover_active:
                        self.leave_panel(kind)
                return
            except Unavailable:
                pass
            env = os.environ.copy()
            env[kind.upper() + "_WIDGET_ANCHOR"] = payload
            await command(
                ["quickshell", "-d", "-n", "-p", str(root)], env=env, timeout=5
            )
            for _ in range(30):
                try:
                    if (await command([*base, "isOpen"], timeout=1)).strip() == "true":
                        if hovering:
                            self.hover_panels[kind] = base
                            if kind not in self.hover_active:
                                self.leave_panel(kind)
                        return
                except Unavailable:
                    pass
                await asyncio.sleep(0.1)
            raise Unavailable(f"{kind} panel did not become ready")

    async def execute(self, action, context, module, row, items):
        if action is None:
            return
        if isinstance(action, dict):
            args = action["command"]
            config.argv(args)
            if action.get("detach"):
                env = os.environ.copy()
                env["BARTJIK_CONTEXT"] = json.dumps(context, ensure_ascii=False)
                subprocess.Popen(
                    args,
                    env=env,
                    stdin=subprocess.DEVNULL,
                    stdout=subprocess.DEVNULL,
                    stderr=subprocess.DEVNULL,
                    start_new_session=True,
                )
                return
            action = args
        if isinstance(action, list):
            config.argv(action)
            env = os.environ.copy()
            env["BARTJIK_CONTEXT"] = json.dumps(context, ensure_ascii=False)
            await command(action, timeout=module.get("actionTimeout", 15), env=env)
            return
        if action not in config.INTERNAL:
            raise ValueError("unknown internal action")
        if action.endswith(".panel"):
            return await self.panel(action.split(".")[0], context, module)
        if action.startswith("workspace."):
            eligible = [
                i
                for i in items
                if not i.get("special") or module.get("scrollSpecial", False)
            ]
            if action != "workspace.activate":
                if not eligible:
                    return
                current = next(
                    (i for i, r in enumerate(eligible) if r["state"].get("active")), 0
                )
                row = eligible[
                    (current + (1 if action.endswith("next") else -1)) % len(eligible)
                ]
            async with self.lock("workspace"):
                for args in workspace_commands(row, context["monitor"]):
                    await command(args)
        elif action.startswith("audio."):
            async with self.lock("audio"):
                sinks, s = await audio_state()
                if action == "audio.route":
                    choices = module.get("routes", [])
                    routes = [
                        sink
                        for sink in sinks
                        if not choices
                        or any(
                            choice in (sink["name"], sink.get("description"))
                            for choice in choices
                        )
                    ]
                    if len(routes) < 2:
                        raise Unavailable(
                            "At least two available output routes are required"
                        )
                    idx = next(
                        (i for i, r in enumerate(routes) if r["name"] == s["name"]), -1
                    )
                    name = routes[(idx + 1) % len(routes)]["name"]
                    await command(["pactl", "set-default-sink", name])
                    failures = []
                    for stream in await json_command(
                        ["pactl", "-f", "json", "list", "sink-inputs"]
                    ):
                        try:
                            await command(
                                ["pactl", "move-sink-input", str(stream["index"]), name]
                            )
                        except Unavailable:
                            remaining = await json_command(
                                ["pactl", "-f", "json", "list", "sink-inputs"]
                            )
                            if any(r["index"] == stream["index"] for r in remaining):
                                failures.append(stream["index"])
                    if failures:
                        raise Unavailable(
                            "Output changed; some playback streams could not be moved"
                        )
                elif action == "audio.mute":
                    await command(
                        ["pactl", "set-sink-mute", s["name"], "0" if s["mute"] else "1"]
                    )
                else:
                    values = list(s["volume"].values())
                    current = round(
                        sum(v["value"] for v in values) / len(values) / 65536 * 100
                    )
                    volume = max(
                        0, min(100, current + (4 if action == "audio.up" else -4))
                    )
                    await command(
                        ["pactl", "set-sink-volume", s["name"], str(volume) + "%"]
                    )
        elif action == "locale.cycle":
            async with self.lock("locale"):
                current = await command(["fcitx5-remote", "--check", "-n"])
                group = await command(["fcitx5-remote", "--check", "-q"])
                profile = configparser.ConfigParser(interpolation=None)
                profile.read(
                    Path(
                        os.environ.get("XDG_CONFIG_HOME", str(Path.home() / ".config"))
                    )
                    / "fcitx5/profile"
                )
                prefix = next(
                    (
                        s
                        for s in profile
                        if s.startswith("Groups/")
                        and s.count("/") == 1
                        and profile[s].get("Name", "").strip('"') == group
                    ),
                    None,
                )
                if prefix is None:
                    raise Unavailable("Active Fcitx group is missing")
                sections = sorted(
                    (s for s in profile if s.startswith(prefix + "/Items/")),
                    key=lambda s: int(s.rsplit("/", 1)[1]),
                )
                methods = [profile[s]["Name"].strip('"') for s in sections]
                if not methods:
                    raise Unavailable("Active Fcitx group is empty")
                index = methods.index(current) if current in methods else -1
                await command(
                    [
                        "fcitx5-remote",
                        "--check",
                        "-s",
                        methods[(index + 1) % len(methods)],
                    ]
                )
        elif action == "DND.toggle":
            await command([module.get("ctl", "notjikctl"), "toggle"])
        elif action == "tailscale.toggle":
            async with self.lock("tailscale"):
                state = (await json_command(["tailscale", "status", "--json"])).get(
                    "BackendState"
                )
                if not state:
                    raise Unavailable("Tailscale unavailable")
                await command(
                    ["tailscale", "down" if state == "Running" else "up"], timeout=15
                )
        elif action in ("caffeine.toggle", "nightlight.toggle"):
            name = "hypridle" if action.startswith("caffeine") else "hyprsunset"
            async with self.lock(name):
                if await running(name):
                    await command(["pkill", "-x", name])
                else:
                    # A deliberately persistent desktop process, not a polling child.
                    subprocess.Popen(
                        [name],
                        stdout=subprocess.DEVNULL,
                        stderr=subprocess.DEVNULL,
                        start_new_session=True,
                    )
        elif action == "monitor.toggle":
            await command(
                [
                    module.get(
                        "commandPath", str(Path.home() / ".local/bin/monitor-game-mode")
                    ),
                    "toggle",
                ],
                timeout=15,
            )
        else:
            raise ValueError("native action must be handled by QML")
