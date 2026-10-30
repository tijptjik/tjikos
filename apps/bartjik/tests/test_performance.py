"""Exercise snapshot bindings in the real Qt engine, without desktop services.

For a longer synthetic allocation run:
    python3 tests/test_performance.py --soak 2400 [--logic /path/to/Logic.js]
"""

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
import sys
import tempfile
import time
import unittest

ROOT = Path(__file__).resolve().parents[1]


def exercise(rounds=100, logic=None):
    with tempfile.TemporaryDirectory(prefix='bartjik-perf-') as directory:
        temp = Path(directory)
        source = Path(logic or ROOT / 'qml/Logic.js').read_text()
        optimized = 'function reconcile(' in source
        source = 'var styleCalls=0; var styleChanges=0;\n' + source.replace(
            'function partStyle(theme, module, item, part, hovered) {',
            'function partStyle(theme, module, item, part, hovered) { ++styleCalls;')
        (temp / 'Logic.js').write_text(source)
        # No real configuration, provider payloads or services enter this fixture.
        fixture = dict(bars=[dict(style=dict(color='white', fontSize=14,
            states=dict(hover=dict(color='pink'))), moduleConfigs=dict(clock={}))],
            providers=dict(items=[dict(id=str(i), state={}, text=dict(value='demo', style={}))
                                  for i in range(80)]))
        (temp / 'shell.qml').write_text('''
import QtQuick
import Quickshell
import Quickshell.Io
import "Logic.js" as Logic
ShellRoot {
    id:root
    property var state: FIXTURE
    property int ticks:0
    property double lastTick:Date.now()
    property double longestTick:0
    property int warmCalls:0
    property int warmChanges:0
    FileView {id:result;path:Qt.resolvedUrl('result.json').toString().replace('file://','');blockWrites:true}
    Item {
        Repeater {
            model:80
            Item {
                required property int index
                readonly property var bar:root.state.bars[0]
                readonly property var row:root.state.providers.items[index]
                readonly property var style:Logic.partStyle(bar.style,bar.moduleConfigs.clock,row,'text',false)
                onStyleChanged:++Logic.styleChanges
            }
        }
    }
    Timer {
        interval:1;running:true;repeat:true
        onTriggered:{
            const now=Date.now();root.longestTick=Math.max(root.longestTick,now-root.lastTick);root.lastTick=now;
            const next=JSON.parse(JSON.stringify(root.state));
            next.providers.items[0].text.value=String(root.ticks);
            root.state=UPDATE;
            if(++root.ticks===10){root.warmCalls=Logic.styleCalls;root.warmChanges=Logic.styleChanges;}
            if(root.ticks===ROUNDS){
                if(root.state.providers.items[0].text.value!==String(ROUNDS-1))throw Error('stale clock');
                result.setText(JSON.stringify({ticks:root.ticks,styleCalls:Logic.styleCalls-root.warmCalls,
                    styleChanges:Logic.styleChanges-root.warmChanges,longestTick:root.longestTick}));
                Qt.quit();
            }
        }
    }
}
'''.replace('FIXTURE', json.dumps(fixture)).replace('UPDATE',
            'Logic.reconcile(root.state,next)' if optimized else 'next').replace('ROUNDS', str(rounds)))
        env = {**os.environ, 'QT_QPA_PLATFORM': 'offscreen', 'QT_QUICK_BACKEND': 'software',
               'XDG_RUNTIME_DIR': str(temp)}
        samples = []
        start = time.monotonic()
        with (temp / 'log').open('w') as log:
            process = subprocess.Popen(['quickshell', '--path', str(temp), '--no-color'],
                                       stdout=log, stderr=log, env=env)
            try:
                while process.poll() is None:
                    if time.monotonic() - start > max(30, rounds * .08):
                        raise TimeoutError('Qt snapshot stress run timed out')
                    try:
                        status = Path(f'/proc/{process.pid}/status').read_text()
                        rss = next(int(line.split()[1]) for line in status.splitlines()
                                   if line.startswith('VmRSS:'))
                        samples.append(rss)
                    except (FileNotFoundError, StopIteration):
                        pass
                    time.sleep(.2)
                if process.returncode or not (temp / 'result.json').exists():
                    raise AssertionError((temp / 'log').read_text())
                output = json.loads((temp / 'result.json').read_text())
                output.update(elapsed=round(time.monotonic()-start, 2),
                              rssKiB=samples, optimized=optimized)
                return output
            finally:
                if process.poll() is None:
                    process.terminate()
                    process.wait(timeout=5)


@unittest.skipUnless(shutil.which('quickshell'), 'Quickshell is required')
class PerformanceTests(unittest.TestCase):
    def test_provider_updates_do_not_restyle_unchanged_items(self):
        result = exercise()
        # Only the changed clock row may recompute; its style remains identical.
        self.assertLessEqual(result['styleCalls'], 100, result)
        self.assertEqual(result['styleChanges'], 0, result)

    def test_crypto_retention_is_bounded_with_lost_and_late_replies(self):
        with tempfile.TemporaryDirectory(prefix='bartjik-crypto-retention-') as directory:
            temp = Path(directory)
            source = (ROOT / 'qml/CryptoPanel.qml').read_text()
            retention = 'property var cachedCharts:' + source.split('property var cachedCharts:', 1)[1].split('property var theme:', 1)[0]
            (temp / 'shell.qml').write_text('''
import QtQuick
import Quickshell
import Quickshell.Io
ShellRoot {
    QtObject {id:panel;property var chart:({});RETENTION}
    FileView {id:result;path:Qt.resolvedUrl('result.txt').toString().replace('file://','');blockWrites:true}
    Timer {interval:1;running:true;onTriggered:{
        for(let i=0;i<1000;++i)panel.remember(panel.requests,'request-'+i,'asset-'+i);
        if(Object.keys(panel.requests).length!==64)throw Error('unbounded requests');
        for(let i=0;i<64;++i)panel.remember(panel.cachedCharts,'cached-'+i,{chart:{},requestedAt:1});
        panel.receive('request-0',{points:[]});
        if(Object.keys(panel.cachedCharts).length!==64)throw Error('evicted request revived');
        for(let i=936;i<1000;++i)panel.receive('request-'+i,{points:[]});
        if(Object.keys(panel.requests).length!==0)throw Error('completed tokens retained');
        if(Object.keys(panel.cachedCharts).length!==64)throw Error('late replies grew cache');
        result.setText('PASS');Qt.quit();
    }}
}
'''.replace('RETENTION', retention))
            result = subprocess.run(['quickshell', '--path', str(temp), '--no-color'],
                env={**os.environ, 'QT_QPA_PLATFORM': 'offscreen', 'QT_QUICK_BACKEND': 'software',
                     'XDG_RUNTIME_DIR': str(temp)},
                capture_output=True, text=True, timeout=15)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertEqual((temp / 'result.txt').read_text(), 'PASS')


if __name__ == '__main__':
    if '--soak' in sys.argv:
        rounds = int(sys.argv[sys.argv.index('--soak') + 1])
        logic = sys.argv[sys.argv.index('--logic') + 1] if '--logic' in sys.argv else None
        print(json.dumps(exercise(rounds, logic)), flush=True)
    else:
        unittest.main()
