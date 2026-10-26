import asyncio
import io
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
from bartjik_core import artwork


class Response(io.BytesIO):
    url = 'https://example.test/cover.png'
    headers = {}


class ArtworkTests(unittest.IsolatedAsyncioTestCase):
    async def asyncSetUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.cache = artwork.ArtworkCache(self.temp.name)
        self.addAsyncCleanup(self.cache.close)

    async def test_dedup_and_persistent_hit_without_network(self):
        image = b'\x89PNG\r\n\x1a\n' + b'fixture'
        with patch.object(artwork, 'urlopen', side_effect=lambda *a, **k: Response(image)) as fetch:
            a, b = await asyncio.gather(self.cache.get(Response.url), self.cache.get(Response.url))
            self.assertEqual(a, b)
            self.assertEqual(fetch.call_count, 1)
            restarted = artwork.ArtworkCache(self.temp.name)
            self.assertEqual(await restarted.get(Response.url), a)
            self.assertEqual(fetch.call_count, 1)
            await restarted.close()

    async def test_failed_and_oversized_downloads_never_enter_cache(self):
        for data in (b'<html>error</html>', b'\x89PNG\r\n\x1a\n' + b'x' * 33):
            self.cache.retry.clear()
            with patch.object(artwork, 'MAX_IMAGE', 32), patch.object(artwork, 'urlopen', return_value=Response(data)) as fetch:
                with self.assertRaises(ValueError):
                    await self.cache.get(Response.url)
                with self.assertRaisesRegex(ValueError, 'cooling down'):
                    await self.cache.get(Response.url)
                self.assertEqual(fetch.call_count, 1)
                self.assertEqual(list(Path(self.temp.name).iterdir()), [])

    async def test_eviction_preserves_recent_hits(self):
        with patch.object(artwork, 'MAX_ENTRIES', 2), patch.object(artwork, 'urlopen', side_effect=lambda *a, **k: Response(b'GIF89a' + b'x' * 16)):
            a = await self.cache.get(Response.url + '?a')
            b = await self.cache.get(Response.url + '?b')
            await self.cache.get(Response.url + '?a')
            c = await self.cache.get(Response.url + '?c')
            self.assertTrue(Path(a.removeprefix('file://')).exists())
            self.assertFalse(Path(b.removeprefix('file://')).exists())
            self.assertTrue(Path(c.removeprefix('file://')).exists())

    async def test_rejects_unsupported_urls(self):
        with patch.object(artwork, 'urlopen') as fetch:
            for url in ('file:///tmp/image', 'ftp://example.test/a', 'https://user:pass@example.test/a', 'bad', None):
                with self.assertRaises(ValueError):
                    await self.cache.get(url)
            fetch.assert_not_called()
