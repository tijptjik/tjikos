import unittest
from image_viewer import lightbox_size


class LightboxTests(unittest.TestCase):
    def test_portrait_fits_height(self):
        self.assertEqual(lightbox_size(600, 1200, 1920, 1080), (486, 972))

    def test_wide_image_has_matching_pixel_margins(self):
        width, height = lightbox_size(4000, 1000, 1920, 1080)
        self.assertEqual((1920 - width) / 2, (1080 - height) / 2)
        self.assertEqual(height, 972)

    def test_square_on_scaled_screen(self):
        self.assertEqual(lightbox_size(100, 100, 1280, 720), (648, 648))
