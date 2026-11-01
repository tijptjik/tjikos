"""Render and exercise crypto failure/recovery with synthetic data on a private bus."""

import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time

from render import wait_for

ROOT = Path(__file__).resolve().parents[1]


def main():
    assert os.environ.get('DBUS_SESSION_BUS_ADDRESS', '').startswith('unix:path=/tmp/')
    destination = Path(sys.argv[1]).resolve()
    destination.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='bartjik-crypto-retry-') as directory:
        temp = Path(directory)
        shutil.copytree(ROOT / 'qml', temp / 'qml')
        # This test exercises only crypto; do not load the shared notification app.
        (temp / 'qml/NotificationHistory.qml').write_text(
            'import QtQuick\nItem {required property var popup;property bool needsKeyboard:false}\n')
        shell = temp / 'qml/shell.qml'
        source = shell.read_text().replace('id: root', '''id: root
    property bool fixtureFrozen:false
    property var fixtureReady:({})
    property string fixtureSource:''
    property int fixtureRetries:0
    function fixtureState(value) {
        const next=JSON.parse(JSON.stringify(state));
        next.providers[fixtureSource]=value;state=next;
    }
    function retryControl(item) {
        if(item.objectName==='cryptoRetry')return item;
        for(const child of item.children || []){const found=retryControl(child);if(found)return found;}
        return null;
    }
''', 1)
        source = source.replace('function send(message) {', '''function send(message) {
        if(message.op==='cryptoRetry'){
            ++fixtureRetries;
            fixtureState(Object.assign({},state.providers[fixtureSource],{state:'refreshing'}));
            return;
        }
''')
        source = source.replace('else root.state=Logic.reconcile(root.state,msg);',
                                'else if(!fixtureFrozen)root.state=Logic.reconcile(root.state,msg);')
        source = source.replace("case 'status':break;", '''case 'failureFixture': {
            const b=bars.find(b=>b.moduleConfigs.prices),source=b.sources.prices;
            fixtureSource=source;fixtureReady=JSON.parse(JSON.stringify(state.providers[source]));fixtureFrozen=true;
            fixtureState({state:'unavailable',error:'API unavailable',polls:1,items:[{id:'unavailable',text:{value:'—'},state:{unavailable:true}}]});
            return {ok:true};
        }
        case 'retryFixture': {
            const button=retryControl(cryptoPanel.panelBody);
            if(req.click && button.visible && button.enabled)button.clicked();
            return {ok:true,visible:button.visible,width:button.width,showLabel:button.showLabel,symbol:button.symbol,enabled:button.enabled,text:button.text,retries:fixtureRetries,loading:!!cryptoPanel.chart.loading,
                error:cryptoPanel.providerState.error,pair:cryptoPanel.pair,chartRequests:Object.keys(cryptoPanel.requests).length};
        }
        case 'retryFailure':fixtureState(Object.assign({},state.providers[fixtureSource],{state:'unavailable',polls:2}));return {ok:true};
        case 'retryRecovery':fixtureState(fixtureReady);return {ok:true};
        case 'status':break;''')
        shell.write_text(source)
        for name in ('Part.qml', 'CryptoPanel.qml', 'PreviewHover.qml'):
            path = temp / 'qml' / name
            path.write_text(path.read_text().replace('HoverHandler {', 'HoverHandler {enabled:false;'))
        cfg = json.loads((ROOT / 'examples/comparison.json').read_text())
        for bar in cfg['bars']:
            bar.update(monitors=[1], autoHide=False)
        config = temp / 'config.json'
        config.write_text(json.dumps(cfg))
        env = {**os.environ, 'BARTJIK_QML': str(temp / 'qml'), 'BARTJIK_CONFIG': str(config)}

        def ipc(request):
            result = subprocess.run(['quickshell', 'ipc', '--path', str(temp / 'qml'),
                                     'call', 'bartjik', 'request', json.dumps(request)],
                                    env=env, capture_output=True, text=True, timeout=5)
            if result.returncode:
                raise OSError(result.stderr)
            value = json.loads(result.stdout)
            assert value['ok'], value
            return value

        with (destination / 'render.log').open('w') as log:
            process = subprocess.Popen([str(ROOT / 'bin/bartjik'), '--demo'], env=env, stdout=log, stderr=log)
            try:
                wait_for(lambda: ipc({'op': 'status'}).get('ready'))
                ipc({'op': 'cryptoPreview', 'module': 'prices', 'item': 'BTC/USD'})
                state = ipc({'op': 'retryFixture'})
                assert not state['visible'] and state['width'] == 0, state
                ipc({'op': 'failureFixture'})
                ipc({'op': 'cryptoPreview', 'module': 'prices', 'item': 'unavailable'})
                state = ipc({'op': 'retryFixture'})
                assert state['pair'] == 'Crypto' and state['text'] == 'Retry', state
                assert state['visible'] and not state['showLabel'] and state['symbol'] == 'refresh', state
                assert not state['loading'] and state['chartRequests'] == 0, state
                assert state['error'] == 'API unavailable', state
                time.sleep(.3)
                ipc({'op': 'captureCrypto', 'path': str(destination / 'unavailable.png')})
                state = ipc({'op': 'retryFixture', 'click': True})
                assert not state['enabled'] and state['text'] == 'Retrying…', state
                assert ipc({'op': 'retryFixture', 'click': True})['retries'] == 1
                ipc({'op': 'retryFailure'})
                assert ipc({'op': 'retryFixture'})['enabled']
                assert ipc({'op': 'retryFixture', 'click': True})['retries'] == 2
                ipc({'op': 'retryRecovery'})
                wait_for(lambda: ipc({'op': 'cryptoState'})['points'] == 288)
                state = ipc({'op': 'retryFixture'})
                assert state['pair'] == 'BTC/USD' and not state['visible'] and state['width'] == 0, state
                assert not state['loading'] and not state['error'], state
                time.sleep(.3)
                ipc({'op': 'captureCrypto', 'path': str(destination / 'recovered.png')})
                wait_for(lambda: all((destination / name).exists() for name in ('unavailable.png', 'recovered.png')))
            finally:
                process.terminate()
                process.wait(timeout=5)
        errors = [line for line in (destination / 'render.log').read_text().splitlines()
                  if any(term in line for term in ('ERROR', 'TypeError', 'ReferenceError', 'Binding loop'))]
        assert not errors, errors
    print('PASS: unavailable panel, retry button, repeated failure, duplicate suppression, recovery and chart render')


if __name__ == '__main__':
    main()
