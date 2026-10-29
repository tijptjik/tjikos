#!/usr/bin/env python3
"""Publish only reviewed pinned sources; credentials stay in the caller's environment."""
from pathlib import Path
import json
import os
import re
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request

root = Path(__file__).resolve().parents[1]
release = json.loads((root / 'release.json').read_text())
for name, repository, required in (
    ('dotjik', 'dotfiles', 'native/home/files-device.json'),
    ('dotjik', 'dotfiles', 'scripts/install-tjikup.sh'),
    ('tjikos', 'tjikos', 'apps/tjikup/src/device.rs'),
    ('tjikos', 'tjikos', 'apps/tjikup/scripts/fetch-release.sh'),
    ('tjikos', 'tjikos', 'apps/tjikup/scripts/install-weekly.sh'),
    ('tjikos', 'tjikos', 'apps/tjikos-sh/helpers/restore-collections.sh'),
    ('tjikos', 'tjikos', 'apps/synctjik/src/engine.rs'),
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
# Verify the exact downloadable executable before changing the live installer.
import hashlib
for tool in ('tjikup', 'synctjik'):
    binary = release[tool + '_binary']
    with tempfile.TemporaryDirectory(prefix='tjikos-sh-binary-') as directory:
        executable = Path(directory) / 'tjikup'
        subprocess.run(['curl', '--proto', '=https', '--proto-redir', '=https',
            '--tlsv1.2', '-fsSL', '--retry', '3', '--max-time', '180',
            binary['url'], '-o', str(executable)], check=True)
        if hashlib.sha256(executable.read_bytes()).hexdigest() != binary['sha256']:
            raise SystemExit('Published runner binary does not match its pinned SHA-256')
config = json.loads((root / 'wrangler.jsonc').read_text())
routes = config.pop('routes')
if len(routes) != 1 or not routes[0].get('custom_domain'):
    raise SystemExit('Expected one explicitly configured custom domain')
domain = routes[0]
config['assets']['directory'] = str(root / 'dist')
# Custom domains use the account Workers API. Wrangler's combined deployment
# also lists zone Worker routes, which needs a separate permission this token
# does not have. Keep asset publication and domain attachment explicit.
with tempfile.TemporaryDirectory(prefix='tjikos-sh-deploy-') as directory:
    upload_config = Path(directory) / 'wrangler.json'
    upload_config.write_text(json.dumps(config))
    command = ['bun', 'x', '--no-install', 'wrangler', 'deploy', '--config', str(upload_config)]
    subprocess.run([*command, '--dry-run'], cwd=root, check=True)
    subprocess.run(command, cwd=root, check=True)
request = urllib.request.Request(
    f"https://api.cloudflare.com/client/v4/accounts/{config['account_id']}/workers/domains",
    method='PUT',
    data=json.dumps({'hostname': domain['pattern'], 'zone_id': domain['zone_id'],
                     'service': config['name'], 'environment': 'production'}).encode(),
    headers={'Authorization': 'Bearer ' + os.environ['CLOUDFLARE_API_TOKEN'],
             'Content-Type': 'application/json'})
with urllib.request.urlopen(request, timeout=30) as response:
    result = json.load(response)
if not result.get('success'):
    raise SystemExit('Worker uploaded but custom-domain attachment failed')
# Allow DNS/certificate propagation, then compare exactly the reviewed bytes.
expected = (root / 'dist/init').read_bytes()
for attempt in range(12):
    try:
        check = urllib.request.Request('https://' + domain['pattern'] + '/init',
                                       headers={'User-Agent': 'tjikos-sh-release/1.0'})
        with urllib.request.urlopen(check, timeout=15) as response:
            if response.read(1024 * 1024) == expected:
                break
    except (urllib.error.URLError, TimeoutError):
        pass
    if attempt == 11:
        raise SystemExit('Deployment completed but endpoint byte verification failed')
    time.sleep(5)
print('Verified https://sh.type.hk/init against the reviewed artifact')
