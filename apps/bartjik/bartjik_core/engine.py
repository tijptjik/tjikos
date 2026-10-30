"""One task per distinct provider configuration, transactional live reload."""

import asyncio
import copy
import json
import os
from pathlib import Path
import sys
import time
from . import config, providers, workspaces, demo, pointer, history, artwork, lyrics, connectivity
from .io import Unavailable, RateLimit, stop
from .model import item, validate_items, untrusted_items
from .actions import Executor, choose
from .battery_alert import CriticalBatteryAlert


class Worker:
    def __init__(self, spec, notify, battery_alert=None):
        self.spec = spec
        self.notify = notify
        self.battery_alert = battery_alert
        self.items = []
        self.error = "starting"
        self.updated = None
        self.failures = 0
        self.polls = 0
        self.task = None
        self.wake = asyncio.Event()

    def start(self):
        self.task = asyncio.create_task(self.run())

    async def close(self):
        if self.task:
            self.task.cancel()
            await asyncio.gather(self.task, return_exceptions=True)

    def status(self):
        rows = copy.deepcopy(self.items)
        if self.error:
            if not rows:
                rows = [
                    item(
                        "unavailable",
                        "—",
                        state={"unavailable": True},
                        tooltip=self.error,
                    )
                ]
            for r in rows:
                r.setdefault("state", {})["stale"] = self.updated is not None
                r["tooltip"] = (r.get("tooltip", "") + "\n" + self.error).strip()
        return dict(
            items=rows,
            error=self.error,
            updated=self.updated,
            polls=self.polls,
            failures=self.failures,
            state="stale"
            if self.error and self.updated
            else "unavailable"
            if self.error
            else "ready",
        )

    def success(self, rows):
        self.items = validate_items(rows)
        self.error = ""
        self.updated = time.time()
        self.failures = 0
        self.notify()

    async def stream(self):
        m = self.spec
        limit = m.get("maxOutput", 262144)
        try:
            p = await asyncio.create_subprocess_exec(
                *m["command"],
                stdout=asyncio.subprocess.PIPE,
                stderr=asyncio.subprocess.DEVNULL,
                start_new_session=True,
                limit=limit,
            )
        except OSError:
            raise Unavailable("stream executable unavailable") from None
        try:
            while True:
                try:
                    raw = await asyncio.wait_for(
                        p.stdout.readline(), m.get("timeout", 30)
                    )
                except asyncio.TimeoutError:
                    raise Unavailable("stream heartbeat timed out") from None
                if not raw:
                    raise Unavailable("stream exited; reconnecting")
                if len(raw) > limit:
                    raise Unavailable("stream output limit exceeded")
                self.success(untrusted_items(json.loads(raw)))
        finally:
            await stop(p)

    async def run(self):
        while True:
            delay = self.spec.get(
                "interval", providers.INTERVALS.get(self.spec["provider"], 5)
            )
            try:
                self.polls += 1
                if (
                    self.spec["provider"] == "process"
                    and self.spec.get("mode") == "stream"
                ):
                    await self.stream()
                else:
                    self.success(
                        await providers.REGISTRY[self.spec["provider"]](self.spec)
                    )
                    if self.spec["provider"] == "battery" and self.battery_alert:
                        await self.battery_alert.update(self.items)
            except Exception as e:
                self.failures += 1
                # API/process payloads and exception reprs may contain private data.
                self.error = (
                    str(e)
                    if isinstance(e, Unavailable)
                    else f"{type(e).__name__}: invalid provider response"
                )
                delay = max(delay, min(300, 2 ** min(self.failures, 8)))
                if isinstance(e, RateLimit):
                    delay = max(delay, e.delay)
                self.notify()
            try:
                await asyncio.wait_for(self.wake.wait(), delay)
            except asyncio.TimeoutError:
                pass
            self.wake.clear()


