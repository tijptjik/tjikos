"""Synthetic saved-state and desktop-action regression tests; no live Spotify."""

# Test-only comparison oracle; production launches the Rust binary.
import sys as _sys
from pathlib import Path as _Path
_sys.path.insert(0, str(_Path(__file__).resolve().parent / "reference"))
_sys.path.insert(0, str(_Path(__file__).resolve().parent / "reference/qml"))
import os
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import Mock, patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
sys.path.insert(0, str(ROOT / 'qml'))
from bartjik_core import spotify_navigation as nav
import media_navigate as helper

TRACK = 'a' * 22
OTHER = 'b' * 22
ARTIST = 'spotify:artist:' + 'c' * 22
PLAYLIST = 'spotify:playlist:' + 'd' * 22


def integer(value):
    result = bytearray()
    while value > 127:
        result.append((value & 127) | 128)
        value >>= 7
    result.append(value)
    return bytes(result)


def field(number, value):
    if isinstance(value, int):
        return integer(number << 3) + integer(value)
    if isinstance(value, str):
        value = value.encode()
    return integer((number << 3) | 2) + integer(len(value)) + value


def track(track_id=TRACK, artist=ARTIST, playlist=PLAYLIST):
    return field(2, 'spotify:track:' + track_id) + b''.join(
        field(3, field(1, key) + field(2, value))
        for key, value in [('artist_uri', artist), ('context_uri', playlist)])


def snapshot(current=None, queued=None, version=15):
    session = field(3, current if current is not None else track())
    queue = field(1, session)
    if queued:
        queue += field(2, field(3, queued)) + field(3, field(2, field(3, queued)))
    return b'1792934240052#' + field(1, version) + field(3, field(3, queue))


class StateTests(unittest.TestCase):
    def test_exact_current_artist_and_playlist(self):
        self.assertEqual(nav.parse_state(snapshot(), TRACK), dict(artist=ARTIST, playlist=PLAYLIST))

    def test_never_uses_queued_or_historical_track(self):
        with self.assertRaisesRegex(ValueError, 'not saved'):
            nav.parse_state(snapshot(track(OTHER), track()), TRACK)
        self.assertEqual(nav.parse_state(snapshot(track(), track(OTHER, 'spotify:artist:' + OTHER)), TRACK)['artist'], ARTIST)

    def test_non_playlist_context_does_not_become_track_link(self):
        self.assertEqual(nav.parse_state(snapshot(track(playlist='spotify:album:' + OTHER)), TRACK)['playlist'], '')

    def test_incomplete_oversize_and_unknown_state_fail_closed(self):
        for value in [snapshot()[:-1], b'bad#'+snapshot(), snapshot(version=16), b'0'*(nav.MAX_STATE+1), b'1792934240052#\x80'*4]:
            with self.subTest(size=len(value)), self.assertRaises(ValueError):
                nav.parse_state(value, TRACK)

    def test_duplicate_active_session_rejected(self):
        raw=b'1792934240052#'+field(1,15)+field(3,field(3,field(1,field(3,track()))*2))
        with self.assertRaises(ValueError):nav.parse_state(raw,TRACK)

    def test_uris_are_typed_and_allowlisted(self):
        self.assertEqual(nav.spotify_uri('https://open.spotify.com/artist/'+'c'*22+'?si=test','artist'),ARTIST)
        for value in ['https://evil.test/artist/'+'c'*22, 'spotify:search:artist', 'spotify:track:'+TRACK, ARTIST+';echo no', ARTIST+'\n', 'spotify:artist:short']:
            self.assertEqual(nav.spotify_uri(value,'artist'),'')

    def test_newest_account_only_and_no_symlink(self):
        with tempfile.TemporaryDirectory() as td:
            old,new,link=(Path(td)/name for name in ('old','new','link'))
            old.write_bytes(snapshot());new.write_bytes(snapshot(track(OTHER)))
            os.utime(old,ns=(1,1));os.utime(new,ns=(2,2));link.symlink_to(old)
            with self.assertRaisesRegex(ValueError,'not saved'):nav.read_context(TRACK,[old,new])
            with self.assertRaises(ValueError):nav.read_context(TRACK,[link])
            self.assertEqual(nav.read_context(TRACK,[old])['playlist'],PLAYLIST)

    def test_direct_link_needs_no_local_state(self):
        with patch.object(nav,'read_context') as read:
            self.assertEqual(nav.destination('artist',TRACK,ARTIST),ARTIST)
        read.assert_not_called()
        with patch.object(nav,'read_context',return_value={'playlist':''}):
            with self.assertRaisesRegex(ValueError,'not playing from a playlist'):nav.destination('playlist',TRACK)


