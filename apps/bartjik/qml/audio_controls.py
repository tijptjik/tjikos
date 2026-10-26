"""Route and control existing PulseAudio-compatible playback."""
import json
import fcntl
import os
from pathlib import Path
import subprocess
import sys


def pactl(*args):
    return subprocess.check_output(["pactl", *args], text=True, stderr=subprocess.PIPE, timeout=5)


def master(action):
    # Serialize wheel events so fast scrolling cannot read the same old volume.
    with open(Path(os.environ["XDG_RUNTIME_DIR"]) / "audio-master.lock", "w") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        sinks = json.loads(pactl("-f", "json", "list", "sinks"))
        default = pactl("get-default-sink").strip()
        active = next((s for s in sinks if s["name"] == default), None)
        if active is None:
            raise ValueError("No active output device is available.")
        if action == "mute":
            pactl("set-sink-mute", active["name"], "0" if active["mute"] else "1")
        else:
            channels = list(active["volume"].values())
            current = sum(v["value"] for v in channels) / len(channels) / 65536 * 100
            volume = max(0, min(100, round(current) + int(action)))
            pactl("set-sink-volume", active["name"], str(volume) + "%")


def route(name):
    if not name:
        raise ValueError("The selected output is unavailable.")
    sinks = json.loads(pactl("-f", "json", "list", "sinks"))
    if not any(s["name"] == name for s in sinks):
        raise ValueError("The selected output was disconnected.")
    pactl("set-default-sink", name)
    failures = []
    for stream in json.loads(pactl("-f", "json", "list", "sink-inputs")):
        try:
            pactl("move-sink-input", str(stream["index"]), name)
        except subprocess.CalledProcessError:
            # Clients may disappear between enumeration and moving.
            remaining = json.loads(pactl("-f", "json", "list", "sink-inputs"))
            if any(s["index"] == stream["index"] for s in remaining):
                failures.append(str(stream["index"]))
    if failures:
        raise ValueError("Output changed; could not move playback clients: " + ", ".join(failures))


def is_headphones(sink):
    # This USB output is the headphone route used by the audio panel.
    return sink.get("description") == "USB Audio Speakers"


def toggle_output():
    with open(Path(os.environ["XDG_RUNTIME_DIR"]) / "audio-route.lock", "w") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        sinks = json.loads(pactl("-f", "json", "list", "sinks"))
        default = pactl("get-default-sink").strip()
        active = next((s for s in sinks if s["name"] == default), {})
        target = next((s for s in sinks if (
            s.get("description", "").startswith("Navi 21/23 HDMI/DP")
            if is_headphones(active) else is_headphones(s)
        )), None)
        route(target["name"] if target else "")


def watch_output():
    # Subscribe before reading state so a switch during startup cannot be missed.
    with subprocess.Popen(["pactl", "subscribe"], stdout=subprocess.PIPE, text=True) as events:
        previous = None

        def update():
            nonlocal previous
            sinks = json.loads(pactl("-f", "json", "list", "sinks"))
            default = pactl("get-default-sink").strip()
            active = next((s for s in sinks if s["name"] == default), {})
            mode = "headphones" if is_headphones(active) else "speakers"
            if mode != previous:
                print(json.dumps({"text": "headphones" if mode == "headphones" else "volume_up", "class": mode}), flush=True)
                previous = mode

        try:
            update()
            for event in events.stdout:
                if " on server " in event or " on sink " in event:
                    update()
        finally:
            events.terminate()


if __name__ == "__main__":
    try:
        if sys.argv[1] == "master":
            master(sys.argv[2])
        elif sys.argv[1] == "route":
            route(sys.argv[2])
        elif sys.argv[1] == "toggle-output":
            toggle_output()
        elif sys.argv[1] == "output-watch":
            watch_output()
    except (ValueError, OSError, subprocess.SubprocessError) as error:
        print(str(error))
        sys.exit(1)
