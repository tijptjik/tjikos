#!/usr/bin/env python3
"""Publish only reviewed pinned sources; credentials stay in the caller's environment."""
from pathlib import Path
import json
import os
import re
import subprocess
import sys
import urllib.request

root = Path(__file__).resolve().parents[1]
release = json.loads((root / 'release.json').read_text())
for name, repository, required in (
    ('dotjik', 'dotfiles', 'scripts/device-policy.py'),
    ('tjikos', 'tjikos', 'apps/tjikup/src/device.rs'),
):
    revision = release.get(name + '_revision')
    if not isinstance(revision, str) or not re.fullmatch('[a-f0-9]{40}', revision):
        raise SystemExit(f'Publish and pin the reviewed {name} source before deploying /init')
    url = f'https://api.github.com/repos/tijptjik/{repository}/contents/{required}?ref={revision}'
    with urllib.request.urlopen(urllib.request.Request(url, headers={'User-Agent': 'tjikos-sh-release'}), timeout=30) as response:
        document = json.load(response)
    if document.get('type') != 'file':
        raise SystemExit(f'Pinned {name} source is missing {required}')
if not os.environ.get('CLOUDFLARE_API_TOKEN'):
    raise SystemExit('Provide a Cloudflare deploy token through the environment; never put it in source')
subprocess.run([sys.executable, str(root / 'scripts/build.py'), '--release'], check=True)
subprocess.run(['bun', 'x', '--no-install', 'wrangler', 'deploy', '--dry-run'], cwd=root, check=True)
subprocess.run(['bun', 'x', '--no-install', 'wrangler', 'deploy'], cwd=root, check=True)
# Compare the public endpoint bytes with what was reviewed and built.
with urllib.request.urlopen('https://sh.type.hk/init', timeout=30) as response:
    body = response.read(1024 * 1024)
if body != (root / 'dist/init').read_bytes():
    raise SystemExit('Deployment completed but endpoint byte verification failed')
print('Verified https://sh.type.hk/init against the reviewed artifact')
