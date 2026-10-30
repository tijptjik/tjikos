"""Bounded, persistent album covers keyed by source URL, with shared downloads."""

import asyncio
import hashlib
import os
from pathlib import Path
import tempfile
import threading
import time
from urllib.parse import urlsplit
from urllib.request import Request, urlopen

MAX_IMAGE = 8 * 1024 * 1024
MAX_CACHE = 128 * 1024 * 1024
MAX_ENTRIES = 256


def raster(data):
    return (data.startswith(b"\x89PNG\r\n\x1a\n") or data.startswith(b"\xff\xd8\xff")
            or data.startswith((b"GIF87a", b"GIF89a"))
            or (data.startswith(b"RIFF") and data[8:12] == b"WEBP"))


class ArtworkCache:
    def __init__(self, directory=None):
        self.directory = Path(directory) if directory else Path(os.environ.get(
            "XDG_CACHE_HOME", str(Path.home() / ".cache"))) / "bartjik/artwork"
        self.pending = {}
        self.retry = {}
        self.slots = asyncio.Semaphore(2)
        self.lock = threading.Lock()

    async def get(self, url):
        if not isinstance(url, str) or len(url) > 8192:
            raise ValueError("Invalid artwork URL")
        parsed = urlsplit(url)
        if parsed.scheme not in ("http", "https") or not parsed.hostname or parsed.username or parsed.password:
            raise ValueError("Artwork requires an HTTP(S) URL")
        if self.retry.get(url, 0) > time.monotonic():
            raise ValueError("Artwork retry cooling down")
        if url not in self.pending:
            self.pending[url] = asyncio.create_task(self.fetch(url))
        return await asyncio.shield(self.pending[url])

    async def fetch(self, url):
        try:
            async with self.slots:
                return await asyncio.to_thread(self.download, url)
        except Exception:
            if len(self.retry) >= MAX_ENTRIES:
                self.retry.pop(next(iter(self.retry)))
            self.retry[url] = time.monotonic() + 60
            raise
        finally:
            self.pending.pop(url, None)

    def download(self, url):
        self.directory.mkdir(mode=0o700, parents=True, exist_ok=True)
        destination = self.directory / (hashlib.sha256(url.encode()).hexdigest() + ".img")
        with self.lock:
            if destination.is_file() and 0 < destination.stat().st_size <= MAX_IMAGE:
                with destination.open("rb") as cached:
                    if raster(cached.read(16)):
                        destination.touch()
                        return destination.as_uri()
        temporary = None
        try:
            started = time.monotonic()
            request = Request(url, headers={"User-Agent": "bartjik/0.1", "Accept": "image/*"})
            with urlopen(request, timeout=8) as response:
                if urlsplit(response.url).scheme not in ("http", "https"):
                    raise ValueError("Invalid artwork redirect")
                size = response.headers.get("Content-Length")
                if size and int(size) > MAX_IMAGE:
                    raise ValueError("Artwork exceeds size limit")
                with tempfile.NamedTemporaryFile(dir=self.directory, prefix=".download-", delete=False) as output:
                    temporary = Path(output.name)
                    total = 0
                    header = b""
                    while chunk := response.read(65536):
                        total += len(chunk)
                        if total > MAX_IMAGE or time.monotonic() - started > 20:
                            raise ValueError("Artwork exceeds download limit")
                        header = (header + chunk)[:16]
                        output.write(chunk)
                    if not raster(header):
                        raise ValueError("Unsupported artwork image")
            with self.lock:
                temporary.replace(destination)
                self.prune(destination)
            return destination.as_uri()
        finally:
            if temporary:
                temporary.unlink(missing_ok=True)

    def prune(self, keep):
        entries = sorted(self.directory.glob("*.img"), key=lambda p: p.stat().st_mtime)
        total = sum(p.stat().st_size for p in entries)
        count = len(entries)
        for path in entries:
            if total <= MAX_CACHE and count <= MAX_ENTRIES:
                break
            if path == keep:
                continue
            total -= path.stat().st_size
            path.unlink()
            count -= 1

    async def close(self):
        # Downloads have size/time limits; let atomic writes finish before exit.
        await asyncio.gather(*list(self.pending.values()), return_exceptions=True)
