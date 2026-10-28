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
        for requirement in ('fish', 'git', 'chezmoi', 'age', 'python3-tomli-w', 'cargo', 'HTTPS', 'separate command'):
            self.assertIn(requirement, result.stdout)

    def test_reviewed_script_contains_no_unpinned_installer_pipeline(self):
        script = (ROOT / 'init.sh').read_text()
        self.assertNotIn('curl |', script)
        self.assertIn('^ [0-9a-f]'.replace(' ', ''), script)
        self.assertIn('git clone --no-checkout', script)
        self.assertIn('No reset or checkout was performed', script)
        self.assertIn('tjikup new device </dev/tty', script)
        self.assertNotIn('tjikup --show-skips\n', script)

    def test_release_build_rejects_unpublished_revisions(self):
        release = json.loads((ROOT / 'release.json').read_text())
        if all(release.get(name + '_revision') for name in ('dotjik', 'tjikos')):
            self.skipTest('release is pinned')
        result = subprocess.run(['python3', '-B', str(ROOT / 'scripts/build.py'), '--release'], capture_output=True)
        self.assertNotEqual(result.returncode, 0)

    def test_shell_syntax(self):
        subprocess.run(['bash', '-n', str(ROOT / 'init.sh')], check=True)
