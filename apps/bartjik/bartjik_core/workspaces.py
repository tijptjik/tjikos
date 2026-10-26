"""One compositor subscription; screen views use ownership, never focused monitor."""

import asyncio
import os
from pathlib import Path
from .io import json_command, Unavailable
from .model import item
from .icons import icon


def view(snapshot, screen, options):
    monitors = {m["name"]: m for m in snapshot.get("monitors", [])}
    if screen not in monitors:
        return []
    monitor = monitors[screen]
    urgent = set(snapshot.get("urgent", []))
    active = monitor.get("activeWorkspace", {}).get("id")
    special = monitor.get("specialWorkspace", {}).get("id")
    owned = [w for w in snapshot.get("workspaces", []) if w.get("monitor") == screen]
    existing = {str(w["name"]) for w in snapshot.get("workspaces", [])}
    for name in options.get("persistent", {}).get(screen, []):
        if str(name) not in existing:
            owned.append(
                dict(id=None, name=str(name), monitor=screen, placeholder=True)
            )
    if options.get("occupiedOnly", False):
        owned = [
            w
            for w in owned
            if w.get("windows", 0) > 0
            or (w.get("id") is not None and w.get("id") in (active, special))
        ]
    if not options.get("special", True):
        owned = [w for w in owned if not w["name"].startswith("special:")]

    def order(w):
        n = w["name"]
        return (
            (0, int(n))
            if n.isdecimal()
            else (2 if n.startswith("special:") else 1, n.casefold())
        )

    result = []
    for w in sorted(owned, key=order):
        name = w["name"]
        wid = w.get("id")
        is_special = name.startswith("special:")
        identifier = str(wid) if wid is not None else "empty:" + name
        # Keep numeric placeholders identical when they become real workspaces.
        if name.isdecimal():
            identifier = str(int(name))
        active_here = wid is not None and wid == (special if is_special else active)
        row = item(
            identifier,
            options.get("labels", {}).get(name, name.removeprefix("special:")),
            state=dict(
                active=active_here,
                focused=active_here and monitor.get("focused", False),
                urgent=wid in urgent,
                placeholder=w.get("placeholder", False),
            ),
            tooltip=f"{name} · {screen}",
            actions={
                "leftClick": "workspace.activate",
                "scrollUp": "workspace.previous",
                "scrollDown": "workspace.next",
            },
        )
        if options.get("label", "name") in ("circles", "circles_name"):
            if options["label"] == "circles":
                row.pop("text", None)
                row["type"] = "icon"
            else:
                row["type"] = "icon_text"
            row["icon"] = dict(
                kind="image",
                value=icon(
                    "circle-filled" if row["state"]["focused"] else "circle-open"
                ),
                style={},
                actions={},
            )
        row["workspace"] = name if is_special or not name.isdecimal() else int(name)
        row["special"] = is_special
        result.append(row)
    return result


async def snapshot():
    monitors, workspaces, clients = await asyncio.gather(
        *[
            json_command(["hyprctl", "-j", what], limit=1048576)
            for what in ("monitors", "workspaces", "clients")
        ]
    )
    if not all(isinstance(v, list) for v in (monitors, workspaces, clients)):
        raise Unavailable("malformed compositor snapshot")
    return dict(
        monitors=[
            {
                k: m.get(k)
                for k in ("name", "activeWorkspace", "specialWorkspace", "focused")
            }
            for m in monitors
        ],
        workspaces=[
            {k: w.get(k) for k in ("id", "name", "monitor", "windows")}
            for w in workspaces
        ],
        urgent=[c.get("workspace", {}).get("id") for c in clients if c.get("urgent")],
    )


class Watcher:
    def __init__(self, notify):
        self.notify = notify
        self.data = {}
        self.error = "connecting"
        self.task = None

    def start(self):
        self.task = asyncio.create_task(self.run())

    async def close(self):
        if self.task:
            self.task.cancel()
            await asyncio.gather(self.task, return_exceptions=True)

    async def refresh(self):
        self.data = await snapshot()
        self.error = ""
        self.notify()

    async def run(self):
        delay = 1
        while True:
            writer = None
            try:
                sock = (
                    Path(os.environ["XDG_RUNTIME_DIR"])
                    / "hypr"
                    / os.environ["HYPRLAND_INSTANCE_SIGNATURE"]
                    / ".socket2.sock"
                )
                reader, writer = await asyncio.open_unix_connection(
                    str(sock), limit=65536
                )
                # Subscribe first; events during snapshot remain queued for reconciliation.
                await self.refresh()
                delay = 1
                while True:
                    try:
                        line = await asyncio.wait_for(reader.readline(), 10)
                    except asyncio.TimeoutError:
                        await self.refresh()
                        continue
                    if not line:
                        raise Unavailable("compositor disconnected")
                    await asyncio.sleep(0.06)
                    # Drain queued bursts with a bounded wait, then reconcile full state.
                    for _ in range(100):
                        try:
                            more = await asyncio.wait_for(reader.readline(), 0.001)
                            if not more:
                                raise Unavailable("compositor disconnected")
                        except asyncio.TimeoutError:
                            break
                    await self.refresh()
            except (OSError, ValueError, KeyError, Unavailable, asyncio.TimeoutError):
                self.error = "compositor unavailable; reconnecting"
                self.notify()
            finally:
                if writer:
                    writer.close()
                    await writer.wait_closed()
            await asyncio.sleep(delay)
            delay = min(30, delay * 2)
