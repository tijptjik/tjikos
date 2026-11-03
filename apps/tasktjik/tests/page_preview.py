"""Persistent preview cache checks; all artwork and requests are synthetic."""
import importlib.util
import json
import os
from pathlib import Path
import tempfile
import time
import unittest
from unittest.mock import patch


spec = importlib.util.spec_from_file_location(
    'page_preview', Path(__file__).resolve().parents[1] / 'qml/page_preview.py')
previews = importlib.util.module_from_spec(spec)
spec.loader.exec_module(previews)


class PreviewCacheTest(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix='tasktjik-preview-test-')
        self.addCleanup(self.directory.cleanup)
        self.environment = patch.dict(os.environ, {'XDG_CACHE_HOME': self.directory.name})
        self.environment.start()
        self.addCleanup(self.environment.stop)
        self.url = 'https://example.invalid/synthetic'

    def save(self, kind='page', age=30 * 86400):
        cache, key, receipt = previews.cache_paths(self.url)
        cache.mkdir(parents=True, exist_ok=True)
        result = {'previewVersion': 4}
        if kind:
            image = cache / (key + '.png')
            image.write_bytes(b'synthetic image fixture')
            result.update(image=image.as_uri(), imageKind=kind)
        else:
            result['unavailable'] = True
        receipt.write_text(json.dumps(result))
        os.utime(receipt, (time.time() - age, time.time() - age))
        return result, receipt

    def test_old_images_survive_reopening_without_fetching_or_rendering(self):
        for kind in ('page', 'metadata', 'favicon'):
            with self.subTest(kind=kind):
                saved, _ = self.save(kind)
                with patch.object(previews, 'fetch', side_effect=AssertionError('network')), \
                        patch.object(previews, 'render_page', side_effect=AssertionError('browser')):
                    self.assertEqual(previews.preview(self.url), saved)

    def test_missing_image_is_regenerated(self):
        saved, _ = self.save()
        Path(previews.unquote(previews.urlsplit(saved['image']).path)).unlink()
        self.assertIsNone(previews.cached_preview(self.url))

    def test_failures_retry_after_five_minutes(self):
        saved, _ = self.save(kind=None, age=0)
        self.assertEqual(previews.cached_preview(self.url), saved)
        self.save(kind=None, age=301)
        self.assertIsNone(previews.cached_preview(self.url))

    def test_invalid_receipts_are_misses(self):
        _, receipt = self.save()
        for content in ('{', '[]', '{"previewVersion":3}'):
            receipt.write_text(content)
            self.assertIsNone(previews.cached_preview(self.url))

    def test_all_cache_hits_arrive_before_first_render(self):
        saved, _ = self.save()
        uncached = 'https://example.invalid/slow'
        with patch.object(previews, 'preview', return_value={}) as render:
            batch = previews.preview_batch([uncached, self.url, self.url])
            self.assertEqual(next(batch), {'url': self.url, 'preview': saved})
            render.assert_not_called()
            self.assertEqual(list(batch), [{'url': uncached, 'preview': {}}])
            render.assert_called_once_with(uncached)


if __name__ == '__main__':
    unittest.main()
