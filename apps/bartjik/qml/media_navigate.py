"""Open a media destination and focus its desktop window, without playback commands."""
import configparser
import json
import subprocess
import os
from pathlib import Path
import re
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from bartjik_core.spotify_navigation import destination, spotify_uri
import tray_focus


def current_track(props):
    metadata = props.Get("org.mpris.MediaPlayer2.Player", "Metadata", timeout=2)
    uri = spotify_uri(str(metadata.get("xesam:url", "")), "track")
    if uri:
        return uri.rsplit(":", 1)[1]
    match = re.fullmatch(r"/com/spotify/track/([A-Za-z0-9]{22})", str(metadata.get("mpris:trackid", "")))
    return match[1] if match else ""


def navigation_command(uri, owner_pid, proc_root=Path('/proc')):
    """Use the native --uri navigation handler, never MPRIS OpenUri playback."""
    if not (spotify_uri(uri, 'artist') or spotify_uri(uri, 'playlist')):
        raise ValueError('Invalid Spotify navigation destination')
    if not isinstance(owner_pid, int) or owner_pid <= 0:
        raise ValueError('Spotify process is unavailable')
    process = proc_root / str(owner_pid)
    info = process / 'root/.flatpak-info'
    try:
        with info.open() as source:
            raw = source.read(65537)
        if len(raw) > 65536:
            raise ValueError('Invalid Spotify instance metadata')
    except FileNotFoundError:
        executable = Path(os.readlink(process / 'exe'))
        if not executable.is_absolute() or executable.name != 'spotify':
            raise ValueError('Spotify native executable is unavailable')
        return [str(executable), '--uri=' + uri]
    instance = configparser.ConfigParser(interpolation=None)
    instance.read_string(raw)
    app = instance.get('Application', 'name', fallback='')
    identifier = instance.get('Instance', 'instance-id', fallback='')
    if app != 'com.spotify.Client' or not re.fullmatch(r'[0-9]{1,20}', identifier):
        raise ValueError('Spotify Flatpak instance is unavailable')
    # The Flatpak wrapper translates bare spotify: arguments into OpenUri, which
    # starts playback. Enter the already running instance and call the binary's
    # documented navigation option instead. This also avoids a new sandbox and
    # the broken document-portal mount that affected the desktop launcher.
    return ['flatpak', 'enter', identifier, '/app/extra/bin/spotify', '--uri=' + uri]


def navigate(request, bus=None):
    import dbus
    player = request.get("player", "")
    track_id = request.get("trackId", "")
    if not re.fullmatch(r"org\.mpris\.MediaPlayer2\.spotify(?:[.][A-Za-z0-9_-]+)?", player):
        raise ValueError("Exact media navigation is currently supported for Spotify")
    bus = bus or dbus.SessionBus()
    obj = bus.get_object(player, "/org/mpris/MediaPlayer2")
    props = dbus.Interface(obj, "org.freedesktop.DBus.Properties")
    if not track_id or current_track(props) != track_id:
        raise ValueError("The track changed; click its updated label")
    uri = destination(request.get("target"), track_id, request.get("uri", ""))
    # Resolution must not navigate for a song that changed during a file read.
    if current_track(props) != track_id:
        raise ValueError("The track changed; click its updated label")
    broker = dbus.Interface(bus.get_object('org.freedesktop.DBus', '/org/freedesktop/DBus'), 'org.freedesktop.DBus')
    owner_pid = int(broker.GetConnectionUnixProcessID(player, timeout=2))
    command = navigation_command(uri, owner_pid)
    subprocess.run(command, capture_output=True, check=True, timeout=6)
    # Raise alone does not switch Hyprland's workspace or guarantee input focus.
    try:
        dbus.Interface(obj, "org.mpris.MediaPlayer2").Raise(timeout=2)
    except dbus.DBusException:
        pass
    if os.environ.get("HYPRLAND_INSTANCE_SIGNATURE"):
        tray_focus.main("spotify")


def main():
    try:
        raw = sys.stdin.read(4097)
        if len(raw) > 4096:
            raise ValueError("Invalid media navigation request")
        request = json.loads(raw)
        if not isinstance(request, dict):
            raise ValueError("Invalid media navigation request")
        navigate(request)
    except ValueError as error:
        print(str(error), file=sys.stderr)
        return 1
    except Exception:
        # No private track records, file paths or D-Bus payloads in diagnostics.
        print("Spotify navigation failed; check that the desktop app is running", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
