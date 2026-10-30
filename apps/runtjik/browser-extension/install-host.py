#!/usr/bin/env python3
"""Register the installed native host for a native Linux Firefox/Zen build."""
import argparse
import json
from pathlib import Path
import shlex

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--manifest-dir', type=Path, default=Path.home() / '.mozilla/native-messaging-hosts')
parser.add_argument('--runtjik-dir', type=Path, default=Path.home() / '.local/lib/runtjik')
args = parser.parse_args()
host = args.runtjik_dir.resolve() / 'browser_host.py'
if not host.is_file():
    parser.error(f'Install Runtjik first: missing {host}')
launcher = args.runtjik_dir.resolve() / 'browser-native-host'
launcher.write_text('#!/bin/sh\nexec /usr/bin/python3 -B ' + shlex.quote(str(host)) + '\n')
launcher.chmod(0o700)
args.manifest_dir.mkdir(mode=0o700, parents=True, exist_ok=True)
manifest = args.manifest_dir / 'org.tjikos.runtjik_browser.json'
manifest.write_text(json.dumps({
    'name': 'org.tjikos.runtjik_browser',
    'description': 'Runtjik browser bridge',
    'path': str(launcher), 'type': 'stdio',
    'allowed_extensions': ['runtjik-browser@tjikos.local']
}, indent=2) + '\n')
manifest.chmod(0o600)
print(f'Registered {manifest}')
