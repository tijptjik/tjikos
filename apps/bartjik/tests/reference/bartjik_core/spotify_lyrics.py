"""Optional Spotify lyrics. Credentials stay local and never reach QML or logs."""

import getpass
import hashlib
import hmac
import json
import math
import os
from pathlib import Path
import re
import stat
import tempfile
import threading
import time
import warnings
from urllib.error import HTTPError
from urllib.parse import urlencode
from urllib.request import HTTPRedirectHandler, Request, build_opener

# Public Web Player protocol data, not an account credential. Parse JSON only.
SECRET_URL = "https://code.thetadev.de/ThetaDev/spotify-secrets/raw/branch/main/secrets/secretDict.json"
TOKEN_URL = "https://open.spotify.com/api/token"
TIME_URL = "https://open.spotify.com/api/server-time"
LYRICS_URL = "https://spclient.wg.spotify.com/color-lyrics/v2/track/"
MAX_RESPONSE = 512 * 1024
TRACK_ID = re.compile(r"[A-Za-z0-9]{22}\Z")


def credential_path():
    return Path(os.environ.get("XDG_CONFIG_HOME", str(Path.home() / ".config"))) / "bartjik" / "spotify-sp-dc"


def valid_cookie(value):
    return isinstance(value, str) and bool(re.fullmatch(r"[A-Za-z0-9_%+./=~-]{1,4096}", value))


