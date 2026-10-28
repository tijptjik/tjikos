"""Exercise enrollment with local keys and fake BWS/GitHub; no live credentials."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
KEY_ID = '11111111-1111-1111-1111-111111111111'


class GitHubSSHTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.home = self.root / 'home'
        self.home.mkdir()
        self.bin = self.root / 'bin'
        self.bin.mkdir()
        key = self.root / 'fixture-key'
        subprocess.run(['ssh-keygen', '-q', '-t', 'ed25519', '-N', '', '-f', str(key)], check=True)
        self.env = {**os.environ, 'HOME': str(self.home), 'XDG_CONFIG_HOME': str(self.home / '.config'),
                    'PATH': str(self.bin) + ':' + os.environ['PATH'], 'FIXTURE': str(self.root),
                    'BWS_ACCESS_TOKEN': 'synthetic-bws-token', 'TJIKUP_SSH_KEY_SECRET_ID': ''}
        self.write('bws', '''import json,os,sys
from pathlib import Path
root=Path(os.environ['FIXTURE'])
if os.environ.get('FAIL_BWS'):sys.exit(3)
value=(root/'fixture-key').read_text() if sys.argv[-1]=='11111111-1111-1111-1111-111111111111' else 'ghp_synthetic_only'
print(json.dumps({'value':value}))
''')
        self.write('curl', '''import json,os,sys
from pathlib import Path
root=Path(os.environ['FIXTURE']);args=sys.argv[1:]
if '--config' in args:sys.stdin.read()
url=next(a for a in args if a.startswith('https://'))
path=url.removeprefix('https://api.github.com/')
with (root/'calls').open('a') as f:f.write(path+'\\n')
if path=='meta':value={'ssh_keys':[(root/'fixture-key.pub').read_text().strip()]}
elif path=='users/tijptjik':value={'login':'tijptjik','id':123,'name':'Test User'}
elif path=='user':
 if os.environ.get('DENY_TOKEN'):sys.exit(22)
 value={'login':'other' if os.environ.get('WRONG_USER') else 'tijptjik'}
elif path.startswith('user/keys?'):value=[]
elif path=='user/keys':
 payload=json.loads(Path(args[args.index('--data-binary')+1].removeprefix('@')).read_text())
 assert payload['key'].startswith('ssh-ed25519 ') and 'PRIVATE' not in payload['key']
 (root/'registered').write_text(json.dumps(payload));value={'id':456}
else:raise AssertionError(path)
print(json.dumps(value))
''')
        self.write('ssh', '''import os,sys
from pathlib import Path
root=Path(os.environ['FIXTURE'])
if (root/'registered').exists() or os.environ.get('ALLOW_EXISTING'):
 print("Hi tijptjik! You've successfully authenticated, but GitHub does not provide shell access.",file=sys.stderr)
 sys.exit(1)
sys.exit(255)
''')

    def write(self, name, code):
        path = self.bin / name
        path.write_text('#!/usr/bin/env python3\n' + code)
        path.chmod(0o755)

    def run_helper(self, *args, **env):
        result = subprocess.run(['bash', str(ROOT / 'helpers/github-ssh.sh'), *args],
                                env={**self.env, **env}, capture_output=True, text=True)
        self.assertNotIn('ghp_synthetic_only', result.stdout + result.stderr)
        self.assertNotIn('synthetic-bws-token', result.stdout + result.stderr)
        return result

    def test_new_key_registration_and_idempotent_reuse(self):
        result = self.run_helper()
        self.assertEqual(result.returncode, 0, result.stderr)
        key = self.home / '.ssh/tjikup_github_ed25519'
        before = key.read_bytes()
        self.assertEqual(key.stat().st_mode & 0o777, 0o600)
        config = (self.home / '.ssh/tjikup-github.conf').read_text()
        self.assertIn('StrictHostKeyChecking yes', config)
        self.assertIn('IdentitiesOnly yes', config)
        calls = (self.root / 'calls').read_text().splitlines()
        self.assertIn('user/keys', calls)
        result = self.run_helper(FAIL_BWS='1')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(key.read_bytes(), before)
        self.assertEqual((self.root / 'calls').read_text().splitlines().count('user/keys'), 1)

    def test_rejected_token_and_wrong_account_do_not_create_keys(self):
        for env in ({'DENY_TOKEN': '1'}, {'WRONG_USER': '1'}):
            with self.subTest(env=env):
                result = self.run_helper(**env)
                self.assertNotEqual(result.returncode, 0)
                self.assertFalse((self.home / '.ssh/tjikup_github_ed25519').exists())
                self.assertFalse((self.root / 'registered').exists())

    def test_existing_bws_key_needs_no_github_token_or_registration(self):
        result = self.run_helper('--ssh-key-secret', KEY_ID, ALLOW_EXISTING='1')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual((self.home / '.ssh/tjikup_github_ed25519').read_bytes().rstrip(),
                         (self.root / 'fixture-key').read_bytes().rstrip())
        calls = (self.root / 'calls').read_text().splitlines()
        self.assertNotIn('user', calls)
        self.assertNotIn('user/keys', calls)

    def test_failed_existing_key_is_never_replaced(self):
        folder = self.home / '.ssh'
        folder.mkdir()
        key = folder / 'tjikup_github_ed25519'
        subprocess.run(['ssh-keygen', '-q', '-t', 'ed25519', '-N', '', '-f', str(key)], check=True)
        before = key.read_bytes()
        result = self.run_helper('--ssh-key-secret', KEY_ID)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(key.read_bytes(), before)
        self.assertIn('no key was replaced', result.stderr)
