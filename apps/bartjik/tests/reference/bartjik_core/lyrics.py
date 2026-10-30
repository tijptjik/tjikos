"""On-demand lyrics: optional Spotify, LRCLIB matches, then lyrics.ovh."""

import asyncio
from collections import OrderedDict
import json
import hashlib
import math
import re
import time
import unicodedata
from urllib.error import HTTPError
from urllib.parse import quote, urlencode
from urllib.request import Request, urlopen

from . import spotify_lyrics

MAX_RESPONSE = 512 * 1024
MAX_ENTRIES = 128


def track_key(track):
    if not isinstance(track, dict):
        raise ValueError("Invalid track")
    fields = []
    for name in ("artist", "title", "album"):
        value = track.get(name, "")
        if not isinstance(value, str) or len(value) > 1024:
            raise ValueError("Invalid track metadata")
        fields.append(value.strip())
    duration = track.get("duration", 0)
    if not isinstance(duration, (int, float)) or not math.isfinite(duration):
        duration = 0
    return (*fields, round(duration) if 0 < duration < 86400 else 0)


# LRC timestamps are seconds; positive offsets make the words appear earlier.
STAMP = re.compile(r"\[(\d+):([0-5]\d)(?:[.:](\d{1,3}))?\]")


def timed_lines(value):
    if not isinstance(value, str):
        return []
    offsets = re.findall(r"\[offset:([+-]?\d+)\]", value, flags=re.I)
    offset = int(offsets[-1]) / 1000 if offsets else 0
    rows = {}
    for raw in value.splitlines():
        tags = list(STAMP.finditer(raw))
        if not tags or raw[:tags[0].start()].strip():
            continue
        text = raw[tags[-1].end():].strip()
        # Enhanced LRC can also contain word timestamps; line highlighting
        # deliberately ignores those while retaining the actual words.
        text = re.sub(r"<\d+:[0-5]\d(?:[.:]\d{1,3})?>", "", text)
        for tag in tags:
            fraction = float("0." + tag[3]) if tag[3] else 0
            stamp = max(0, round(int(tag[1]) * 60 + int(tag[2]) + fraction - offset, 3))
            if stamp > 86400:
                continue
            texts = rows.setdefault(stamp, [])
            if text and text not in texts:
                texts.append(text)
    return [dict(time=stamp, text="\n".join(texts)) for stamp, texts in sorted(rows.items())]


def parse_result(data):
    if not isinstance(data, dict):
        raise ValueError("Invalid lyrics response")
    if data.get("instrumental") is True:
        return dict(status="instrumental", text="")
    plain = data.get("plainLyrics")
    if not isinstance(plain, str) or not plain.strip():
        synced = data.get("syncedLyrics")
        if isinstance(synced, str):
            # Display untimed lyrics when the provider only has LRC content.
            plain = re.sub(r"\[\d+:\d{2}(?:[.:]\d+)?\]", "", synced)
            plain = re.sub(r"(?m)^\[(?:ar|al|ti|au|length|by|offset|re|ve):[^\]\r\n]*\]\s*", "", plain)
    text = plain.strip() if isinstance(plain, str) else ""
    lines = timed_lines(data.get("syncedLyrics"))
    if not text and any(row["text"] for row in lines):
        text = "\n".join(row["text"] for row in lines)
    result = dict(status="ready" if text else "missing", text=text)
    if text and lines and any(row["text"] for row in lines):
        result["lines"] = lines
    return result


VERSION = re.compile(r"\b(?:remaster(?:ed)?|remix|mix|edit|version|live|mono|stereo|feat(?:uring)?|ft)\b", re.I)


def normalize(value):
    text = unicodedata.normalize("NFKD", value).casefold()
    return "".join(c for c in text if c.isalnum())


TIMING_VERSION = re.compile(r"\b(?:remaster(?:ed)?|mono|stereo|feat(?:uring)?|ft)\b", re.I)


def clean_title(title, timing=False):
    # Keep meaningful parentheses such as "(I Can't Get No) Satisfaction".
    marker = TIMING_VERSION if timing else VERSION

    def removable(value):
        return marker.search(value) and not (timing and re.search(r"\b(?:live|remix|mix|edit|version)\b", value, re.I))

    title = re.sub(r"\([^()]*\)|\[[^\[\]]*\]", lambda m: "" if removable(m[0]) else m[0], title)
    parts = re.split(r"\s+[-–—]\s+", title)
    while len(parts) > 1 and removable(parts[-1]):
        parts.pop()
    title = " - ".join(parts)
    title = re.sub(r"\s+(?:feat\.?|ft\.?|featuring)\s+.+$", "", title, flags=re.I)
    return " ".join(title.split())


def artist_matches(candidate, artist):
    names = [candidate] + re.split(r"\s*(?:;|,|\bfeat\.? |\bft\.? |\bfeaturing )\s*", candidate, flags=re.I)
    return normalize(artist) in {normalize(name) for name in names}


def track_result(record, key):
    result = parse_result(record)
    if "lines" in result:
        artist, title, album, duration = key
        length = record.get("duration")
        same_recording = (duration > 0 and isinstance(length, (int, float))
                          and math.isfinite(length) and abs(length - duration) <= 2
                          and isinstance(record.get("trackName"), str)
                          and normalize(clean_title(record["trackName"], timing=True)) == normalize(clean_title(title, timing=True))
                          and isinstance(record.get("artistName"), str)
                          and artist_matches(record["artistName"], artist))
        if not same_recording:
            result.pop("lines")
    return result


