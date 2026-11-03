"""Exercise installation checks using synthetic artifacts, paths and ownership."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


class InstallCheckTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        scripts = self.root / 'scripts'
        scripts.mkdir()
        dist = self.root / 'dist'
        dist.mkdir()
        (dist / 'pam_oo7_tjik.so').write_bytes(b'synthetic module')
        (dist / 'source.sha256').write_text('fixture\n')
        (scripts / 'source-digest.sh').write_text('printf "fixture\\n"\n')
        self.target = self.root / 'installed/pam_oo7_tjik.so'
        self.script = scripts / 'install.sh'
        self.script.write_text((ROOT / 'scripts/install.sh').read_text().replace(
            'target=/usr/local/lib/security/pam_oo7_tjik.so',
            f'target="{self.target}"',
        ))
        mock_bin = self.root / 'bin'
        mock_bin.mkdir()
        for name, body in {
            'stat': 'printf "%s\\n" "$MOCK_METADATA"',
            'sudo': 'touch "$MUTATION"; exit 99',
        }.items():
            tool = mock_bin / name
            tool.write_text('#!/bin/sh\n' + body + '\n')
            tool.chmod(0o755)
        self.env = {**os.environ, 'PATH': f'{mock_bin}:{os.environ["PATH"]}',
                    'MOCK_METADATA': '0:0:755', 'MUTATION': str(self.root / 'mutation')}

    def check(self, expected):
        result = subprocess.run(['bash', str(self.script), '--check'],
                                env=self.env, capture_output=True, text=True)
        self.assertEqual(result.returncode, expected, result.stderr)
        self.assertFalse((self.root / 'mutation').exists())
        return result

    def install_fixture(self, content=b'synthetic module'):
        self.target.parent.mkdir()
        self.target.write_bytes(content)

    def test_missing_installation_requests_apply_instead_of_check_error(self):
        result = self.check(1)
        self.assertIn('not installed', result.stderr)
        self.assertFalse(self.target.parent.exists())

    def test_identical_root_owned_module_is_current(self):
        self.install_fixture()
        self.check(0)

    def test_changed_module_requests_apply(self):
        self.install_fixture(b'older module')
        self.check(1)

    def test_incorrect_ownership_or_mode_requests_apply(self):
        self.install_fixture()
        for metadata in ('1000:1000:755', '0:0:777'):
            with self.subTest(metadata=metadata):
                self.env['MOCK_METADATA'] = metadata
                self.check(1)

    def test_invalid_target_preserves_comparison_error(self):
        self.target.mkdir(parents=True)
        self.check(2)

    def test_dangling_symlink_preserves_comparison_error(self):
        self.target.parent.mkdir()
        self.target.symlink_to(self.root / 'missing')
        self.check(2)

    def test_stale_source_is_rejected_before_comparison(self):
        self.install_fixture()
        (self.root / 'dist/source.sha256').write_text('stale\n')
        self.assertIn('Rebuild oo7-pam', self.check(1).stderr)


if __name__ == '__main__':
    unittest.main()
