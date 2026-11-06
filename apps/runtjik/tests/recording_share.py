"""Synthetic recording actions and compositor requests; never open real apps."""
import json
import os
from pathlib import Path
import subprocess
import tempfile

APP = Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix='runtjik-recording-share-') as directory:
    root = Path(directory)
    bindir = root / 'bin'
    bindir.mkdir()
    launch = '''#!/usr/bin/python3
import json, os, pathlib, sys
pathlib.Path(os.environ['SHARE_ARGS']).write_text(json.dumps(sys.argv[1:]))
pathlib.Path(os.environ['VIEW_PID']).write_text(str(os.getpid()))
'''
    for app in ('telegram-desktop', 'qvlc', 'vlc'):
        (bindir / app).write_text(launch)
    (bindir / 'hyprctl').write_text('''#!/usr/bin/python3
import json, os, pathlib, sys
args = sys.argv[1:]
with pathlib.Path(os.environ['REQUESTS']).open('a') as log:
    log.write(json.dumps(args) + '\\n')
if args == ['-j', 'clients']:
    count = pathlib.Path(os.environ['POLL_COUNT'])
    polls = int(count.read_text()) if count.exists() else 0
    count.write_text(str(polls + 1))
    clients = [] if polls < int(os.environ['STARTUP_POLLS']) else [
        dict(address='0xdef', monitor=1, workspace={'name':'special:chat'},
             **{'class':'org.telegram.desktop', 'title':'Media viewer', 'focusHistoryID':0}),
        dict(address='0xabc', monitor=1, workspace={'name':os.environ['WORKSPACE']},
             **{'class':'org.telegram.desktop._account', 'title':'Telegram', 'focusHistoryID':1})]
    if clients and os.environ['ACTION'] == 'view-recording':
        clients[0].update(pid=1, **{'class':'vlc', 'title':'Unrelated existing playback'})
        pid = pathlib.Path(os.environ['VIEW_PID'])
        clients[1].update(pid=int(pid.read_text()) if pid.exists() else 0, **{'class':'vlc'})
    print(json.dumps(clients))
elif args == ['-j', 'activewindow']:
    print(json.dumps({'address':'0xabc'}))
elif args == ['-j', 'activeworkspace']:
    print(json.dumps({'tiledLayout':'scrolling'}))
elif args == ['-j', 'monitors']:
    print(json.dumps([dict(id=1, name='TEST', specialWorkspace={'name':os.environ['VISIBLE_SPECIAL']})]))
else:
    print('ok')
''')
    for path in bindir.iterdir():
        path.chmod(0o700)
    video = root / "recording with spaces ' and ;.mp4"
    video.write_bytes(b'synthetic video')
    cases = [('hidden', 'special:chat', '', 0),
             ('visible', 'special:chat', 'special:chat', 0),
             ('normal', '4', 'special:other', 0),
             ('cold', 'special:chat', '', 3)]
    for action, name, workspace, visible, startup in (
        (action, *case) for action in ('share-recording-telegram', 'view-recording') for case in cases
    ):
        key = action + '-' + name
        requests = root / (key + '.requests')
        shared = root / (key + '.shared')
        env = {**os.environ, 'PATH': str(bindir),
               'HYPRLAND_INSTANCE_SIGNATURE': 'synthetic', 'SHARE_ARGS': str(shared),
               'REQUESTS': str(requests), 'POLL_COUNT': str(root / (key + '.polls')),
               'VIEW_PID': str(root / (key + '.pid')), 'ACTION': action,
               'WORKSPACE': workspace, 'VISIBLE_SPECIAL': visible, 'STARTUP_POLLS': str(startup)}
        subprocess.run([str(APP / 'runtjik-core'), action, str(video)],
                       env=env, check=True, timeout=8)
        expected_args = ['-sendpath', str(video)] if action == 'share-recording-telegram' else ['--no-one-instance', '--', str(video)]
        assert json.loads(shared.read_text()) == expected_args
        commands = [json.loads(line) for line in requests.read_text().splitlines()]
        dispatches = [args[1] for args in commands if args[0] == 'dispatch']
        assert dispatches[-2] == 'hl.dsp.focus({ window = "address:0xabc" })', dispatches
        assert dispatches[-1] == 'hl.dsp.layout("center")', dispatches
        toggles = [command for command in dispatches if 'toggle_special' in command]
        expected = [] if name == 'visible' else ['other' if name == 'normal' else 'chat']
        assert toggles == [f'hl.dsp.workspace.toggle_special("{value}")' for value in expected], toggles
        print('PASS:', action, name)
    (bindir / 'qvlc').unlink()
    subprocess.run([str(APP / 'runtjik-core'), 'view-recording', str(video)], env=env, check=True, timeout=8)
    assert json.loads(shared.read_text()) == ['--intf', 'qt', '--no-one-instance', '--', str(video)]
    print('PASS: VLC explicit Qt fallback')
