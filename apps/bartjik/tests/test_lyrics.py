import asyncio
import io
import json
from urllib.error import HTTPError
from urllib.parse import parse_qs, urlsplit
import unittest
from unittest.mock import patch

from bartjik_core import lyrics
from bartjik_core.engine import Engine


TRACK = dict(artist="Artist & friends", title="Song / 歌", album="Album", duration=183.4)


def response(**values):
    return io.BytesIO(json.dumps(values).encode())


class LyricsTests(unittest.IsolatedAsyncioTestCase):
    async def asyncSetUp(self):
        self.cache = lyrics.LyricsCache()
        self.addAsyncCleanup(self.cache.close)

    async def test_shared_lookup_and_cached_result(self):
        with patch.object(lyrics, "urlopen", side_effect=lambda *a, **k: response(plainLyrics="First line\n第二行")) as fetch:
            first, second = await asyncio.gather(self.cache.get(TRACK), self.cache.get(TRACK))
            self.assertEqual(first, dict(status="ready", text="First line\n第二行"))
            self.assertEqual(first, second)
            self.assertEqual(await self.cache.get(TRACK), first)
            self.assertEqual(fetch.call_count, 1)
            request = fetch.call_args.args[0]
            self.assertEqual(urlsplit(request.full_url).hostname, "lrclib.net")
            self.assertEqual(parse_qs(urlsplit(request.full_url).query), {
                "artist_name": [TRACK["artist"]], "track_name": [TRACK["title"]],
                "album_name": [TRACK["album"]], "duration": ["183"]})

    async def test_unknown_duration_omitted_and_different_versions_separated(self):
        with patch.object(lyrics, "urlopen", side_effect=lambda *a, **k: response(plainLyrics="Verse")) as fetch:
            await self.cache.get(dict(TRACK, duration=0, album=""))
            self.assertNotIn("duration", parse_qs(urlsplit(fetch.call_args.args[0].full_url).query))
            await self.cache.get(TRACK)
            await self.cache.get(dict(TRACK, duration=260))
            await self.cache.get(dict(TRACK, album="Live"))
            self.assertEqual(fetch.call_count, 4)

    async def test_not_found_instrumental_and_timed_only(self):
        cases = [
            ([None, [], None], dict(status="missing", text="")),
            ([dict(instrumental=True)], dict(status="instrumental", text="")),
            ([dict(syncedLyrics="[ar:Artist]\n[00:01.20]One\n[00:02.00][Chorus] 二")], dict(status="ready", text="One\n[Chorus] 二")),
            ([dict(plainLyrics=" \n", syncedLyrics=""), [], None], dict(status="missing", text="")),
        ]
        for index, (values, expected) in enumerate(cases):
            with patch.object(self.cache, "request", side_effect=values) as fetch:
                self.assertEqual(await self.cache.get(dict(TRACK, title=str(index))), expected)
                self.assertEqual(fetch.call_count, len(values))

    async def test_failures_cached_then_retry(self):
        with patch.object(lyrics, "urlopen", side_effect=TimeoutError) as fetch:
            self.assertEqual((await self.cache.get(TRACK))["status"], "error")
            await self.cache.get(TRACK)
            self.assertEqual(fetch.call_count, 3)
        key = lyrics.track_key(TRACK)
        self.cache.cache[key] = (0, self.cache.cache[key][1])
        with patch.object(lyrics, "urlopen", return_value=response(plainLyrics="Recovered")):
            self.assertEqual((await self.cache.get(TRACK))["text"], "Recovered")

    async def test_invalid_or_oversized_response_is_not_lyrics(self):
        for index, value in enumerate((b"<html>error</html>", b"[]", b"x" * (lyrics.MAX_RESPONSE + 1))):
            with patch.object(lyrics, "urlopen", return_value=io.BytesIO(value)):
                self.assertEqual((await self.cache.get(dict(TRACK, title=str(index))))["status"], "error")

    async def test_no_lookup_without_artist_and_title(self):
        with patch.object(lyrics, "urlopen") as fetch:
            for track in ({}, dict(TRACK, artist=""), dict(TRACK, title="")):
                self.assertEqual((await self.cache.get(track))["status"], "missing")
            for track in (None, dict(TRACK, artist=["bad"]), dict(TRACK, title="x" * 1025)):
                with self.assertRaises(ValueError):
                    await self.cache.get(track)
            fetch.assert_not_called()

    async def test_bounded_cache_evicts_oldest_unused(self):
        with patch.object(lyrics, "MAX_ENTRIES", 2), patch.object(lyrics, "urlopen", side_effect=lambda *a, **k: response(plainLyrics="Verse")) as fetch:
            await self.cache.get(TRACK)
            await self.cache.get(dict(TRACK, title="Second"))
            await self.cache.get(TRACK)
            await self.cache.get(dict(TRACK, title="Third"))
            await self.cache.get(TRACK)
            self.assertEqual(fetch.call_count, 3)
            await self.cache.get(dict(TRACK, title="Second"))
            self.assertEqual(fetch.call_count, 4)

    async def test_demo_reply_is_offline_and_retains_request_token(self):
        events = []
        engine = Engine(events.append, demo_mode=True)
        with patch.object(lyrics, "urlopen") as fetch:
            await engine.message(dict(op="lyrics", token="request-2", track=TRACK))
            await asyncio.gather(*list(engine.actions))
            self.assertEqual(events[0]["kind"], "lyrics")
            self.assertEqual(events[0]["token"], "request-2")
            self.assertEqual(events[0]["lyrics"]["status"], "ready")
            fetch.assert_not_called()


