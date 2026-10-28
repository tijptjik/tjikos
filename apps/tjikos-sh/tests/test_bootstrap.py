"""Exercise bootstrap contracts without installing packages or reading credentials."""
from pathlib import Path
import json
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


class BootstrapTests(unittest.TestCase):
    def test_plan_is_non_mutating_and_lists_early_dependencies(self):
        result = subprocess.run(['bash', str(ROOT / 'init.sh'), '--plan'], capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        packages = next(line for line in result.stdout.splitlines() if 'Install Fedora packages:' in line).split(':', 1)[1].split()
        self.assertFalse(set(packages) & {'python3', 'python3-tomli', 'python3-tomli-w', 'cargo', 'gcc', 'make', 'pkgconf-pkg-config', 'unzip', 'tar', 'gzip', 'gh', 'openssh-clients'})
        self.assertNotIn('chezmoi', result.stdout)
        for requirement in ('fish', 'git', 'age', 'SHA-256', 'HTTPS', 'separate command'):
            self.assertIn(requirement, result.stdout)

    def test_reviewed_script_contains_no_unpinned_installer_pipeline(self):
        script = (ROOT / 'init.sh').read_text()
        self.assertNotIn('curl |', script)
        for forbidden in ('github-ssh.sh', 'GHI_TOKEN', 'remote set-url', 'core.sshCommand', 'git push'):
            self.assertNotIn(forbidden, script)
        self.assertIn('^ [0-9a-f]'.replace(' ', ''), script)
        self.assertIn('git clone --no-checkout', script)
        self.assertIn('No reset or checkout was performed', script)
        self.assertIn('tjikup new device </dev/tty', script)
        self.assertNotIn('tjikup --show-skips\n', script)

    def test_release_build_rejects_unpinned_binary(self):
        import shutil
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'scripts').mkdir()
            shutil.copy2(ROOT / 'scripts/build.py', root / 'scripts/build.py')
            shutil.copy2(ROOT / 'init.sh', root / 'init.sh')
            release = json.loads((ROOT / 'release.json').read_text())
            release['tjikup_binary'] = {'url': 'https://example.invalid/tjikup', 'sha256': '0' * 64}
            (root / 'release.json').write_text(json.dumps(release))
            result = subprocess.run(['python3', '-B', str(root / 'scripts/build.py'), '--release'], capture_output=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertFalse((root / 'dist').exists())

    def test_shell_syntax(self):
        for path in [ROOT / 'init.sh', *ROOT.glob('helpers/*.sh')]:
            subprocess.run(['bash', '-n', str(path)], check=True)
