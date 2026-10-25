"""Synthetic renderer and asynchronous bridge integration; no Telegram account access."""
import gzip
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
import telegram_bridge as telegram


class RenderTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        if not shutil.which('ffmpeg'):
            raise unittest.SkipTest('FFmpeg is required for media rendering')
        runtime = Path.home()/'.local/share/notjik/telegram-venv/bin/python'
        cls.python = sys.executable if importlib.util.find_spec('rlottie_python') else str(runtime)
        if not Path(cls.python).exists():
            raise unittest.SkipTest('Install the optional Telegram runtime for renderer tests')

    def test_real_conversions(self):
        with tempfile.TemporaryDirectory() as temp:
            for filename,format,signature in (('demo-sticker.webp','webp',b'\x89PNG'),('demo-sticker.tgs','tgs',b'GIF'),('demo-video-sticker.webm','webm',b'GIF')):
                target=Path(temp)/format
                subprocess.run([self.python,str(ROOT/'telegram_media_render.py'),str(ROOT/'examples'/filename),str(target),format],check=True,timeout=40)
                self.assertTrue(target.read_bytes().startswith(signature))
                self.assertGreater(target.stat().st_size,100)

    def test_external_tgs_assets_rejected(self):
        with tempfile.TemporaryDirectory() as temp:
            source=Path(temp)/'external.tgs';target=Path(temp)/'output.gif'
            with gzip.open(ROOT/'examples/demo-sticker.tgs','rb') as stream: data=json.load(stream)
            data['assets']=[{'id':'external','u':'/etc/','p':'passwd'}]
            with gzip.open(source,'wb') as stream: stream.write(json.dumps(data).encode())
            result=subprocess.run([self.python,str(ROOT/'telegram_media_render.py'),str(source),str(target),'tgs'],capture_output=True,timeout=40)
            self.assertNotEqual(result.returncode,0)
            self.assertFalse(target.exists())

    def test_bridge_converts_off_thread_and_ignores_stale_card(self):
        with tempfile.TemporaryDirectory() as temp, patch.dict(os.environ,{'NOTJIK_TELEGRAM_DIR':temp}):
            from test_telegram import FakeTD
            cards=[]; bridge=telegram.Bridge(FakeTD(),lambda n: cards.append(json.loads(json.dumps(n))),lambda n:None,clock=lambda:1000)
            self.addCleanup(bridge.media_pool.shutdown,wait=True,cancel_futures=True)
            path=Path(temp)/'files/sticker.tgs';path.parent.mkdir();shutil.copy2(ROOT/'examples/demo-sticker.tgs',path)
            message={'id':42,'chat_id':123,'date':1000,'content':{'@type':'messageSticker','sticker':{'format':{'@type':'stickerFormatTgs'},'sticker':{'id':5,'size':path.stat().st_size,'local':{'is_downloading_completed':True,'path':str(path)}}}}}
            with patch.object(telegram.sys,'executable',self.python):
                bridge.add(1,123,{'id':1,'date':1000,'type':{'@type':'notificationTypeNewMessage','show_preview':True,'message':message}})
                self.assertTrue(bridge.media_jobs)
                deadline=time.monotonic()+40
                while bridge.media_jobs and time.monotonic()<deadline:
                    bridge.poll_media();time.sleep(.05)
            self.assertFalse(bridge.media_jobs)
            self.assertEqual(cards[-1]['data']['format'],'gif')
            self.assertTrue(cards[-1]['data']['source'].startswith((Path(temp)/'rendered').as_uri()))
            rendered=list((Path(temp)/'rendered').glob('*.gif'))[0]
            self.assertEqual(rendered.stat().st_mode&0o777,0o600)
            os.utime(rendered,(0,0));bridge.prune_render_cache()
            self.assertTrue(rendered.exists(),'Active media must stay cached')
            bridge.remove(next(iter(bridge.entries)));bridge.prune_render_cache()
            self.assertFalse(rendered.exists(),'Old unreferenced derivatives must expire')


if __name__ == '__main__': unittest.main()
