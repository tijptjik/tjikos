"""One non-intercepting pointer observer for all auto-hiding surfaces.

Query Hyprland's command socket directly: no subprocess per sample. Monitor
geometry is refreshed once a second. Failure makes the bars visible again.
"""

import asyncio
import json
import os
from pathlib import Path


async def query(command):
    sock = (
        Path(os.environ["XDG_RUNTIME_DIR"])
        / "hypr"
        / os.environ["HYPRLAND_INSTANCE_SIGNATURE"]
        / ".socket.sock"
    )
    writer = None
    try:
        async with asyncio.timeout(0.5):
            reader, writer = await asyncio.open_unix_connection(str(sock), limit=65536)
            writer.write(("j/" + command).encode())
            await writer.drain()
            data = bytearray()
            while chunk := await reader.read(8192):
                data.extend(chunk)
                if len(data) > 65536:
                    raise ValueError("pointer response too large")
            return json.loads(data)
    finally:
        if writer:
            writer.close()
            await writer.wait_closed()


def local_position(point, monitors):
    x, y = float(point["x"]), float(point["y"])
    for m in monitors:
        scale = m["scale"]
        w, h = m["width"] / scale, m["height"] / scale
        if m.get("transform", 0) % 2:
            w, h = h, w
        if m["x"] <= x < m["x"] + w and m["y"] <= y < m["y"] + h:
            return dict(valid=True, monitor=m["name"], x=x - m["x"], y=y - m["y"])
    return dict(valid=True, monitor="", x=0, y=0)


class Observer:
    def __init__(self, emit):
        self.emit = emit
        self.task = asyncio.create_task(self.run())

    async def close(self):
        self.task.cancel()
        await asyncio.gather(self.task, return_exceptions=True)

    async def run(self):
        monitors, refresh = [], 0
        while True:
            delay = 0.05
            try:
                now = asyncio.get_running_loop().time()
                if now >= refresh:
                    monitors = await query("monitors")
                    refresh = now + 1
                point = local_position(await query("cursorpos"), monitors)
            except (
                OSError,
                KeyError,
                ValueError,
                TypeError,
                TimeoutError,
                ZeroDivisionError,
            ):
                point = dict(
                    valid=False,
                    error="Pointer observer unavailable; auto-hide suspended",
                )
                refresh, delay = 0, 1
            self.emit(dict(kind="pointer", pointer=point))
            await asyncio.sleep(delay)
