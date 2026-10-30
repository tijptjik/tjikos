
# Test-only comparison oracle; production launches the Rust binary.
import sys as _sys
from pathlib import Path as _Path
_sys.path.insert(0, str(_Path(__file__).resolve().parent / "reference"))
_sys.path.insert(0, str(_Path(__file__).resolve().parent / "reference/qml"))
import asyncio
from contextlib import redirect_stdout, redirect_stderr
import hashlib
import hmac
import io
import json
import os
from pathlib import Path
import tempfile
import time
import unittest
from urllib.error import HTTPError
from urllib.parse import urlsplit
from unittest.mock import patch

import bartjik
from bartjik_core import lyrics, spotify_lyrics as spotify

TRACK_ID = '1234567890abcdefghijkl'
TRACK = dict(artist='Synthetic artist', title='Synthetic song', album='Fixture', duration=120, spotifyId=TRACK_ID)
COOKIE = 'synthetic-cookie'
SECRETS = {'1': [1, 2, 3], '9': [4, 5, 6], '2': [7, 8, 9]}


def payload(sync='LINE_SYNCED', lines=None):
    return {'lyrics': {'syncType': sync, 'lines': lines if lines is not None else [
        {'startTimeMs': '1250', 'words': 'Synthetic verse'},
        {'startTimeMs': '3000', 'words': ''},
        {'startTimeMs': '4250', 'words': 'Synthetic refrain'}]}}


def token(value='synthetic-token'):
    return dict(accessToken=value, isAnonymous=False, accessTokenExpirationTimestampMs=(time.time() + 3600) * 1000)


def error(code, delay=None):
    return HTTPError('https://spotify.invalid', code, 'fixture', {'Retry-After': str(delay)} if delay is not None else {}, None)


