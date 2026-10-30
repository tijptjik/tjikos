"""One critical warning per low-battery episode, shared by all bar workers."""

import asyncio
import json
import math
import os
from pathlib import Path
import sys
import time

from .io import command, Unavailable


class CriticalBatteryAlert:
    def __init__(self, state_path=None):
        self.path = Path(state_path) if state_path is not None else (
            Path(os.environ.get("XDG_STATE_HOME", Path.home() / ".local/state"))
            / "bartjik/battery-alert.json"
        )
        self.alerted = False
        try:
            self.alerted = json.loads(self.path.read_text()).get("alerted") is True
        except (OSError, ValueError, AttributeError):
            pass
        self.retry_at = 0
        self.lock = asyncio.Lock()

    def save(self):
        try:
            self.path.parent.mkdir(parents=True, exist_ok=True)
            temporary = self.path.with_suffix(".tmp")
            temporary.write_text(json.dumps({"alerted": self.alerted}) + "\n")
            temporary.replace(self.path)
        except OSError:
            # Keep in-memory deduplication even if persistent state is unavailable.
            print("Battery alert state could not be saved", file=sys.stderr)

    async def update(self, rows):
        battery = next((row.get("battery") for row in rows if row.get("battery")), None)
        if not battery:
            return
        percent = battery.get("percent")
        if not isinstance(percent, (int, float)) or not math.isfinite(percent) or not 0 <= percent <= 100:
            return
        async with self.lock:
            if battery.get("external") is True or percent >= 7:
                # Hysteresis prevents a noisy reading around 5% from spamming.
                if self.alerted:
                    self.alerted = False
                    self.save()
                self.retry_at = 0
                return
            if percent > 5 or self.alerted or time.monotonic() < self.retry_at:
                return
            try:
                await command([
                    "notjikctl", "send", "Battery critically low",
                    f"{math.ceil(percent)}% remaining. Connect to power now.",
                    "--app", "Bartjik", "--id", "battery-critical",
                    "--urgency", "critical", "--timeout", "never",
                    "--icon", "battery-caution-symbolic",
                ])
            except Unavailable:
                self.retry_at = time.monotonic() + 30
                print("Critical battery notification unavailable; retrying in 30 seconds", file=sys.stderr)
                return
            self.alerted = True
            self.save()
