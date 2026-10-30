"""Render the production icon font loader with system Material Symbols excluded."""

# Test-only comparison oracle; production launches the Rust binary.
import sys as _sys
from pathlib import Path as _Path
_sys.path.insert(0, str(_Path(__file__).resolve().parent / "reference"))
_sys.path.insert(0, str(_Path(__file__).resolve().parent / "reference/qml"))
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


@unittest.skipUnless(shutil.which("quickshell") and shutil.which("fc-match"),
                     "Quickshell and Fontconfig are required")
class BundledFontTests(unittest.TestCase):
    def test_font_loads_and_shapes_icons_without_system_installation(self):
        with tempfile.TemporaryDirectory(prefix="bartjik-font-") as directory:
            work = Path(directory)
            fonts = work / "fonts"
            fonts.mkdir()
            fallback = subprocess.check_output(
                ["fc-match", "-f", "%{file}", "sans-serif"], text=True
            ).strip()
            shutil.copyfile(fallback, fonts / "fallback.ttf")
            fontconfig = work / "fonts.conf"
            fontconfig.write_text(f'''<?xml version="1.0"?>
<!DOCTYPE fontconfig SYSTEM "urn:fontconfig:fonts.dtd">
<fontconfig><dir>{fonts}</dir><cachedir>{work / 'font-cache'}</cachedir></fontconfig>
''')
            fixture = '''
import QtQuick
import QtQuick.Window
import Quickshell
import Quickshell.Io
import "PRODUCTION" as Production
ShellRoot {
    id: root
    property var bundled: null
    property bool baselineAbsent: false
    property bool captured: false
    property int ticks: 0
    Component { id: factory; Production.IconFonts {} }
    FileView { id: report; path: Qt.resolvedUrl("result.json").toString().replace("file://", ""); blockWrites: true }
    TextMetrics { id: metrics; font.family: root.bundled ? root.bundled.materialSymbols.name : "sans-serif"; font.pixelSize: 48; text: "desktop_windows" }
    Window {
        id: preview
        visible: true; width: 360; height: 100; color: "#191724"
        Row {
            anchors.centerIn: parent; spacing: 24
            Repeater {
                model: ["settings", "headphones", "desktop_windows", "lan"]
                Text { required property string modelData; text: modelData; color: "#f4b7b5"; font.pixelSize: 48; font.family: metrics.font.family }
            }
        }
    }
    Timer {
        interval: 50; running: true; repeat: true
        onTriggered: {
            if (root.captured) return;
            if (++root.ticks > 100) { console.error("Font fixture timed out"); Qt.quit(); return; }
            if (!root.bundled) {
                root.baselineAbsent = Qt.fontFamilies().indexOf("Material Symbols Rounded") < 0;
                if (!root.baselineAbsent) { console.error("System font leaked into isolated fixture"); Qt.quit(); return; }
                root.bundled = factory.createObject(root);
                return;
            }
            if (root.bundled.materialSymbols.status !== FontLoader.Ready) return;
            root.captured = true;
            preview.contentItem.grabToImage(result => {
                if (!result.saveToFile(Qt.resolvedUrl("icons.png").toString().replace("file://", ""))) {
                    console.error("Could not capture font fixture"); Qt.quit(); return;
                }
                report.setText(JSON.stringify({baselineAbsent: root.baselineAbsent,
                    family: root.bundled.materialSymbols.name,
                    source: root.bundled.materialSymbols.source.toString(),
                    ligatureWidth: metrics.advanceWidth}));
                Qt.quit();
            });
        }
    }
}
'''.replace("PRODUCTION", (ROOT / "qml").as_uri())
            (work / "shell.qml").write_text(fixture)
            env = dict(os.environ, FONTCONFIG_FILE=str(fontconfig), FONTCONFIG_PATH=str(work),
                       QT_QPA_PLATFORM="offscreen", QT_QUICK_BACKEND="software",
                       XDG_RUNTIME_DIR=str(work), XDG_CACHE_HOME=str(work / "cache"),
                       XDG_CONFIG_HOME=str(work / "config"))
            result = subprocess.run(["quickshell", "--path", str(work), "--no-color"],
                                    env=env, text=True, capture_output=True, timeout=20)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertTrue((work / "result.json").exists(), result.stdout + result.stderr)
            data = json.loads((work / "result.json").read_text())
            self.assertTrue(data["baselineAbsent"])
            self.assertEqual(data["family"], "Material Symbols Rounded")
            self.assertEqual(data["source"], (ROOT / "qml/fonts/MaterialSymbolsRounded.ttf").as_uri())
            self.assertGreater(data["ligatureWidth"], 10)
            self.assertLess(data["ligatureWidth"], 72)
            self.assertTrue((work / "icons.png").read_bytes().startswith(b"\x89PNG"))
            artifacts = os.environ.get("BARTJIK_FONT_ARTIFACTS")
            if artifacts:
                target = Path(artifacts)
                target.mkdir(parents=True, exist_ok=True)
                for name in ("icons.png", "result.json"):
                    shutil.copyfile(work / name, target / name)