class Engine:
    def __init__(self, emit, demo_mode=False, history_path=None):
        self.emit = emit
        self.demo = demo_mode
        self.battery_alert = None if demo_mode else CriticalBatteryAlert()
        self.config = None
        self.bars = []
        self.screens = []
        self.workers = {}
        self.error = ""
        self.action_error = ""
        self.panel_feedback = {}
        self.executor = Executor()
        self.watcher = None
        self.pending = False
        self.actions = set()
        self.current_serial = None
        self.pointer = None
        self.history = history.History(history_path)
        self.artwork = artwork.ArtworkCache()
        self.lyrics = lyrics.LyricsCache()

    def changed(self):
        if not self.pending:
            self.pending = True
            asyncio.get_running_loop().call_later(0.03, self.publish)

    def publish(self):
        self.pending = False
        if self.config is None:
            self.emit(dict(error=self.error, ready=False))
            return
        states = {k: w.status() for k, w in self.workers.items()}
        for b in self.bars:
            for mid, m in b["moduleConfigs"].items():
                key = b["sources"][mid]
                if self.demo:
                    states[key] = dict(
                        items=demo.items(m["provider"], m), state="demo", error=""
                    )
                elif m["provider"] == "workspaces":
                    rows = workspaces.view(
                        self.watcher.data if self.watcher else {}, b["screen"], m
                    )
                    err = (
                        self.watcher.error if self.watcher else "compositor unavailable"
                    )
                    if err:
                        for r in rows:
                            r["state"]["stale"] = True
                    states[key] = dict(
                        items=rows,
                        state="stale"
                        if rows and err
                        else "unavailable"
                        if err
                        else "ready",
                        error=err,
                    )
        self.emit(
            dict(
                ready=True,
                config=self.config,
                bars=self.bars,
                providers=states,
                error=self.error,
                actionError=self.action_error,
                panelFeedback=self.panel_feedback,
                demo=self.demo,
                compositor=self.watcher.data if self.watcher else {},
            )
        )

    async def reload(self, force=False):
        try:
            cfg = config.load()
            # Preserve original style overrides for a monitor-specific named theme.
            p = config.path()
            raw = json.loads(p.read_text()) if p.exists() else {}
            cfg["_styleOverrides"] = raw.get("style", {})
            bars = [
                b for i, s in enumerate(self.screens) for b in config.resolve(cfg, s, i)
            ]
            serial = json.dumps([cfg, self.screens], sort_keys=True)
            if not force and serial == self.current_serial:
                if self.error:
                    self.error = ""
                    self.changed()
                return
            needed = {}
            need_workspaces = False
            for b in bars:
                used = {mid for island in b["islands"] for mid in island["modules"]}
                b["moduleConfigs"] = {
                    mid: m for mid, m in b["moduleConfigs"].items() if mid in used
                }
                b["sources"] = {}
                for mid, m in b["moduleConfigs"].items():
                    key = config.provider_key(m)
                    if m["provider"] == "workspaces":
                        key += "@" + b["screen"]
                        need_workspaces = True
                    b["sources"][mid] = key
                    if not self.demo and m["provider"] not in config.NATIVE | {
                        "workspaces"
                    }:
                        needed[key] = config.provider_config(m)
            for key in set(self.workers) - set(needed):
                await self.workers.pop(key).close()
            if self.watcher and (self.demo or not need_workspaces):
                await self.watcher.close()
                self.watcher = None
            if need_workspaces and not self.demo and not self.watcher:
                self.watcher = workspaces.Watcher(self.changed)
                self.watcher.start()
            if self.watcher:
                # Viewport gestures can move columns without a workspace/focus event.
                self.watcher.track_geometry = any(
                    m["provider"] == "workspaces" and m.get("offscreen", False)
                    for b in bars for m in b["moduleConfigs"].values()
                )
            for key, spec in needed.items():
                if key not in self.workers:
                    self.workers[key] = Worker(spec, self.changed, self.battery_alert)
                    self.workers[key].start()
            need_pointer = any(
                b["autoHide"]
                or any(
                    m.get("panel", {}).get("hover")
                    or (m["provider"] == "workspaces" and m.get("preview", {}).get("enabled"))
                    for m in b["moduleConfigs"].values()
                )
                for b in bars
            )
            if self.pointer and not need_pointer:
                await self.pointer.close()
                self.pointer = None
            if need_pointer and not self.pointer:
                self.pointer = pointer.Observer(self.emit)
            await self.executor.close()
            self.config = cfg
            self.bars = bars
            self.error = ""
            self.current_serial = serial
            self.changed()
        except Exception as e:
            self.error = (
                str(e)
                if isinstance(e, (ValueError, FileNotFoundError))
                else type(e).__name__ + " while loading config"
            )
            self.changed()

    async def action(self, msg):
        if self.demo:
            return
        try:
            context = msg["context"]
            b = next(b for b in self.bars if b["key"] == context["barKey"])
            if context["monitor"] != b["screen"]:
                raise ValueError("action monitor mismatch")
            mid = context["module"]
            m = b["moduleConfigs"][mid]
            key = b["sources"][mid]
            if m["provider"] == "workspaces":
                rows = workspaces.view(self.watcher.data, b["screen"], m)
            elif m["provider"] in config.NATIVE:
                rows = [dict(id=context["item"])]
            else:
                rows = self.workers[key].items
            row = next(r for r in rows if r["id"] == context["item"])
            action = choose(m, row, context["part"], context["event"])
            await self.executor.execute(action, context, m, row, rows)
            self.action_error = ""
            if key in self.workers:
                self.workers[key].wake.set()
            if m["provider"] == "workspaces":
                await self.watcher.refresh()
        except Exception as e:
            self.action_error = (
                str(e)
                if isinstance(e, Unavailable)
                else type(e).__name__ + " while executing action"
            )
        self.changed()

    async def panel_action(self, msg):
        if self.demo:
            return
        feedback = {}
        try:
            context = msg["context"]
            b = next(b for b in self.bars if b["key"] == context["barKey"])
            m = b["moduleConfigs"][context["module"]]
            provider = m["provider"]
            if provider not in ("locale", "mullvad", "tailscale", "battery") or b["screen"] != context["monitor"]:
                raise ValueError("invalid panel action")
            feedback = dict(barKey=b["key"], module=context["module"], busy=True, message="", error=False)
            self.panel_feedback = feedback
            self.changed()
            if provider == "battery":
                from .battery import select_profile
                message = await select_profile(context.get("action"), context.get("selection"), self.executor)
            elif provider == "locale":
                from .io import command
                from .details import input_methods
                async with self.executor.lock("locale"):
                    group = await command(["fcitx5-remote", "--check", "-q"])
                    methods = await asyncio.to_thread(input_methods, group)
                    if context.get("selection") not in methods:
                        raise ValueError("method no longer in active group")
                    await command(["fcitx5-remote", "--check", "-s", context["selection"]])
                message = "Input method selected"
            elif context.get("action") == "refresh":
                message = "Refreshing"
            else:
                message = await connectivity.act(provider, context.get("action"), context.get("selection"), m, self.executor)
            self.workers[b["sources"][context["module"]]].wake.set()
            self.action_error = ""
            self.panel_feedback = dict(feedback, busy=False, message=message)
        except Exception as e:
            self.action_error = str(e) if isinstance(e, Unavailable) else "This control is currently unavailable"
            self.panel_feedback = dict(feedback, busy=False, error=True, message=self.action_error)
        self.changed()

    async def chart(self, msg):
        token = msg.get("token", "")
        try:
            context = msg["context"]
            bar = next(b for b in self.bars if b["key"] == context["barKey"])
            module = bar["moduleConfigs"][context["module"]]
            if module["provider"] != "crypto" or context["monitor"] != bar["screen"]:
                raise ValueError("Invalid chart context")
            currency = module.get("currency", "USD").upper()
            asset = next(
                a
                for a in module.get("assets", providers.ASSETS)
                if a["symbol"].upper() + "/" + currency == context["item"]
            )
            if self.demo:
                import math

                end = time.time() * 1000
                base = {"BTC": 67432, "SOL": 148, "AR": 18.4}.get(
                    asset["symbol"].upper(), 100
                )
                result = dict(
                    points=[
                        [
                            end - (287 - i) * 300000,
                            base * (1 + math.sin(i / 22) * 0.03 + i / 10000),
                        ]
                        for i in range(288)
                    ],
                    updated=time.time(),
                    source="Synthetic demo",
                    error="",
                    stale=False,
                )
            else:
                coin = asset.get(
                    "chartId", history.COINS.get(asset["symbol"].upper(), "")
                )
                cached = self.history.cache.get((coin, currency.lower()))
                if cached:
                    self.emit(dict(kind="crypto",token=token,chart={**cached,"loading":True}))
                result = await self.history.get(coin, currency.lower())
        except (ValueError, KeyError, StopIteration, Unavailable):
            result = dict(error="Chart unavailable: check asset chartId and currency")
        self.emit(dict(kind="crypto", token=token, chart=result))

    async def cover(self, msg):
        source = ""
        try:
            source = await self.artwork.get(msg.get("url", ""))
        except Exception:
            pass  # Keep the previous cover; never expose private URLs in logs.
        self.emit(dict(kind="artwork", token=msg.get("token", ""), source=source))

    async def lyric_text(self, msg):
        try:
            if self.demo:
                result = dict(status="ready", text="Evening light across the water\nQuiet footsteps on the quay\n\nWindows glow along the harbour\nHome is where the lights will be\n\nLet the city settle slowly\nLet the last boat cross the bay\n\nEvery little light a story\nEvery wave will find its way")
            else:
                result = await self.lyrics.get(msg.get("track"))
        except Exception:
            result = dict(status="error", text="")
        self.emit(dict(kind="lyrics", token=msg.get("token", ""), lyrics=result))

    async def message(self, msg):
        if msg.get("op") == "screens":
            self.screens = msg["screens"]
            await self.reload()
        elif msg.get("op") == "reload":
            await self.reload(True)
        elif msg.get("op") == "externalLeave":
            c = msg.get("context", {})
            bar = next((b for b in self.bars if b["key"] == c.get("barKey") and b["screen"] == c.get("monitor")), None)
            module = bar["moduleConfigs"].get(c.get("module"), {}) if bar else {}
            if module.get("provider") in ("audio", "network"):
                self.executor.leave_panel(module["provider"])
        elif msg.get("op") == "closeExternalPanels":
            await self.executor.close_hover_panels()
        elif msg.get("op") == "panelAction":
            if len(self.actions) < 32:
                task = asyncio.create_task(self.panel_action(msg))
                self.actions.add(task)
                task.add_done_callback(self.actions.discard)
        elif msg.get("op") == "lyrics":
            if len(self.actions) < 32:
                task = asyncio.create_task(self.lyric_text(msg))
                self.actions.add(task)
                task.add_done_callback(self.actions.discard)
            else:
                self.emit(dict(kind="lyrics", token=msg.get("token", ""), lyrics=dict(status="error", text="")))
        elif msg.get("op") == "artwork":
            if len(self.actions) < 32:
                task = asyncio.create_task(self.cover(msg))
                self.actions.add(task)
                task.add_done_callback(self.actions.discard)
            else:
                self.emit(dict(kind="artwork", token=msg.get("token", ""), source=""))
        elif msg.get("op") == "crypto":
            if len(self.actions) < 32:
                task = asyncio.create_task(self.chart(msg))
                self.actions.add(task)
                task.add_done_callback(self.actions.discard)
        elif msg.get("op") == "action":
            if len(self.actions) >= 32:
                self.action_error = "action queue full"
                self.changed()
                return
            task = asyncio.create_task(self.action(msg))
            self.actions.add(task)
            task.add_done_callback(self.actions.discard)

    async def close(self):
        pending = list(self.actions)
        for task in pending:
            task.cancel()
        await asyncio.gather(*pending, return_exceptions=True)
        await self.executor.close()
        await self.history.close()
        await self.artwork.close()
        await self.lyrics.close()
        if self.pointer:
            await self.pointer.close()
        await asyncio.gather(*(w.close() for w in self.workers.values()))
        if self.watcher:
            await self.watcher.close()


async def serve(demo_mode=False):
    def emit(value):
        print(json.dumps(value, ensure_ascii=False, separators=(",", ":")), flush=True)

    cache = Path(os.environ.get("XDG_CACHE_HOME", str(Path.home()/".cache")))/"bartjik"/"crypto-history.json"
    engine = Engine(emit, demo_mode, None if demo_mode else cache)
    reader = asyncio.StreamReader(limit=1048576)
    protocol = asyncio.StreamReaderProtocol(reader)
    await asyncio.get_running_loop().connect_read_pipe(lambda: protocol, sys.stdin)

    async def watch():
        while True:
            await engine.reload()
            await asyncio.sleep(1)

    task = asyncio.create_task(watch())
    try:
        while raw := await reader.readline():
            try:
                await engine.message(json.loads(raw))
            except (ValueError, KeyError):
                pass
    finally:
        task.cancel()
        await asyncio.gather(task, return_exceptions=True)
        await engine.close()
