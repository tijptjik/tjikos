"""On-demand public 24h history, deduplicated with bounded cache and retry backoff."""

import asyncio
import math
import json
from pathlib import Path
import re
import time
from urllib.parse import urlencode
from .io import http_json, Unavailable, RateLimit

COINS = {"BTC": "bitcoin", "SOL": "solana", "AR": "arweave"}


def normalize(data):
    values = data.get("prices") if isinstance(data, dict) else None
    if not isinstance(values, list) or not 2 <= len(values) <= 2048:
        raise Unavailable("Invalid price history")
    rows = {}
    for row in values:
        if not isinstance(row, list) or len(row) != 2:
            raise Unavailable("Invalid history point")
        stamp, price = row
        if (
            any(
                isinstance(v, bool)
                or not isinstance(v, (int, float))
                or not math.isfinite(v)
                for v in row
            )
            or stamp <= 0
            or price < 0
        ):
            raise Unavailable("Invalid history value")
        rows[stamp] = price
    points = sorted(rows.items())
    if len(points) < 2:
        raise Unavailable("Insufficient price history")
    end = points[-1][0]
    return [[t, p] for t, p in points if t >= end - 86400000]


class History:
    def __init__(self, cache_path=None):
        self.cache = {}
        self.tasks = {}
        self.retry = {}
        self.attempts = {}
        self.path = Path(cache_path) if cache_path else None
        if self.path:
            try:
                if self.path.stat().st_size > 4_000_000:
                    return
                rows = json.loads(self.path.read_text())
                for row in rows[-64:]:
                    key = tuple(row["key"])
                    if len(key) != 2 or not all(isinstance(v, str) for v in key):
                        continue
                    attempted = float(row["attempted"])
                    if not math.isfinite(attempted) or attempted > time.time() + 60:
                        continue
                    cached = row.get("chart")
                    if cached:
                        points = normalize({"prices": cached["points"]})
                        updated = float(cached["updated"])
                        if not math.isfinite(updated) or updated > time.time() + 60:
                            continue
                        self.cache[key] = dict(points=points,updated=updated,source="CoinGecko",error="",stale=False)
                    self.attempts[key] = attempted
                    if row.get("retry"):
                        retry_at, message = row["retry"]
                        if isinstance(retry_at, (int,float)) and math.isfinite(retry_at) and isinstance(message,str):
                            self.retry[key] = (retry_at,message)
            except (OSError, ValueError, TypeError, KeyError, Unavailable):
                pass

    def save(self):
        if not self.path:
            return
        try:
            rows = [dict(key=key, attempted=value, chart=self.cache.get(key), retry=self.retry.get(key))
                    for key,value in list(self.attempts.items())[-64:]]
            self.path.parent.mkdir(parents=True, exist_ok=True)
            temporary = self.path.with_suffix(".tmp")
            temporary.write_text(json.dumps(rows))
            temporary.replace(self.path)
        except OSError:
            pass  # A read-only cache must not prevent the chart from working.

    async def get(self, coin, currency):
        if (
            not isinstance(coin, str)
            or not re.fullmatch("[a-z0-9-]{1,80}", coin)
            or not re.fullmatch("[a-z]{2,10}", currency)
        ):
            raise Unavailable("Configure a valid chartId for this asset")
        key = (coin, currency)
        now = time.time()
        cached = self.cache.get(key)
        if cached and now - cached["updated"] < 60:
            return cached
        if key in self.retry and now < self.retry[key][0]:
            return {
                **(cached or {}),
                "error": self.retry[key][1],
                "stale": bool(cached),
            }
        if key in self.tasks:
            return await asyncio.shield(self.tasks[key])
        if now - self.attempts.get(key, 0) < 60:
            return {**(cached or {}), "stale": bool(cached), "error": "History request cooling down" if not cached else ""}
        if key not in self.tasks:
            if len(self.tasks) >= 8:
                raise Unavailable("History request queue full")
            self.attempts[key] = now
            if len(self.attempts)>64:
                self.attempts.pop(next(iter(self.attempts)))
            self.save()
            self.tasks[key] = asyncio.create_task(self.fetch(key))
        return await asyncio.shield(self.tasks[key])

    async def fetch(self, key):
        try:
            coin, currency = key
            data = await asyncio.to_thread(
                http_json,
                "https://api.coingecko.com/api/v3/coins/"
                + coin
                + "/market_chart?"
                + urlencode(dict(vs_currency=currency, days=1)),
                {"Accept": "application/json", "User-Agent": "bartjik/0.1"},
            )
            result = dict(
                points=normalize(data),
                updated=time.time(),
                source="CoinGecko",
                error="",
                stale=False,
            )
            if len(self.cache) >= 64:
                self.cache.pop(next(iter(self.cache)))
            self.cache[key] = result
            self.retry.pop(key, None)
            return result
        except (Unavailable, ValueError, TypeError) as error:
            delay = max(60, error.delay) if isinstance(error, RateLimit) else 60
            message = (
                str(error)
                if isinstance(error, Unavailable)
                else "Invalid history response"
            )
            if len(self.retry) >= 64:
                self.retry.pop(next(iter(self.retry)))
            self.retry[key] = (time.time() + delay, message)
            return {
                **self.cache.get(key, {}),
                "error": message,
                "stale": key in self.cache,
            }
        finally:
            self.save()
            self.tasks.pop(key, None)

    async def close(self):
        tasks = list(self.tasks.values())
        for task in tasks:
            task.cancel()
        await asyncio.gather(*tasks, return_exceptions=True)
