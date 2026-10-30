"""Native-only gallery capture contracts; no live desktop or accounts used."""

# Test-only comparison oracle; production launches the Rust binary.
import sys as _sys
from pathlib import Path as _Path
_sys.path.insert(0, str(_Path(__file__).resolve().parent / "reference"))
_sys.path.insert(0, str(_Path(__file__).resolve().parent / "reference/qml"))
import contextlib
import importlib.util
import io
import json
from pathlib import Path
import shutil
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT=Path(__file__).resolve().parents[3]
sys.path.insert(0,str(ROOT/"scripts"))
import bartjik_showcase as film
sys.path.insert(0,str(ROOT/"apps/bartjik"))
from bartjik_core import config


class ShowcaseTests(unittest.TestCase):
    def test_plan_requires_no_desktop(self):
        with (patch.object(sys,"argv",["showcase","--plan"]),
              patch.object(film,"record") as record,
              contextlib.redirect_stdout(io.StringIO()) as output):
            film.main()
        record.assert_not_called()
        self.assertFalse(json.loads(output.getvalue())["comparison"])

    def test_existing_capture_or_evidence_is_preserved(self):
        with tempfile.TemporaryDirectory() as directory:
            for suffix in (".mp4",".json",".review"):
                output=Path(directory)/(suffix[1:]+".mp4")
                keep=output.with_suffix(suffix)
                if suffix==".review":keep.mkdir();keep=keep/"keep"
                keep.write_text("original")
                with (self.subTest(suffix=suffix),patch.object(sys,"argv",["showcase",str(output)]),
                      patch.object(film,"record") as record,contextlib.redirect_stderr(io.StringIO()),self.assertRaises(SystemExit)):
                    film.main()
                record.assert_not_called()
                self.assertEqual(keep.read_text(),"original")

    def test_private_copy_keeps_native_handlers_and_pointer_observer(self):
        source=ROOT/"apps/bartjik/qml"
        before={p.relative_to(source):p.read_bytes() for p in source.glob("**/*.qml")}
        with tempfile.TemporaryDirectory() as directory:
            app=Path(directory)
            shutil.copytree(source,app/"qml")
            film.prepare(app)
            text="\n".join(p.read_text() for p in (app/"qml").glob("**/*.qml"))
            self.assertNotIn("import QtTest",text)
            self.assertNotIn("TestEvent",text)
            drag=(app/"qml/DetachablePopup.qml").read_text()
            self.assertIn("onReleased:mouse=>host.endDrag(Qt.point(mouse.x,mouse.y))",drag)
            self.assertIn("host.moveDrag(point);host.showcaseMove(point);",drag)
            self.assertEqual((app/"qml/DetachedWindow.qml").read_bytes(),before[Path("DetachedWindow.qml")])
            shell=(app/"qml/shell.qml").read_text()
            self.assertIn("root.pointer=msg.pointer;root.pointerUpdated=Date.now();",shell)
            path=app/"config.json";path.write_text(json.dumps(film.configuration("HEADLESS-1")))
            loaded=config.load(path)
            self.assertEqual(len(loaded["bars"]),2)
            self.assertEqual([b["id"] for b in loaded["bars"]],["workspace-bar","status-bar"])
            self.assertEqual(loaded["bars"][0]["monitors"],["HEADLESS-1"])
        self.assertEqual(before,{p.relative_to(source):p.read_bytes() for p in source.glob("**/*.qml")})


if __name__=="__main__":unittest.main()
