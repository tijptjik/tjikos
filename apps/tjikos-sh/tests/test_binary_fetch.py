"""Downloaded executables cannot replace the current binary before verification."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


class BinaryFetchTests(unittest.TestCase):
    def test_checksum_failure_preserves_existing_binary_and_success_is_cached(self):
        self.check_fetch("tjikup")

    def test_synctjik_checksum_failure_preserves_existing_binary(self):
        self.check_fetch("synctjik")

    def check_fetch(self, tool):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            app = root / 'apps' / tool
            (app / 'scripts').mkdir(parents=True)
            (app / 'dist').mkdir()
            (root / 'apps/tjikos-sh').mkdir()
            script = app / 'scripts/fetch-release.sh'
            shutil.copy2(ROOT / tool / 'scripts/fetch-release.sh', script)
            payload = b'#!/bin/sh\nexit 0\n'
            manifest = {tool + '_binary': {
                'url': f'https://github.com/tijptjik/tjikos/releases/download/{tool}-v0.2.0-test/{tool}-x86_64-unknown-linux-musl',
                'sha256': hashlib.sha256(payload).hexdigest()}}
            (root / 'apps/tjikos-sh/release.json').write_text(json.dumps(manifest))
            binary = app / 'dist' / tool
            binary.write_bytes(b'previous executable')
            tools = root / 'bin'
            tools.mkdir()
            curl = tools / 'curl'
            curl.write_text('#!/usr/bin/env python3\nimport os,sys\nfrom pathlib import Path\n'
                            'Path(sys.argv[sys.argv.index("-o")+1]).write_bytes(Path(os.environ["PAYLOAD"]).read_bytes())\n'
                            'with Path(os.environ["CALLS"]).open("a") as f:f.write("download\\n")\n')
            curl.chmod(0o755)
            downloaded = root / 'payload'
            downloaded.write_bytes(b'tampered content')
            env = {**os.environ, 'PATH': str(tools) + ':' + os.environ['PATH'],
                   'PAYLOAD': str(downloaded), 'CALLS': str(root / 'calls')}
            result = subprocess.run(['bash', str(script)], env=env, capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(binary.read_bytes(), b'previous executable')
            self.assertFalse(list((app / 'dist').glob(f'.{tool}.*')))
            downloaded.write_bytes(payload)
            subprocess.run(['bash', str(script)], env=env, check=True)
            self.assertEqual(binary.read_bytes(), payload)
            self.assertEqual(binary.stat().st_mode & 0o777, 0o755)
            subprocess.run(['bash', str(script)], env=env, check=True)
            self.assertEqual((root / 'calls').read_text().splitlines(), ['download', 'download'])
