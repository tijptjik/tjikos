"""Isolation and no-clobber contracts for the native gallery recorder."""
import contextlib
import importlib.util
import io
from pathlib import Path
import shutil
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[3]
SPEC = importlib.util.spec_from_file_location("os_showcase", ROOT / "scripts/os_showcase.py")
showcase = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(showcase)


class NativeShowcaseTests(unittest.TestCase):
    def test_plan_is_available_without_desktop_or_capture(self):
        with (patch.object(sys, "argv", ["showcase", "notjik", "--plan"]),
              patch.object(showcase, "record") as record,
              patch.object(showcase.shutil, "which", side_effect=AssertionError("desktop dependency")),
              contextlib.redirect_stdout(io.StringIO()) as output):
            showcase.main()
        self.assertIn("1200", output.getvalue())
        self.assertIn("720", output.getvalue())
        record.assert_not_called()

    def test_existing_video_receipt_or_review_is_not_overwritten(self):
        with tempfile.TemporaryDirectory() as directory:
            for suffix in (".mp4", ".json", ".review"):
                output = Path(directory) / (suffix[1:] + ".mp4")
                existing = output.with_suffix(suffix)
                if suffix == ".review":
                    existing.mkdir()
                    (existing / "keep").write_text("original")
                else:
                    existing.write_text("original")
                with (self.subTest(suffix=suffix),
                      patch.object(sys, "argv", ["showcase", "notjik", str(output)]),
                      patch.object(showcase, "record") as record,
                      contextlib.redirect_stderr(io.StringIO()),
                      self.assertRaises(SystemExit)):
                    showcase.main()
                record.assert_not_called()
                self.assertEqual((existing / "keep" if existing.is_dir() else existing).read_text(), "original")

    def test_instrumentation_preserves_source_and_real_input_handlers(self):
        source = ROOT / "apps/notjik/qml"
        before = {p.relative_to(source):p.read_bytes() for p in source.glob("**/*.qml")}
        with tempfile.TemporaryDirectory() as directory:
            app = Path(directory)
            shutil.copytree(source, app / "qml")
            showcase.prepare_notjik(app)
            instrumented = "\n".join(p.read_text() for p in (app / "qml").glob("**/*.qml"))
            for forbidden in ("import QtTest", "mouseClick(", "mouseMove(", "cycleText("):
                self.assertNotIn(forbidden, instrumented)
            # Keep production reply routing; inspection adds only a read-only alias.
            telegram = (app / "qml/components/Telegram.qml").read_text()
            self.assertIn('onClicked: detail.action("reply", reply.text)', telegram)
            self.assertIn('readonly property alias showcaseDraft: reply.text', telegram)
            self.assertIn("raise SystemExit", (app / "telegram_bridge.py").read_text())
            wrapper = (app / "telegram_recording.py").read_text()
            self.assertIn("showcase-voice.wav", wrapper)
            self.assertNotIn('"pulse"', wrapper)
            self.assertEqual((app / "showcase_recording_backend.py").read_bytes(),
                             (ROOT / "apps/notjik/telegram_recording.py").read_bytes())
            self.assertEqual((app / "showcase-voice.wav").read_bytes(),
                             (ROOT / "apps/notjik/examples/demo-voice.wav").read_bytes())
        after = {p.relative_to(source):p.read_bytes() for p in source.glob("**/*.qml")}
        self.assertEqual(before, after)


if __name__ == "__main__":
    unittest.main()
