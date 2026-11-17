#!/usr/bin/env python3
"""Install validated helper and available packages, never alter browser profiles."""
import hashlib
import json
from pathlib import Path
import shutil
import os
import tempfile
import zipfile

root = Path(__file__).resolve().parents[1]
dist = root / 'dist'
target = Path.home() / '.local/lib/tasktjik-browser'
manifest_dir = Path.home() / '.mozilla/native-messaging-hosts'
if not (Path.home() / '.local/lib/tasktjik/tasktjik-api').is_file():
    raise SystemExit('Install Tasktjik first')
target.mkdir(parents=True, exist_ok=True)
manifest_dir.mkdir(parents=True, exist_ok=True)
version = json.loads((root / 'extension/manifest.json').read_text())['version']
sources = [dist / 'tasktjik-host', dist / f'tasktjik-browser-{version}.zip']
signed = sorted((dist / 'signed').glob(f'*-{version}.xpi'))
if len(signed) > 1:
    raise SystemExit('Expected one signed package; retain older releases elsewhere')
if signed:
    with zipfile.ZipFile(sources[1]) as unsigned, zipfile.ZipFile(signed[0]) as package:
        for name in unsigned.namelist():
            expected, actual = unsigned.read(name), package.read(name)
            # web-ext reformats manifest JSON while packaging for Mozilla.
            matches = json.loads(expected) == json.loads(actual) if name == 'manifest.json' else expected == actual
            if not matches:
                raise SystemExit(f'Signed package differs from validated build: {name}')
        extra = set(package.namelist()) - set(unsigned.namelist())
        if any(not name.lower().startswith('meta-inf/') for name in extra):
            raise SystemExit('Signed package contains unexpected extra files')
        if not any(name.lower().startswith('meta-inf/') for name in package.namelist()):
            raise SystemExit('Signed package has no signature metadata')
    sources.extend(signed)
for source in sources:
    fd, pending = tempfile.mkstemp(prefix='.install-', dir=target)
    os.close(fd)
    shutil.copyfile(source, pending)
    os.chmod(pending, 0o700 if source.name == 'tasktjik-host' else 0o600)
    os.replace(pending, target / source.name)
    assert hashlib.sha256(source.read_bytes()).digest() == hashlib.sha256((target / source.name).read_bytes()).digest()
manifest = {'name': 'org.tjikos.tasktjik', 'description': 'Tasktjik browser reading capture',
            'path': str(target / 'tasktjik-host'), 'type': 'stdio',
            'allowed_extensions': ['tasktjik@tjikos.local']}
path = manifest_dir / 'org.tjikos.tasktjik.json'
fd, pending = tempfile.mkstemp(prefix='.tasktjik-', dir=manifest_dir)
with os.fdopen(fd, 'w') as output:
    json.dump(manifest, output, indent=2)
os.replace(pending, path)
print(f'Installed and hash-verified {target}')
if signed:
    print(f'Signed package ready: {target / signed[0].name}')
    print('Install it through Zen/Firefox Add-ons Manager; browser profiles were not modified.')
else:
    print('Permanent browser installation still requires a Mozilla-signed XPI.')