class NavigationTests(unittest.TestCase):
    def setUp(self):
        self.obj = Mock()
        self.obj.GetConnectionUnixProcessID.return_value = 42
        self.bus = Mock()
        self.bus.get_object.return_value = self.obj
        self.request = dict(target='artist', player='org.mpris.MediaPlayer2.spotify', trackId=TRACK)
        self.dbus = SimpleNamespace(Interface=lambda obj, interface: obj, DBusException=RuntimeError)

    def test_exact_page_command_and_focus_never_call_openuri(self):
        self.obj.Get.return_value = {'xesam:url': 'https://open.spotify.com/track/' + TRACK}
        command = ['flatpak', 'enter', '42', '/app/extra/bin/spotify', '--uri=' + ARTIST]
        with patch.dict(sys.modules, dbus=self.dbus), patch.dict(os.environ, HYPRLAND_INSTANCE_SIGNATURE='fixture'), patch.object(helper, 'destination', return_value=ARTIST) as resolve, patch.object(helper, 'navigation_command', return_value=command) as argv, patch.object(helper.subprocess, 'run') as run, patch.object(helper.tray_focus, 'main') as focus:
            helper.navigate(self.request, self.bus)
        resolve.assert_called_once_with('artist', TRACK, '')
        argv.assert_called_once_with(ARTIST, 42)
        run.assert_called_once_with(command, capture_output=True, check=True, timeout=6)
        focus.assert_called_once_with('spotify')
        self.obj.Raise.assert_called_once()
        self.obj.OpenUri.assert_not_called()
        self.assertEqual({call[0] for call in self.obj.method_calls}, {'Get', 'GetConnectionUnixProcessID', 'Raise'})

    def test_track_change_during_resolution_cancels_navigation(self):
        self.obj.Get.side_effect = [{'xesam:url': 'spotify:track:' + TRACK}, {'xesam:url': 'spotify:track:' + OTHER}]
        with patch.dict(sys.modules, dbus=self.dbus), patch.object(helper, 'destination', return_value=ARTIST), patch.object(helper.subprocess, 'run') as run:
            with self.assertRaisesRegex(ValueError, 'track changed'):
                helper.navigate(self.request, self.bus)
        run.assert_not_called()
        self.obj.OpenUri.assert_not_called()

    def test_wrong_player_and_no_playlist_never_open_search_or_track(self):
        self.obj.Get.return_value = {'xesam:url': 'spotify:track:' + TRACK}
        with patch.dict(sys.modules, dbus=self.dbus), patch.object(helper.subprocess, 'run') as run:
            with self.assertRaises(ValueError):
                helper.navigate(dict(self.request, player='other'), self.bus)
            with patch.object(helper, 'destination', side_effect=ValueError('No playlist')):
                with self.assertRaises(ValueError):
                    helper.navigate(dict(self.request, target='playlist'), self.bus)
        run.assert_not_called()
        self.obj.OpenUri.assert_not_called()

    def test_failed_delivery_does_not_only_focus_or_fall_back_to_playback(self):
        self.obj.Get.return_value = {'xesam:url': 'spotify:track:' + TRACK}
        with patch.dict(sys.modules, dbus=self.dbus), patch.object(helper, 'destination', return_value=ARTIST), patch.object(helper, 'navigation_command', return_value=['fixture']), patch.object(helper.subprocess, 'run', side_effect=RuntimeError('Delivery failed')), patch.object(helper.tray_focus, 'main') as focus:
            with self.assertRaisesRegex(RuntimeError, 'Delivery failed'):
                helper.navigate(self.request, self.bus)
        focus.assert_not_called()
        self.obj.Raise.assert_not_called()
        self.obj.OpenUri.assert_not_called()

    def test_exact_flatpak_instance_uses_native_navigation_option(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            info = root / '42/root/.flatpak-info'
            info.parent.mkdir(parents=True)
            info.write_text('[Application]\nname=com.spotify.Client\n[Instance]\ninstance-id=123456\n')
            self.assertEqual(helper.navigation_command(PLAYLIST, 42, root),
                             ['flatpak', 'enter', '123456', '/app/extra/bin/spotify', '--uri=' + PLAYLIST])
            for value in ['123; echo wrong', '../42', '']:
                info.write_text('[Application]\nname=com.spotify.Client\n[Instance]\ninstance-id=' + value + '\n')
                with self.assertRaises(ValueError):
                    helper.navigation_command(PLAYLIST, 42, root)
            info.write_text('[Application]\nname=Other.App\n[Instance]\ninstance-id=123456\n')
            with self.assertRaises(ValueError):
                helper.navigation_command(PLAYLIST, 42, root)

    def test_native_binary_navigation_and_invalid_targets(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            (root / '42/root').mkdir(parents=True)
            (root / '42/exe').symlink_to('/usr/share/spotify/spotify')
            self.assertEqual(helper.navigation_command(ARTIST, 42, root),
                             ['/usr/share/spotify/spotify', '--uri=' + ARTIST])
            for uri in ['spotify:track:' + TRACK, 'spotify:search:anything', 'https://evil.example/']:
                with self.assertRaises(ValueError):
                    helper.navigation_command(uri, 42, root)
            (root / '42/exe').unlink()
            (root / '42/exe').symlink_to('/usr/bin/other-app')
            with self.assertRaises(ValueError):
                helper.navigation_command(ARTIST, 42, root)

    def test_spotify_focus_matches_native_and_flatpak_windows(self):
        for name in ('spotify', 'Spotify', 'com.spotify.Client'):
            client = {'class': name, 'mapped': True}
            self.assertIs(helper.tray_focus.select_client('spotify', [client]), client)
