"""Real keyboard search in Bartjik's shared history view; synthetic demo only."""

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

from render import wait_for
ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT))
from bartjik import ipc


def main():
    assert os.environ.get('DBUS_SESSION_BUS_ADDRESS','').startswith('unix:path=/tmp/')
    destination=Path(sys.argv[1]).resolve()
    destination.mkdir(parents=True,exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='bartjik-history-') as td:
        temp=Path(td)
        shutil.copytree(ROOT/'qml',temp/'qml')
        for name in ('Part.qml','PreviewHover.qml','DetailPanel.qml'):
            p=temp/'qml'/name;p.write_text(p.read_text().replace('HoverHandler {','HoverHandler {enabled:false;'))
        p=temp/'qml/NotificationHistory.qml'
        s=p.read_text().replace('import QtQuick\n','import QtQuick\nimport QtTest\n',1)
        s=s.replace('    id:root','''    id:root
    TestEvent {id:testKeys}
    function testKey(key){
        if(key==='focus'){view.focusHistory();return;}
        if(key.length===1)testKeys.keyClickChar(key,Qt.NoModifier,0);
        else testKeys.keyClick(Qt['Key_'+key],Qt.NoModifier,0);
    }''',1)
        p.write_text(s)
        p=temp/'qml/DetailPanel.qml';s=p.read_text().replace('    id:popup',"    id:popup\n    function historyTest(key){notificationHistory.testKey(key);}",1);p.write_text(s)
        p=temp/'qml/shell.qml';s=p.read_text().replace("case 'status':break;","case 'historyKey':detailPanel.historyTest(req.key);return {ok:true};case 'status':break;")
        s=s.replace("if(msg.kind==='pointer'){root.pointer=msg.pointer;root.pointerUpdated=Date.now();}","if(msg.kind==='pointer'){}")
        p.write_text(s)
        cfg=json.loads((ROOT/'examples/laptop.json').read_text())
        for b in cfg['bars']:b.update(monitors=[1],autoHide=False)
        config=temp/'config.json';config.write_text(json.dumps(cfg))
        os.environ.update(BARTJIK_QML=str(temp/'qml'),BARTJIK_CONFIG=str(config))
        log_path=destination/'render.log'
        with log_path.open('w') as log:
            process=subprocess.Popen([str(ROOT/'bin/bartjik'),'--demo'],stdout=log,stderr=log)
            try:
                wait_for(lambda:ipc({'op':'status'}).get('ready'))
                ipc({'op':'detailTest','module':'notifications'})
                def state():return ipc({'op':'detailState'})['panel']
                def key(value):ipc({'op':'historyKey','key':value})
                wait_for(lambda:state()['opened'] and state()['notifications']['count']==3)
                key('focus');key('/')
                for char in 'CENTRAL':key(char)
                assert state()['notifications']['matches']==2,state()
                key('Tab');first=state()['notifications']['selected'];assert first
                key('Tab');assert state()['notifications']['selected']!=first
                key('Backtab');assert state()['notifications']['selected']==first
                time.sleep(.5)
                image=destination/'notifications-search.png'
                ipc({'op':'captureDetail','path':str(image)})
                wait_for(lambda:image.exists() and image.read_bytes().endswith(b'IEND\xaeB`\x82'))
                key('Escape');assert state()['notifications']['query']==''
                key('Escape');wait_for(lambda:not state()['opened'])
                ipc({'op':'detailTest','module':'notifications'})
                wait_for(lambda:state()['opened'] and state()['notifications']['count']==3)
                key('focus');key('/')
                for char in 'no match':key(char)
                assert state()['notifications']['matches']==0
                key('Tab');assert not state()['notifications']['selected']
            finally:
                process.terminate();process.wait(timeout=5)
        errors=[line for line in log_path.read_text().splitlines() if any(word in line for word in ('WARN scene','ERROR','ReferenceError','TypeError','Binding loop'))]
        assert not errors,errors[:12]
    print('Bartjik history: real search keys, grouped matches, selection, empty results, Escape and reopening passed')


if __name__=='__main__':main()
