import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('tray_attention', Path(__file__).resolve().parents[1] / 'qml/tray_attention.py')
tray = importlib.util.module_from_spec(spec)
spec.loader.exec_module(tray)


class TrayAttentionTests(unittest.TestCase):
    def test_telegram_active_attention_and_clear(self):
        self.assertTrue(tray.unread('TelegramDesktop', {'Status':'Active','IconName':'org.telegram.desktop-attention-symbolic'}))
        self.assertFalse(tray.unread('TelegramDesktop', {'Status':'Active','IconName':'org.telegram.desktop-symbolic'}))
        self.assertTrue(tray.unread('TelegramDesktop', {'Status':'NeedsAttention'}))

    def test_zapzap_badge_not_logo_or_malformed_pixels(self):
        red = bytes([255,255,0,0]) * 64
        green = bytes([255,0,220,60]) * 64
        def props(pixels):
            return {'IconPixmap':[(8,8,pixels)]}
        self.assertTrue(tray.unread('ZapZap', props(red)))
        self.assertFalse(tray.unread('ZapZap', props(green)))
        self.assertFalse(tray.unread('Other', props(red)))
        self.assertFalse(tray.unread('ZapZap', props(red[:4])))
        self.assertFalse(tray.unread('ZapZap', props(bytes([0,255,0,0])*64)))
        self.assertFalse(tray.unread('ZapZap', props(red[:4]+green[4:])))
