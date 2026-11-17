#!/usr/bin/env python3
"""Install the validated browser archive without modifying browser profiles."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import zipfile

root = Path(__file__).resolve().parents[1]
version = json.loads((root / 'extension/manifest.json').read_text())['version']
archive = root / 'browser-artifacts' / f'runtjik_browser_bridge-{version}.zip'
stage = root / 'dist/extension'
with zipfile.ZipFile(archive) as package:
    expected = {path.name: path.read_bytes() for path in stage.iterdir()}
    if set(package.namelist()) != set(expected):
        raise SystemExit('Archive contents differ from validated build')
    for name, contents in expected.items():
        actual = package.read(name)
        matches = json.loads(contents) == json.loads(actual) if name == 'manifest.json' else contents == actual
        if not matches:
            raise SystemExit(f'Archive differs from validated build: {name}')
home = Path(os.environ.get('RUNTJIK_INSTALL_HOME', Path.home()))
native = home / '.local/lib/runtjik/runtjik-core'
if not native.is_file():
    raise SystemExit('Install Runtjik first')
target = home / '.local/lib/runtjik-browser'
target.mkdir(parents=True, exist_ok=True)
for source in [archive, root / 'extension/install-host.sh']:
    fd, pending = tempfile.mkstemp(prefix='.install-', dir=target)
    os.close(fd)
    shutil.copyfile(source, pending)
    os.chmod(pending, 0o755 if source.suffix == '.sh' else 0o644)
    os.replace(pending, target / source.name)
    if hashlib.sha256(source.read_bytes()).digest() != hashlib.sha256((target / source.name).read_bytes()).digest():
        raise SystemExit(f'Installed file differs: {source.name}')
subprocess.run([str(native), 'register-browser-host', '--runtjik-dir', str(native.parent),
                '--manifest-dir', str(home / '.mozilla/native-messaging-hosts')], check=True)
print(f'Installed and hash-verified {target}')
print('The ZIP is unsigned. Keep the signed AMO extension installed; browser profiles were not modified.')