class SpotifyTests(unittest.TestCase):
    def setUp(self):
        self.source = spotify.SpotifyLyrics()
        self.deadline = time.monotonic() + 8

    def test_authentication_cached_and_credentials_only_sent_to_token_endpoint(self):
        with patch.object(spotify, 'request', side_effect=[SECRETS, {'serverTime': 1700000000}, token(), payload(), payload()]) as fetch:
            result = self.source.get(TRACK_ID, COOKIE, self.deadline)
            again = self.source.get(TRACK_ID, COOKIE, self.deadline)
        self.assertEqual(result, again)
        self.assertEqual(result['lines'][0], dict(time=1.25, text='Synthetic verse'))
        self.assertEqual(result['lines'][1], dict(time=3.0, text=''))
        self.assertEqual(fetch.call_count, 5)
        for call in fetch.call_args_list:
            url = call.args[0]
            headers = call.args[2] if len(call.args) > 2 else {}
            self.assertNotIn(COOKIE, url)
            self.assertEqual('Cookie' in headers, url.startswith(spotify.TOKEN_URL))
            self.assertEqual('Authorization' in headers, url.startswith(spotify.LYRICS_URL))
        self.assertIn('totpVer=9', fetch.call_args_list[2].args[0])

    def test_totp_uses_latest_numeric_version_and_standard_counter(self):
        version, code = spotify.totp(SECRETS, 1700000000)
        key = ''.join(str(v ^ ((i % 33) + 9)) for i, v in enumerate(SECRETS['9'])).encode()
        digest = hmac.new(key, (1700000000 // 30).to_bytes(8, 'big'), hashlib.sha1).digest()
        offset = digest[-1] & 15
        expected = (int.from_bytes(digest[offset:offset + 4], 'big') & 0x7fffffff) % 1000000
        self.assertEqual((version, code), ('9', f'{expected:06d}'))
        for data in ([], {}, {'1': []}, {'1': [True]}, {'1': [300]}):
            with self.assertRaises(ValueError):
                spotify.totp(data, 1700000000)

    def test_unauthorized_refreshes_once_then_backs_off(self):
        with patch.object(spotify, 'request', side_effect=[SECRETS, {'serverTime': 1700000000}, token(), error(401), {'serverTime': 1700000000}, token('refreshed'), payload()]) as fetch:
            self.assertEqual(self.source.get(TRACK_ID, COOKIE, self.deadline)['status'], 'ready')
            self.assertEqual(fetch.call_count, 7)
            self.assertEqual(fetch.call_args.args[2]['Authorization'], 'Bearer refreshed')
        self.source.token = ''
        with patch.object(spotify, 'request', side_effect=[{'serverTime': 1700000000}, token(), error(401), {'serverTime': 1700000000}, token(), error(401)]) as fetch:
            with self.assertRaises(ValueError):
                self.source.get(TRACK_ID, COOKIE, self.deadline)
            with self.assertRaises(ValueError):
                self.source.get(TRACK_ID, COOKIE, self.deadline)
            self.assertEqual(fetch.call_count, 6)
            self.assertGreater(self.source.retry_at, time.monotonic() + 290)

    def test_rate_limit_retries_after_delay_and_cookie_rotation_resets_auth(self):
        with patch.object(spotify, 'request', side_effect=error(429, 180)) as fetch:
            for _ in range(2):
                with self.assertRaises(ValueError):
                    self.source.get(TRACK_ID, COOKIE, self.deadline)
            self.assertEqual(fetch.call_count, 1)
            self.assertGreater(self.source.retry_at, time.monotonic() + 175)
        with patch.object(spotify, 'request', side_effect=[SECRETS, {'serverTime': 1700000000}, token(), payload()]):
            self.assertEqual(self.source.get(TRACK_ID, 'replacement', self.deadline)['status'], 'ready')

    def test_missing_lyrics_is_not_an_auth_failure(self):
        with patch.object(spotify, 'request', side_effect=[SECRETS, {'serverTime': 1700000000}, token(), error(404)]):
            self.assertIsNone(self.source.get(TRACK_ID, COOKIE, self.deadline))
        self.assertEqual(self.source.retry_at, 0)

    def test_expired_and_anonymous_tokens_rejected_without_lyrics_request(self):
        for bad in (dict(token(), isAnonymous=True), dict(token(), accessTokenExpirationTimestampMs=0), dict(token(), accessToken='bad\r\nheader'), {}):
            source = spotify.SpotifyLyrics()
            with patch.object(spotify, 'request', side_effect=[SECRETS, {'serverTime': 1700000000}, bad]) as fetch:
                with self.assertRaises(ValueError):
                    source.get(TRACK_ID, COOKIE, self.deadline)
                self.assertEqual(fetch.call_count, 3)

    def test_skip_without_login_or_valid_track_id(self):
        with patch.object(spotify, 'request') as fetch:
            for track_id, cookie in ((TRACK_ID, ''), ('file:///tmp/x', COOKIE), ('bad', COOKIE)):
                self.assertIsNone(self.source.get(track_id, cookie, self.deadline))
            fetch.assert_not_called()

    def test_parser_preserves_blank_intervals_and_simultaneous_lines(self):
        result = spotify.parse_lyrics(payload(lines=[{'startTimeMs':'1000','words':'First'}, {'startTimeMs':'1000','words':'Translation'}, {'startTimeMs':'2000','words':''}]))
        self.assertEqual(result['lines'], [dict(time=1, text='First\nTranslation'), dict(time=2, text='')])
        for sync, stamp in (('UNSYNCED', '1000'), ('LINE_SYNCED', 'NaN'), ('LINE_SYNCED', '-1')):
            self.assertNotIn('lines', spotify.parse_lyrics(payload(sync, [{'words':'Plain', 'startTimeMs':stamp}])))
        self.assertIsNone(spotify.parse_lyrics(payload(lines=[])))
        for value in ({}, [], {'lyrics': []}, {'lyrics': {'lines': ['bad']}}):
            with self.assertRaises(ValueError):
                spotify.parse_lyrics(value)

    def test_network_bounds_and_no_redirects(self):
        with patch.object(spotify, 'build_opener') as opener:
            with self.assertRaises(TimeoutError):
                spotify.request(spotify.TIME_URL, 0)
            opener.assert_not_called()
            opener.return_value.open.return_value = io.BytesIO(b'x' * (spotify.MAX_RESPONSE + 1))
            with self.assertRaises(ValueError):
                spotify.request(spotify.TIME_URL, self.deadline)
            self.assertIsInstance(opener.call_args.args[0], spotify.NoRedirect)
        self.assertIsNone(spotify.NoRedirect().redirect_request(None, None, 302, '', {}, 'https://other.example'))


class CredentialTests(unittest.TestCase):
    def test_hidden_login_permissions_status_logout_and_cli(self):
        with tempfile.TemporaryDirectory() as tmp, patch.dict(os.environ, XDG_CONFIG_HOME=tmp):
            with patch.object(spotify.getpass, 'getpass', return_value=COOKIE):
                self.assertTrue(spotify.configure('login')['configured'])
            path = spotify.credential_path()
            self.assertEqual(path.stat().st_mode & 0o777, 0o600)
            self.assertEqual(spotify.read_cookie(), COOKIE)
            output = io.StringIO()
            with redirect_stdout(output), patch.object(spotify, 'request') as fetch:
                self.assertEqual(bartjik.main(['spotify', 'status']), 0)
                fetch.assert_not_called()
            self.assertTrue(json.loads(output.getvalue())['configured'])
            self.assertNotIn(COOKIE, output.getvalue())
            self.assertFalse(spotify.configure('logout')['configured'])
            self.assertFalse(path.exists())

    def test_reject_insecure_file_symlink_and_header_injection(self):
        with tempfile.TemporaryDirectory() as tmp, patch.dict(os.environ, XDG_CONFIG_HOME=tmp):
            path = spotify.credential_path()
            path.parent.mkdir()
            path.write_text(COOKIE)
            path.chmod(0o644)
            self.assertEqual(spotify.read_cookie(), '')
            path.unlink()
            path.symlink_to(Path(tmp) / 'missing')
            self.assertEqual(spotify.read_cookie(), '')
            for value in ('sp_dc=value; more=bad', 'foo\nCookie: bad', 'x' * 4097, ''):
                self.assertFalse(spotify.valid_cookie(value))


class CacheIntegrationTests(unittest.IsolatedAsyncioTestCase):
    async def asyncSetUp(self):
        self.cache = lyrics.LyricsCache()
        self.addAsyncCleanup(self.cache.close)

    async def test_spotify_first_shared_and_exact_ids_separate(self):
        result = spotify.parse_lyrics(payload())
        with patch.object(spotify, 'read_cookie', return_value=COOKIE), patch.object(self.cache.spotify, 'get', return_value=result) as fetch, patch.object(self.cache, 'request') as fallback:
            first, second = await asyncio.gather(self.cache.get(TRACK), self.cache.get(TRACK))
            self.assertEqual(first, second)
            self.assertEqual(first['lines'][0]['time'], 1.25)
            await self.cache.get(TRACK)
            self.assertEqual(fetch.call_count, 1)
            await self.cache.get(dict(TRACK, spotifyId='abcdefghijkl1234567890'))
            self.assertEqual(fetch.call_count, 2)
            fallback.assert_not_called()

    async def test_missing_and_failure_fall_back_and_failure_uses_short_cache(self):
        for outcome in (None, ValueError('no auth')):
            cache = lyrics.LyricsCache()
            self.addAsyncCleanup(cache.close)
            with patch.object(spotify, 'read_cookie', return_value=COOKIE), patch.object(cache.spotify, 'get', side_effect=[outcome]), patch.object(cache, 'request', return_value={'plainLyrics': 'Fallback'}):
                self.assertEqual((await cache.get(TRACK))['text'], 'Fallback')
                expiry, result = next(iter(cache.cache.values()))
                self.assertNotIn('_retry', result)
                if outcome is not None:
                    self.assertLessEqual(expiry - time.monotonic(), 30)

    async def test_configuration_change_invalidates_previous_public_result(self):
        with patch.object(spotify, 'read_cookie', return_value=''), patch.object(self.cache.spotify, 'get') as fetch, patch.object(self.cache, 'request', return_value={'plainLyrics': 'Public'}):
            self.assertEqual((await self.cache.get(TRACK))['text'], 'Public')
            fetch.assert_not_called()
        with patch.object(spotify, 'read_cookie', return_value=COOKIE), patch.object(self.cache.spotify, 'get', return_value=spotify.parse_lyrics(payload())) as fetch:
            self.assertEqual((await self.cache.get(TRACK))['source'], 'Spotify')
            fetch.assert_called_once()
        with patch.object(spotify, 'read_cookie', return_value=''), patch.object(self.cache.spotify, 'get') as fetch:
            self.assertEqual((await self.cache.get(TRACK))['text'], 'Public')
            fetch.assert_not_called()
