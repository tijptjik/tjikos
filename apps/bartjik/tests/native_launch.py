"""Launcher routing without opening windows or accessing a desktop session."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

APP = Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix='bartjik-launch-') as directory:
    root = Path(directory)
    (root / 'assets').mkdir()
    (root / 'bin').mkdir()
    shutil.copy2(APP / 'bin/bartjik', root / 'bin/bartjik')
    target = root / 'bin/bartjik-gtk'
    target.write_text('#!/usr/bin/env python3\nimport json,os,sys\nprint(json.dumps({"args":sys.argv[1:],"home":os.environ["BARTJIK_HOME"],"config":os.environ.get("BARTJIK_CONFIG")}))\n')
    target.chmod(0o700)
    for args, expected in [([], ['serve', '--reserve']), (['status'], ['status']),
                           (['--gtk', 'quit'], ['quit']),
                           (['--config', 'example.json', '--demo'], ['serve', '--reserve', '--demo'])]:
        value = json.loads(subprocess.check_output([str(root / 'bin/bartjik'), *args], cwd=root, text=True))
        assert value['args'] == expected, value
        assert value['home'] == str(root), value
        if '--config' in args:
            assert value['config'] == str(root / 'example.json'), value
print('PASS: native default, control, compatibility alias and configuration routing')