class MatchingTests(unittest.IsolatedAsyncioTestCase):
    async def asyncSetUp(self):
        self.cache = lyrics.LyricsCache()
        self.addAsyncCleanup(self.cache.close)

    def record(self, **changes):
        return dict(dict(trackName=TRACK["title"], artistName=TRACK["artist"],
                         albumName="Another release", duration=200, plainLyrics="Found verse"), **changes)

    async def test_search_recovers_other_album_and_duration_without_secondary_source(self):
        with patch.object(self.cache, "request", side_effect=[None, [self.record()]]) as fetch:
            result = await self.cache.get(TRACK)
            self.assertEqual(result, dict(status="ready", text="Found verse"))
            search = urlsplit(fetch.call_args.args[0])
            self.assertEqual(search.path, "/api/search")
            self.assertEqual(parse_qs(search.query), {"artist_name": [TRACK["artist"]], "track_name": [TRACK["title"]]})
            self.assertEqual(fetch.call_count, 2)

    async def test_normalized_search_recovers_remaster_and_featured_artist_suffixes(self):
        track = dict(TRACK, title=TRACK["title"] + " (feat. Guest) - 2026 Remaster")
        with patch.object(self.cache, "request", side_effect=[None, [], [self.record()]]) as fetch:
            self.assertEqual((await self.cache.get(track))["status"], "ready")
            self.assertEqual(parse_qs(urlsplit(fetch.call_args.args[0]).query)["track_name"], [TRACK["title"]])
            self.assertEqual(fetch.call_count, 3)

    async def test_wrong_artist_or_song_rejected_and_secondary_source_used(self):
        records = [self.record(artistName="Other artist"), self.record(trackName="Song 2 / 歌"),
                   self.record(artistName=TRACK["artist"] + " Tribute")]
        with patch.object(self.cache, "request", side_effect=[None, records, dict(lyrics="Fallback verse")]) as fetch:
            result = await self.cache.get(TRACK)
            self.assertEqual(result, dict(status="ready", text="Fallback verse", source="lyrics.ovh"))
            self.assertEqual(urlsplit(fetch.call_args.args[0]).hostname, "api.lyrics.ovh")
            self.assertIn("%2F", fetch.call_args.args[0])
            self.assertIn("%26", fetch.call_args.args[0])

    async def test_second_source_works_when_lrclib_is_unavailable(self):
        with patch.object(self.cache, "request", side_effect=[TimeoutError(), TimeoutError(), dict(lyrics="Fallback")]):
            self.assertEqual((await self.cache.get(TRACK))["source"], "lyrics.ovh")

    async def test_partial_failure_is_not_cached_as_missing(self):
        with patch.object(self.cache, "request", side_effect=[None, [], TimeoutError()]):
            self.assertEqual((await self.cache.get(TRACK))["status"], "error")

    async def test_complete_miss_is_cached_and_limited_to_three_requests(self):
        with patch.object(self.cache, "request", side_effect=[None, [], None]) as fetch:
            self.assertEqual((await self.cache.get(TRACK))["status"], "missing")
            self.assertEqual((await self.cache.get(TRACK))["status"], "missing")
            self.assertEqual(fetch.call_count, 3)

    def test_ranking_prefers_exact_release_then_closest_duration(self):
        other = self.record(plainLyrics="Other release")
        same = self.record(albumName=TRACK["album"], duration=183, plainLyrics="Exact release")
        self.assertEqual(lyrics.best_match([other, same], lyrics.track_key(TRACK))["text"], "Exact release")
        near = self.record(duration=184, plainLyrics="Near duration")
        self.assertEqual(lyrics.best_match([other, near], lyrics.track_key(TRACK))["text"], "Near duration")

    def test_search_instrumentals_are_not_evidence_for_current_recording(self):
        self.assertIsNone(lyrics.best_match([self.record(instrumental=True)], lyrics.track_key(TRACK)))

    def test_title_cleanup_preserves_song_words_and_normalizes_punctuation(self):
        self.assertEqual(lyrics.clean_title("(I Can't Get No) Satisfaction"), "(I Can't Get No) Satisfaction")
        self.assertEqual(lyrics.clean_title("Song - Part Two"), "Song - Part Two")
        self.assertEqual(lyrics.clean_title("Song (DJ Remix)"), "Song")
        self.assertEqual(lyrics.normalize("Déjà-vu!"), lyrics.normalize("Deja Vu"))
        self.assertTrue(lyrics.artist_matches("Artist, Guest", "Artist"))
        self.assertFalse(lyrics.artist_matches("Artist Tribute", "Artist"))

    def test_transport_handles_not_found_and_enforces_limits(self):
        missing = HTTPError("https://lrclib.net/api/get", 404, "missing", {}, None)
        with patch.object(lyrics, "urlopen", side_effect=missing):
            self.assertIsNone(self.cache.request("https://lrclib.net/api/get", lyrics.time.monotonic() + 5))
        with patch.object(lyrics, "urlopen") as fetch:
            with self.assertRaises(TimeoutError):
                self.cache.request("https://lrclib.net/api/get", 0)
            fetch.assert_not_called()
        with patch.object(lyrics, "urlopen", return_value=io.BytesIO(b"x" * (lyrics.MAX_RESPONSE + 1))):
            with self.assertRaises(ValueError):
                self.cache.request("https://lrclib.net/api/get", lyrics.time.monotonic() + 5)


