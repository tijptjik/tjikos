"""Synthetic Telegram launch and compositor requests; never open the real app."""
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
    (bindir / 'telegram-desktop').write_text('''#!/usr/bin/python3
import json, os, pathlib, sys
pathlib.Path(os.environ['SHARE_ARGS']).write_text(json.dumps(sys.argv[1:]))
''')
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
    print(json.dumps(clients))
elif args == ['-j', 'monitors']:
    print(json.dumps([dict(id=1, name='TEST', specialWorkspace={'name':os.environ['VISIBLE_SPECIAL']})]))
else:
    print('ok')
''')
    for path in bindir.iterdir():
        path.chmod(0o700)
    video = root / "recording with spaces ' and ;.mp4"
    video.write_bytes(b'synthetic video')
    for name, workspace, visible, startup in (
        ('hidden', 'special:chat', '', 0),
        ('visible', 'special:chat', 'special:chat', 0),
        ('normal', '4', 'special:other', 0),
        ('cold', 'special:chat', '', 3),
    ):
        requests = root / (name + '.requests')
        shared = root / (name + '.shared')
        env = {**os.environ, 'PATH': str(bindir) + ':' + os.environ['PATH'],
               'HYPRLAND_INSTANCE_SIGNATURE': 'synthetic', 'SHARE_ARGS': str(shared),
               'REQUESTS': str(requests), 'POLL_COUNT': str(root / (name + '.polls')),
               'WORKSPACE': workspace, 'VISIBLE_SPECIAL': visible, 'STARTUP_POLLS': str(startup)}
        subprocess.run([str(APP / 'runtjik-core'), 'share-recording-telegram', str(video)],
                       env=env, check=True, timeout=8)
        assert json.loads(shared.read_text()) == ['-sendpath', str(video)]
        commands = [json.loads(line) for line in requests.read_text().splitlines()]
        dispatches = [args[1] for args in commands if args[0] == 'dispatch']
        assert dispatches[-1] == 'hl.dsp.focus({ window = "address:0xabc" })', dispatches
        toggles = [command for command in dispatches if 'toggle_special' in command]
        expected = [] if name == 'visible' else ['other' if name == 'normal' else 'chat']
        assert toggles == [f'hl.dsp.workspace.toggle_special("{value}")' for value in expected], toggles
        print('PASS: Telegram share/focus', name)
