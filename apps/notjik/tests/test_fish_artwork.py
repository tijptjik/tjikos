import json
from pathlib import Path
import shutil
import subprocess
import unittest

ROOT = Path(__file__).resolve().parents[1]


@unittest.skipUnless(shutil.which('bun'), 'Bun required for QML JavaScript checks')
class FishArtworkTests(unittest.TestCase):
    def test_random_draw_history_and_sender_isolation(self):
        source = (ROOT / 'qml/Logic.js').read_text()
        assertions = r'''
const assert = require('node:assert/strict');
const seen = new Set();
for (const app of ['fish', 'Fish', 'fish.desktop']) assert.ok(fishNotice({app}));
assert.ok(fishNotice({desktopEntry:'fish.desktop'}));
for (const app of ['kitty', 'fishing', 'shellfish', '']) assert.ok(!fishNotice({app}));
for (let i=0; i<fishFiles.length; ++i) {
    Math.random = () => (i + 0.5) / fishFiles.length;
    const n = {app:'fish', id:'one', received:123};
    assignFish(n, null);
    assert.equal(n.fishIndex, i);
    seen.add(fishArtwork(n));
    const updated = {...n, urgency:'critical', body:'Changed'};
    Math.random = () => {throw Error('Updates must not reroll');};
    assignFish(updated, n);
    assert.equal(fishArtwork(updated), fishArtwork(n));
    assert.equal(fishArtwork(JSON.parse(JSON.stringify(n))), fishArtwork(n));
    const preview = {app:'fish', data:{demo:true, fishIndex:i}};
    assignFish(preview, null);
    assert.equal(preview.fishIndex, i);
}
assert.equal(seen.size, fishFiles.length);
const kitty = {app:'kitty'};
assignFish(kitty, null);
assert.equal(kitty.fishIndex, undefined);
for (const fishIndex of [undefined, -1, fishFiles.length, 1.5, '3', null]) {
    const old = {app:'fish', id:'old', received:123, fishIndex};
    assert.ok(seen.has(fishArtwork(old)));
    assert.equal(fishArtwork(old), fishArtwork({...old}));
}
Math.random = () => 0;
const normal = {app:'fish',data:{fishIndex:12}};
assignFish(normal,null);
assert.equal(normal.fishIndex,0);
assert.equal(fishArtwork({fishIndex:4}), 'icons/fish/fish-04-jade-koi.png');
assert.equal(fishArtwork({fishIndex:36}), 'icons/fish/fish-28-catfish.png');
const removed = {app:'fish', fishIndex:3, id:'removed', received:123};
assert.ok(seen.has(fishArtwork(removed)));
const migrated = {app:'fish'};
assignFish(migrated, removed);
assert.equal(fishArtwork(migrated), fishArtwork(removed));
assert.equal(migrated.fishName, fishNames[migrated.fishIndex]);
console.log(JSON.stringify(fishFiles));
'''
        result = subprocess.run(['bun', '-e', source + assertions], check=True, text=True, capture_output=True)
        files = json.loads(result.stdout)
        directory = ROOT / 'qml/icons/fish'
        manifest = json.loads((directory / 'manifest.json').read_text())
        self.assertEqual(files, [item['file'] for item in manifest['fish']])
        self.assertEqual(len(files), 28)
        for item in manifest['fish']:
            self.assertTrue((directory / item['file']).is_file())
            self.assertGreaterEqual(min(item['width'], item['height']), 1024)
        shell = (ROOT / 'qml/shell.qml').read_text()
        self.assertIn('fishIndex: n.fishIndex', shell)
        self.assertIn('fishName: n.fishName', shell)
        self.assertIn('Logic.assignFish(notice,', shell)
