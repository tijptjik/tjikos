"""Explicit microphone capture and private Ogg/Opus drafts for notification replies.

The helper owns the recorder until the UI discards or sends the draft. EOF (UI
exit) always stops capture and removes the temporary recording. Tests pass a
synthetic FFmpeg input directly to run(); the CLI always uses the default mic.
"""
import json
import math
import os
from pathlib import Path
import re
import selectors
import shutil
import signal
import stat
import subprocess
import sys
import tempfile
import time
import uuid

MAX_SECONDS = 300
MAX_BYTES = 4 * 1024 * 1024


def draft_dir():
    runtime = os.environ.get("XDG_RUNTIME_DIR")
    if not runtime:
        raise ValueError("XDG_RUNTIME_DIR is required for voice recording")
    path = Path(runtime) / "notjik-voice"
    path.mkdir(mode=0o700, exist_ok=True)
    info = path.lstat()
    if not stat.S_ISDIR(info.st_mode) or info.st_uid != os.getuid():
        raise ValueError("Voice draft directory is not private")
    path.chmod(0o700)
    return path


def draft_path(token):
    if not isinstance(token, str) or not re.fullmatch(r"[0-9a-f]{32}", token):
        raise ValueError("Invalid voice draft")
    return draft_dir() / (token + ".ogg")


def inspect(path):
    if not shutil.which("ffprobe"):
        raise ValueError("Install FFmpeg and ffprobe to use voice replies")
    result = subprocess.run(["ffprobe", "-v", "error", "-show_streams", "-show_format", "-of", "json", str(path)],
                            capture_output=True, timeout=10, check=False)
    if result.returncode:
        raise ValueError("The recording could not be read")
    data = json.loads(result.stdout)
    streams = data.get("streams", [])
    duration = float(data.get("format", {}).get("duration", 0))
    if (len(streams) != 1 or streams[0].get("codec_name") != "opus" or streams[0].get("channels") != 1
            or data.get("format", {}).get("format_name") != "ogg" or not math.isfinite(duration)
            or duration < 0.25 or duration > MAX_SECONDS + 1 or path.stat().st_size > MAX_BYTES):
        raise ValueError("Record a voice reply between 1 second and 5 minutes")
    return duration


def copy_for_upload(token, directory):
    """Only accept our private draft tokens, never a caller-provided file path."""
    source = draft_path(token)
    fd = os.open(source, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    target = None
    try:
        info = os.fstat(fd)
        if (not stat.S_ISREG(info.st_mode) or info.st_uid != os.getuid() or info.st_nlink != 1
                or info.st_mode & 0o077 or not 0 < info.st_size <= MAX_BYTES):
            raise ValueError("Invalid voice draft file")
        directory.mkdir(mode=0o700, parents=True, exist_ok=True)
        directory.chmod(0o700)
        if len(list(directory.glob("*.ogg"))) >= 20:
            raise ValueError("Too many pending voice uploads; finish them in Telegram first")
        out, name = tempfile.mkstemp(suffix=".ogg", prefix="voice-", dir=directory)
        target = Path(name)
        with os.fdopen(out, "wb") as stream:
            remaining = MAX_BYTES + 1
            while remaining:
                block = os.read(fd, min(65536, remaining))
                if not block:
                    break
                stream.write(block)
                remaining -= len(block)
            if not remaining:
                raise ValueError("Voice recording is too large")
        return target, inspect(target)
    except Exception:
        if target:
            target.unlink(missing_ok=True)
        raise
    finally:
        os.close(fd)


def run(input_args=None):
    os.umask(0o077)
    token = uuid.uuid4().hex
    path = draft_path(token)
    recorder = None
    def emit(**fields):
        print(json.dumps(fields), flush=True)
    def interrupted(*_):
        raise KeyboardInterrupt
    signal.signal(signal.SIGTERM, interrupted)
    try:
        if not shutil.which("ffmpeg") or not shutil.which("ffprobe"):
            raise ValueError("Install FFmpeg and ffprobe to use voice replies")
        # FFmpeg receives its own stdin; the UI command stream never reaches it.
        with tempfile.TemporaryFile() as errors:
            recorder = subprocess.Popen(["ffmpeg", "-hide_banner", "-loglevel", "error", "-y",
                *(input_args if input_args is not None else ["-f", "pulse", "-i", "default"]),
                "-t", str(MAX_SECONDS), "-vn", "-ac", "1", "-ar", "48000", "-c:a", "libopus",
                "-b:a", "32k", "-application", "voip", "-progress", "pipe:1", "-nostats", str(path)],
                stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=errors)
            emit(state="recording", duration=0)
            selector = selectors.DefaultSelector()
            selector.register(sys.stdin, selectors.EVENT_READ, "ui")
            selector.register(recorder.stdout, selectors.EVENT_READ, "progress")
            buffers = {"ui": b"", "progress": b""}
            ready = False
            stopped_at = None
            while True:
                for key, _ in selector.select(0.1):
                    name = key.data
                    block = os.read(key.fileobj.fileno(), 65536)
                    if not block:
                        selector.unregister(key.fileobj)
                        if name == "ui":
                            return 0
                        continue
                    buffers[name] += block
                    while b"\n" in buffers[name]:
                        line, buffers[name] = buffers[name].split(b"\n", 1)
                        if name == "ui":
                            command = json.loads(line).get("action")
                            if command == "discard":
                                return 0
                            if command == "stop" and not ready and stopped_at is None:
                                stopped_at = time.monotonic()
                                if recorder.poll() is None:
                                    recorder.stdin.write(b"q\n"); recorder.stdin.flush()
                                emit(state="stopping")
                        elif line.startswith(b"out_time_us=") and not ready:
                            try:
                                emit(duration=max(0, int(line.split(b"=", 1)[1]) // 1000))
                            except ValueError:
                                pass
                if stopped_at and not ready and time.monotonic() - stopped_at > 10:
                    raise ValueError("Microphone did not stop; recording cancelled")
                if recorder.poll() is not None and not ready:
                    if recorder.returncode:
                        raise ValueError("Microphone unavailable; check the default input and mute settings")
                    duration = inspect(path)
                    ready = True
                    emit(state="ready", token=token, source=path.as_uri(), duration=round(duration * 1000))
    except (ValueError, OSError, subprocess.SubprocessError) as error:
        emit(state="error", error=str(error) if isinstance(error, ValueError) else "Voice recording failed")
        return 1
    except KeyboardInterrupt:
        return 0
    finally:
        if recorder and recorder.poll() is None:
            recorder.terminate()
            try:
                recorder.wait(timeout=3)
            except subprocess.TimeoutExpired:
                recorder.kill(); recorder.wait()
        path.unlink(missing_ok=True)


if __name__ == "__main__":
    raise SystemExit(run())
