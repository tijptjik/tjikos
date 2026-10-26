"""Bounded, cancellable subprocess and HTTP primitives. Never echo stderr or secrets."""

import asyncio
import json
import os
import signal
from urllib.request import Request, urlopen
from urllib.error import HTTPError


class Unavailable(RuntimeError):
    pass


class RateLimit(Unavailable):
    def __init__(self, delay=60):
        super().__init__("HTTP 429; rate limited")
        self.delay = max(1, min(3600, delay))


async def stop(process):
    if process.returncode is None:
        try:
            os.killpg(process.pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
        try:
            await asyncio.wait_for(process.wait(), 0.5)
        except asyncio.TimeoutError:
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            await process.wait()


async def command(args, timeout=5, limit=262144, env=None, input_text=None):
    try:
        p = await asyncio.create_subprocess_exec(
            *args,
            stdin=asyncio.subprocess.PIPE if input_text is not None else asyncio.subprocess.DEVNULL,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.DEVNULL,
            start_new_session=True,
            env=env,
            limit=limit + 1,
        )
    except OSError:
        raise Unavailable(f"{os.path.basename(args[0])} unavailable") from None

    async def collect():
        if input_text is not None:
            p.stdin.write(input_text.encode("utf-8"))
            await p.stdin.drain()
            p.stdin.close()
        chunks = []
        size = 0
        while chunk := await p.stdout.read(16384):
            size += len(chunk)
            if size > limit:
                raise Unavailable("provider output limit exceeded")
            chunks.append(chunk)
        if await p.wait():
            raise Unavailable(
                f"{os.path.basename(args[0])} failed (exit {p.returncode})"
            )
        return b"".join(chunks).decode("utf-8", "replace").strip()

    try:
        return await asyncio.wait_for(collect(), timeout)
    except asyncio.TimeoutError:
        raise Unavailable(f"{os.path.basename(args[0])} timed out") from None
    finally:
        await stop(p)


async def json_command(args, **kwargs):
    try:
        return json.loads(await command(args, **kwargs))
    except ValueError:
        raise Unavailable("malformed JSON response") from None


def http_json(url, headers=None):
    try:
        with urlopen(Request(url, headers=headers or {}), timeout=8) as response:
            data = response.read(1048577)
            if len(data) > 1048576:
                raise Unavailable("API output limit exceeded")
            return json.loads(data)
    except HTTPError as e:
        if e.code == 429:
            try:
                delay = float(e.headers.get("Retry-After", "60"))
            except ValueError:
                delay = 60
            raise RateLimit(delay) from None
        raise Unavailable(f"HTTP {e.code}") from None
    except (OSError, ValueError):
        raise Unavailable("API unavailable or malformed response") from None