def best_match(records, key):
    if not isinstance(records, list):
        raise ValueError("Invalid lyrics search response")
    artist, title, album, duration = key
    matches = []
    for record in records:
        if not isinstance(record, dict):
            continue
        name, performer = record.get("trackName"), record.get("artistName")
        if not isinstance(name, str) or not isinstance(performer, str):
            continue
        if normalize(clean_title(name)) != normalize(clean_title(title)) or not artist_matches(performer, artist):
            continue
        result = track_result(record, key)
        # An instrumental flag on a different release must not hide vocal lyrics.
        if result["status"] != "ready":
            continue
        length = record.get("duration")
        difference = abs(length - duration) if duration and isinstance(length, (int, float)) and math.isfinite(length) else 86400
        score = (bool(result.get("lines")), normalize(name) == normalize(title), normalize(str(record.get("albumName", ""))) == normalize(album), -difference)
        matches.append((score, result))
    return max(matches, key=lambda match: match[0])[1] if matches else None


class LyricsCache:
    def __init__(self):
        self.spotify = spotify_lyrics.SpotifyLyrics()
        self.cache = OrderedDict()
        self.pending = {}
        self.slots = asyncio.Semaphore(2)

    async def get(self, track):
        key = track_key(track)
        if not key[0] or not key[1]:
            return dict(status="missing", text="")
        spotify_id = track.get("spotifyId", "")
        if not isinstance(spotify_id, str) or not spotify_lyrics.TRACK_ID.fullmatch(spotify_id):
            spotify_id = ""
        cookie = spotify_lyrics.read_cookie() if spotify_id else ""
        if spotify_id:
            key += (spotify_id, hashlib.sha256(cookie.encode()).hexdigest())
        cached = self.cache.get(key)
        if cached and cached[0] > time.monotonic():
            self.cache.move_to_end(key)
            return cached[1]
        if key not in self.pending:
            self.pending[key] = asyncio.create_task(self.fetch(key, cookie))
        return await asyncio.shield(self.pending[key])

    async def fetch(self, key, cookie=""):
        try:
            async with self.slots:
                try:
                    result = await asyncio.to_thread(self.download, key, cookie)
                except Exception:
                    result = dict(status="error", text="")
            ttl = 30 if result["status"] == "error" else 900 if result["status"] == "missing" else 21600
            ttl = min(ttl, result.pop("_retry", ttl))
            self.cache[key] = (time.monotonic() + ttl, result)
            self.cache.move_to_end(key)
            while len(self.cache) > MAX_ENTRIES:
                self.cache.popitem(last=False)
            return result
        finally:
            self.pending.pop(key, None)

    def request(self, url, deadline):
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise TimeoutError("Lyrics lookup deadline exceeded")
        request = Request(url, headers={
            "User-Agent": "bartjik/0.1 (desktop lyrics panel)", "Accept": "application/json"})
        try:
            with urlopen(request, timeout=min(4, remaining)) as response:
                raw = bytearray()
                while chunk := response.read1(min(65536, MAX_RESPONSE + 1 - len(raw))):
                    raw.extend(chunk)
                    if len(raw) > MAX_RESPONSE or time.monotonic() >= deadline:
                        raise ValueError("Lyrics response exceeds lookup limit")
        except HTTPError as error:
            error.close()
            if error.code == 404:
                return None
            raise
        return json.loads(raw)

    def download(self, key, cookie=""):
        spotify_failed = False
        if len(key) > 4 and cookie:
            try:
                result = self.spotify.get(key[4], cookie, time.monotonic() + 8)
                if result:
                    return result
            except Exception:
                spotify_failed = True
        key = key[:4]
        result = self.download_fallback(key, spotify_failed)
        if spotify_failed:
            result["_retry"] = 30
        return result

    def download_fallback(self, key, failed=False):
        artist, title, album, duration = key
        deadline = time.monotonic() + 17

        def request(url):
            nonlocal failed
            try:
                return self.request(url, deadline)
            except Exception:
                failed = True
                return None

        params = dict(artist_name=artist, track_name=title)
        if album:
            params["album_name"] = album
        if duration:
            params["duration"] = duration
        data = request("https://lrclib.net/api/get?" + urlencode(params))
        if data is not None:
            if isinstance(data, dict):
                result = track_result(data, key)
                if result["status"] in ("ready", "instrumental"):
                    return result
            else:
                failed = True

        # Album names and durations rank candidates, but do not reject the same
        # song on another release. Only matching artist/title pairs are accepted.
        titles = list(dict.fromkeys((title, clean_title(title))))
        for query in titles:
            data = request("https://lrclib.net/api/search?" + urlencode(dict(artist_name=artist, track_name=query)))
            if data is None:
                continue
            if not isinstance(data, list):
                failed = True
                continue
            result = best_match(data, key)
            if result:
                return result

        # This public API consults additional providers without an account/key.
        url = "https://api.lyrics.ovh/v1/" + quote(artist, safe="") + "/" + quote(clean_title(title), safe="")
        data = request(url)
        if data is not None:
            if not isinstance(data, dict) or not isinstance(data.get("lyrics"), str):
                failed = True
            elif data["lyrics"].strip():
                return dict(status="ready", text=data["lyrics"].strip(), source="lyrics.ovh")
        return dict(status="error" if failed else "missing", text="")

    async def close(self):
        await asyncio.gather(*list(self.pending.values()), return_exceptions=True)