def read_cookie():
    try:
        fd = os.open(credential_path(), os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
        with os.fdopen(fd) as source:
            info = os.fstat(source.fileno())
            if not stat.S_ISREG(info.st_mode) or info.st_uid != os.getuid() or info.st_mode & 0o077:
                return ""
            value = source.read(4097).strip()
            return value if valid_cookie(value) else ""
    except (OSError, UnicodeError):
        return ""


def configure(action):
    path = credential_path()
    if action == "login":
        # No argument, environment variable or echoed stdin containing the cookie.
        with warnings.catch_warnings():
            warnings.simplefilter("error", getpass.GetPassWarning)
            try:
                value = getpass.getpass("Spotify sp_dc cookie (hidden): ").strip()
            except (getpass.GetPassWarning, EOFError, KeyboardInterrupt):
                raise ValueError("Run spotify login in an interactive terminal") from None
        if not valid_cookie(value):
            raise ValueError("Enter only the sp_dc cookie value")
        path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
        fd, name = tempfile.mkstemp(prefix=".spotify-sp-dc-", dir=path.parent)
        try:
            with os.fdopen(fd, "w") as target:
                target.write(value + "\n")
            os.replace(name, path)
        finally:
            Path(name).unlink(missing_ok=True)
    elif action == "logout":
        path.unlink(missing_ok=True)
    return dict(ok=True, configured=bool(read_cookie()), path=str(path))


def totp(dictionary, timestamp):
    if not isinstance(dictionary, dict):
        raise ValueError("Invalid Spotify authentication data")
    versions = [int(key) for key in dictionary if re.fullmatch(r"\d{1,6}", str(key))]
    if not versions:
        raise ValueError("Missing Spotify authentication version")
    version = str(max(versions))
    values = dictionary[version]
    if not isinstance(values, list) or not 1 <= len(values) <= 256 or any(type(v) is not int or not 0 <= v <= 255 for v in values):
        raise ValueError("Invalid Spotify authentication data")
    key = "".join(str(v ^ ((i % 33) + 9)) for i, v in enumerate(values)).encode()
    digest = hmac.new(key, (int(timestamp) // 30).to_bytes(8, "big"), hashlib.sha1).digest()
    offset = digest[-1] & 15
    number = int.from_bytes(digest[offset:offset + 4], "big") & 0x7fffffff
    return version, f"{number % 1000000:06d}"


class NoRedirect(HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        # Cookie and bearer headers must never follow redirects to another host.
        return None


def request(url, deadline, headers=None):
    remaining = deadline - time.monotonic()
    if remaining <= 0:
        raise TimeoutError("Spotify lookup deadline exceeded")
    req = Request(url, headers={"User-Agent": "Mozilla/5.0", "Accept": "application/json", **(headers or {})})
    with build_opener(NoRedirect()).open(req, timeout=min(3, remaining)) as response:
        raw = bytearray()
        while chunk := response.read1(min(65536, MAX_RESPONSE + 1 - len(raw))):
            raw.extend(chunk)
            if len(raw) > MAX_RESPONSE or time.monotonic() >= deadline:
                raise ValueError("Spotify response exceeds lookup limit")
    return json.loads(raw)


def parse_lyrics(data):
    if not isinstance(data, dict) or not isinstance(data.get("lyrics"), dict):
        raise ValueError("Invalid Spotify lyrics response")
    lyrics = data["lyrics"]
    rows = lyrics.get("lines")
    if not isinstance(rows, list):
        raise ValueError("Invalid Spotify lyrics lines")
    texts, timed = [], {}
    synced = lyrics.get("syncType") == "LINE_SYNCED"
    for row in rows:
        if not isinstance(row, dict) or not isinstance(row.get("words"), str):
            raise ValueError("Invalid Spotify lyrics line")
        text = row["words"].strip()
        texts.append(text)
        try:
            stamp = float(row.get("startTimeMs", "")) / 1000
            if not math.isfinite(stamp) or not 0 <= stamp <= 86400:
                raise ValueError()
            simultaneous = timed.setdefault(stamp, [])
            if text and text not in simultaneous:
                simultaneous.append(text)
        except (TypeError, ValueError):
            synced = False
    text = "\n".join(texts).strip()
    if not text:
        return None
    result = dict(status="ready", text=text, source="Spotify")
    if synced and timed:
        result["lines"] = [dict(time=stamp, text="\n".join(words)) for stamp, words in sorted(timed.items())]
    return result


class SpotifyLyrics:
    def __init__(self):
        self.lock = threading.Lock()
        self.credential = ""
        self.token = ""
        self.expires = 0
        self.secrets = None
        self.secrets_until = 0
        self.retry_at = 0

    def access_token(self, cookie, deadline):
        if self.token and time.monotonic() < self.expires:
            return self.token
        if not self.secrets or time.monotonic() >= self.secrets_until:
            self.secrets = request(SECRET_URL, deadline)
            self.secrets_until = time.monotonic() + 86400
        clock = request(TIME_URL, deadline)
        timestamp = clock.get("serverTime") if isinstance(clock, dict) else None
        if not isinstance(timestamp, (int, float)) or not math.isfinite(timestamp) or timestamp <= 0:
            raise ValueError("Invalid Spotify server time")
        version, code = totp(self.secrets, timestamp)
        params = urlencode(dict(reason="init", productType="web-player", totp=code, totpServer=code, totpVer=version))
        data = request(TOKEN_URL + "?" + params, deadline, {"Cookie": "sp_dc=" + cookie})
        if not isinstance(data, dict) or data.get("isAnonymous") is not False:
            raise ValueError("Spotify login expired")
        token = data.get("accessToken")
        expiry = data.get("accessTokenExpirationTimestampMs")
        if not isinstance(token, str) or not re.fullmatch(r"[A-Za-z0-9._~-]{1,8192}", token):
            raise ValueError("Invalid Spotify access token")
        if not isinstance(expiry, (int, float)) or not math.isfinite(expiry) or expiry / 1000 <= time.time():
            raise ValueError("Expired Spotify access token")
        self.token = token
        self.expires = time.monotonic() + max(0, min(3600, expiry / 1000 - time.time()) - 30)
        return token

    def get(self, track_id, cookie, deadline):
        if not cookie or not TRACK_ID.fullmatch(track_id):
            return None
        if not self.lock.acquire(timeout=max(0, deadline - time.monotonic())):
            raise TimeoutError("Spotify lookup busy")
        try:
            fingerprint = hashlib.sha256(cookie.encode()).hexdigest()
            if self.credential != fingerprint:
                self.credential = fingerprint
                self.token = ""
                self.retry_at = 0
            if time.monotonic() < self.retry_at:
                raise ValueError("Spotify lookup cooling down")
            try:
                for attempt in range(2):
                    token = self.access_token(cookie, deadline)
                    try:
                        data = request(LYRICS_URL + track_id + "?format=json&market=from_token", deadline,
                                       {"Authorization": "Bearer " + token, "App-Platform": "WebPlayer"})
                        return parse_lyrics(data)
                    except HTTPError as error:
                        if error.code == 404:
                            error.close()
                            return None
                        if error.code == 401 and attempt == 0:
                            error.close()
                            self.token = ""
                            continue
                        raise
            except HTTPError as error:
                delay = 300 if error.code in (401, 403) else 30
                if error.code == 429:
                    try:
                        delay = max(60, float(error.headers.get("Retry-After", "60")))
                        if not math.isfinite(delay):
                            delay = 60
                    except ValueError:
                        delay = 60
                error.close()
                self.retry_at = time.monotonic() + delay
                self.secrets_until = 0
                raise ValueError("Spotify lookup unavailable") from None
            except Exception:
                self.retry_at = time.monotonic() + 30
                self.secrets_until = 0
                raise ValueError("Spotify lookup unavailable") from None
        finally:
            self.lock.release()