class TimingTests(unittest.TestCase):
    def test_fractional_repeated_and_out_of_order_timestamps(self):
        self.assertEqual(lyrics.timed_lines('[00:12.345]Last\n[00:01.2][00:05.25]Repeat\n[00:03]Middle'), [
            dict(time=1.2,text='Repeat'),dict(time=3,text='Middle'),dict(time=5.25,text='Repeat'),dict(time=12.345,text='Last')])

    def test_offset_blank_interlude_and_simultaneous_translation(self):
        self.assertEqual(lyrics.timed_lines('[offset:+500]\n[00:01]Hello\n[00:01]你好\n[00:01]Hello\n[00:02]\n[00:04]Again'), [
            dict(time=.5,text='Hello\n你好'),dict(time=1.5,text=''),dict(time=3.5,text='Again')])
        self.assertEqual(lyrics.timed_lines('[offset:-250]\n[00:00]Start')[0]['time'], .25)

    def test_metadata_malformed_tags_and_word_timestamps(self):
        self.assertEqual(lyrics.timed_lines('[ar:Artist]\n[00:99]Invalid\n[bad]No time\n[00:02]<00:02.1>Two <00:02.3>words'), [dict(time=2,text='Two words')])
        self.assertEqual(lyrics.timed_lines(None), [])
        self.assertEqual(lyrics.timed_lines('[ar:Artist]\nPlain text'), [])

    def record(self, **changes):
        return dict(dict(artistName=TRACK['artist'],trackName=TRACK['title'],duration=TRACK['duration'],
                         plainLyrics='First\nSecond',syncedLyrics='[00:01]First\n[00:03]Second'), **changes)

    def test_matching_recording_keeps_timing_and_plain_text(self):
        result=lyrics.track_result(self.record(),lyrics.track_key(TRACK))
        self.assertEqual(result['text'],'First\nSecond')
        self.assertEqual(result['lines'],[dict(time=1,text='First'),dict(time=3,text='Second')])

    def test_other_length_live_or_remix_keeps_words_without_guessing_timing(self):
        for record in (self.record(duration=220),self.record(trackName=TRACK['title']+' - Live'),
                       self.record(trackName=TRACK['title']+' (DJ Remix)'),
                       self.record(trackName=TRACK['title']+' (Live - Remastered)'),self.record(artistName='Other')):
            result=lyrics.track_result(record,lyrics.track_key(TRACK))
            self.assertEqual(result['text'],'First\nSecond')
            self.assertNotIn('lines',result)
        self.assertNotIn('lines',lyrics.track_result(self.record(),lyrics.track_key(dict(TRACK,duration=0))))

    def test_matching_remaster_can_keep_timing(self):
        record=self.record(trackName=TRACK['title']+' - 2026 Remaster')
        self.assertIn('lines',lyrics.track_result(record,lyrics.track_key(TRACK)))

    def test_search_prefers_aligned_timing_over_plain_release(self):
        plain=self.record(syncedLyrics=None,albumName=TRACK['album'])
        timed=self.record(albumName='Other release')
        self.assertIn('lines',lyrics.best_match([plain,timed],lyrics.track_key(TRACK)))
        self.assertNotIn('lines',lyrics.parse_result(dict(plainLyrics='Words',syncedLyrics='invalid')))
