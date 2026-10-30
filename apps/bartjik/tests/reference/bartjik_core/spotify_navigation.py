"""Read-only Spotify desktop navigation context; no Web API or credentials.

Wire field numbers follow librespot protocol/proto/state_restore schemas:
ContextPlayerRestorable.player_model -> PlayerModel.session_queue ->
PlayerSessionQueue.active -> PlayerSession.track (ProvidedTrack).
Only that current track is eligible, never queued tracks or playback history.
"""
import os
from pathlib import Path
import re
import stat

MAX_STATE = 2 * 1024 * 1024
ID = r"[A-Za-z0-9]{22}"


def spotify_uri(value, kind):
    if not isinstance(value, str) or kind not in ("artist", "playlist", "track"):
        return ""
    match = re.fullmatch(r"spotify:" + kind + ":(" + ID + ")", value)
    if not match:
        match = re.fullmatch(r"https://open\.spotify\.com/(?:intl-[a-zA-Z-]+/)?" + kind + "/(" + ID + r")(?:\?[^\s]*)?", value)
    return "spotify:" + kind + ":" + match[1] if match else ""


def fields(data):
    """Small bounded protobuf wire reader; unknown fields are skipped by callers."""
    offset = 0

    def varint():
        nonlocal offset
        value = 0
        for shift in range(0, 70, 7):
            if offset >= len(data):
                raise ValueError("Incomplete Spotify state")
            byte = data[offset]
            offset += 1
            if shift == 63 and byte > 1:
                break
            value |= (byte & 127) << shift
            if byte < 128:
                return value
        raise ValueError("Invalid Spotify state integer")

    result = {}
    while offset < len(data):
        tag = varint()
        number, wire = tag >> 3, tag & 7
        if not 0 < number < 2**29:
            raise ValueError("Invalid Spotify state field")
        if wire == 0:
            value = varint()
        elif wire in (1, 2, 5):
            size = varint() if wire == 2 else 8 if wire == 1 else 4
            if size > len(data) - offset:
                raise ValueError("Incomplete Spotify state field")
            value = data[offset:offset + size]
            offset += size
        else:
            raise ValueError("Unsupported Spotify state encoding")
        result.setdefault(number, []).append((wire, value))
    return result


def one(message, number, wire=2):
    entries = message.get(number, [])
    if len(entries) != 1 or entries[0][0] != wire:
        raise ValueError("Missing or ambiguous Spotify state field")
    return entries[0][1]


def parse_state(raw, track_id):
    if not isinstance(track_id, str) or not re.fullmatch(ID, track_id):
        raise ValueError("Spotify track is unavailable")
    if len(raw) > MAX_STATE:
        raise ValueError("Spotify state exceeds size limit")
    stamp, separator, payload = raw.partition(b"#")
    if not separator or not re.fullmatch(rb"[0-9]{10,16}", stamp):
        raise ValueError("Unsupported Spotify saved state")
    message = fields(payload)
    # Unknown format versions fail closed instead of interpreting unrelated fields.
    if one(message, 1, 0) != 15:
        raise ValueError("Unsupported Spotify saved-state version")
    for number in (3, 3, 1, 3):
        message = fields(one(message, number))
    if one(message, 2).decode("utf-8") != "spotify:track:" + track_id:
        raise ValueError("Spotify has not saved this track's navigation context yet")
    metadata = {}
    for wire, entry in message.get(3, []):
        if wire != 2:
            raise ValueError("Invalid Spotify track metadata")
        pair = fields(entry)
        key, value = one(pair, 1).decode("utf-8"), one(pair, 2).decode("utf-8")
        if key in ("artist_uri", "context_uri"):
            if key in metadata and metadata[key] != value:
                raise ValueError("Ambiguous Spotify navigation context")
            metadata[key] = value
    return {"artist": spotify_uri(metadata.get("artist_uri"), "artist"),
            "playlist": spotify_uri(metadata.get("context_uri"), "playlist")}


def state_paths():
    home = Path.home()
    cache = Path(os.environ.get("XDG_CACHE_HOME", str(home / ".cache")))
    roots = [cache / "spotify/Users",
             home / ".var/app/com.spotify.Client/cache/spotify/Users",
             home / "snap/spotify/common/.cache/spotify/Users"]
    return [path for root in roots for path in root.glob("*/context_player_state_restore")]


def read_context(track_id, paths=None):
    # Use the newest account snapshot only. Never search old accounts/history for
    # an occurrence of the same song: it may have a different playlist context.
    candidates = []
    for path in state_paths() if paths is None else paths:
        try:
            info = path.lstat()
            if stat.S_ISREG(info.st_mode) and info.st_uid == os.getuid():
                candidates.append((info.st_mtime_ns, path))
        except OSError:
            continue
    if not candidates:
        raise ValueError("Spotify desktop navigation context is unavailable")
    path = max(candidates, key=lambda pair: pair[0])[1]
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(fd, "rb") as source:
        before = os.fstat(source.fileno())
        if not stat.S_ISREG(before.st_mode) or before.st_uid != os.getuid() or before.st_size > MAX_STATE:
            raise ValueError("Invalid Spotify saved state")
        raw = source.read(MAX_STATE + 1)
        after = os.fstat(source.fileno())
        if (before.st_mtime_ns, before.st_size) != (after.st_mtime_ns, after.st_size):
            raise ValueError("Spotify navigation context is updating; try again")
    return parse_state(raw, track_id)


def destination(target, track_id, direct="", paths=None):
    if target not in ("artist", "playlist"):
        raise ValueError("Unsupported Spotify navigation action")
    uri = spotify_uri(direct, target)
    if uri:
        return uri
    uri = read_context(track_id, paths)[target]
    if not uri:
        message = "This track is not playing from a playlist" if target == "playlist" else "Spotify artist link is unavailable"
        raise ValueError(message)
    return uri
