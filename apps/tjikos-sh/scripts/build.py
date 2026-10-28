#!/usr/bin/env python3
"""Build a pinned, reviewable bootstrap; refuse unpublished source revisions."""
from pathlib import Path
import hashlib
import json
import re
import shutil
import subprocess
import sys

root = Path(__file__).resolve().parents[1]
release = json.loads((root / 'release.json').read_text())
script = (root / 'init.sh').read_text()
for name in ('dotjik', 'tjikos'):
    branch = release.get(name + '_branch')
    if not isinstance(branch, str) or not re.fullmatch('[A-Za-z0-9][A-Za-z0-9._/-]*', branch):
        raise SystemExit(f'Invalid {name} release branch')
    subprocess.run(['git', 'check-ref-format', '--branch', branch], check=True, capture_output=True)
    script = script.replace('@' + name.upper() + '_BRANCH@', branch)
    revision = release.get(name + '_revision')
    if not isinstance(revision, str) or not re.fullmatch('[a-f0-9]{40}', revision):
        if '--release' in sys.argv:
            raise SystemExit(f'Pin a published {name} commit in release.json before deploying')
        print(f'{name}: building a non-executable draft until source publication')
        continue
    script = script.replace('@' + name.upper() + '_REVISION@', revision)
for tool in ('tjikup', 'synctjik'):
    binary = release.get(tool + '_binary', {})
    url, digest = binary.get('url', ''), binary.get('sha256', '')
    valid = (isinstance(url, str) and isinstance(digest, str)
             and re.fullmatch(rf'https://github\.com/tijptjik/tjikos/releases/download/{tool}-v[A-Za-z0-9._-]+/{tool}-x86_64-unknown-linux-musl', url)
             and re.fullmatch('[a-f0-9]{64}', digest)
             and re.fullmatch('[a-f0-9]{40}', binary.get('source_revision', ''))
             and binary.get('target') == 'x86_64-unknown-linux-musl')
    if not valid:
        if '--release' in sys.argv:
            raise SystemExit(f'Publish and pin a static {tool} binary before deploying')
        print('binary: building a non-executable draft until release publication')
    else:
        script = script.replace('@' + tool.upper() + '_BINARY_URL@', url).replace('@' + tool.upper() + '_BINARY_SHA256@', digest)
subprocess.run(['bash', '-n'], input=script, text=True, check=True)
dist = root / 'dist'
dist.mkdir(exist_ok=True)
for path in (root / 'public').iterdir():
    if path.is_file():
        shutil.copyfile(path, dist / path.name)
(dist / 'init').write_text(script)
(dist / 'init.sha256').write_text(hashlib.sha256(script.encode()).hexdigest() + '  init\n')
(dist / 'release.json').write_text(json.dumps(release, indent=2) + '\n')
print('Built bootstrap artifact and SHA-256 receipt')
