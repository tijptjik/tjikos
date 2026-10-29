"""Installer ownership checks with synthetic executables and a private prefix."""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


class InstallTests(unittest.TestCase):
    def test_managed_install_preserves_wrappers_and_only_installs_binary(self):
        for existing in (True, False):
            with self.subTest(existing=existing), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                app, prefix = self.fixture(root)
                before = {}
                if existing:
                    for name in ('device-archives', 'chezmoi-externals-backup'):
                        path = prefix / 'bin' / name
                        path.write_text('managed configuration: ' + name)
                        path.chmod(0o700)
                        before[name] = (path.read_bytes(), path.stat())
                self.install(app, prefix, managed=True)
                self.assertEqual((prefix / 'bin/synctjik').read_bytes(), (app / 'dist/synctjik').read_bytes())
                self.assertEqual((prefix / 'bin/synctjik').stat().st_mode & 0o777, 0o755)
                for name in ('device-archives', 'chezmoi-externals-backup'):
                    path = prefix / 'bin' / name
                    if existing:
                        content, info = before[name]
                        self.assertEqual(path.read_bytes(), content)
                        self.assertEqual((path.stat().st_ino, path.stat().st_mode, path.stat().st_mtime_ns),
                                         (info.st_ino, info.st_mode, info.st_mtime_ns))
                    else:
                        self.assertFalse(path.exists())

    def test_standalone_install_creates_working_prefix_relative_wrappers(self):
        with tempfile.TemporaryDirectory() as directory:
            app, prefix = self.fixture(Path(directory))
            self.install(app, prefix, managed=False)
            for name, expected in [('device-archives', 'status\nfolder with spaces\n'),
                                   ('chezmoi-externals-backup', 'backup\nstatus\nfolder with spaces\n')]:
                result = subprocess.run([str(prefix / 'bin' / name), 'status', 'folder with spaces'],
                                        capture_output=True, text=True, check=True)
                self.assertEqual(result.stdout, expected)

    def test_incompatible_pinned_release_retains_capable_installed_runner(self):
        with tempfile.TemporaryDirectory() as directory:
            app, prefix = self.fixture(Path(directory))
            installed = prefix / 'bin/synctjik'
            shutil.copy2(app / 'dist/synctjik', installed)
            before = installed.read_bytes()
            (app / 'dist/synctjik').write_text('#!/bin/sh\necho old-release\n')
            self.install(app, prefix, managed=True)
            self.assertEqual(installed.read_bytes(), before)

    def test_incompatible_release_is_rejected_on_fresh_install(self):
        with tempfile.TemporaryDirectory() as directory:
            app, prefix = self.fixture(Path(directory))
            (app / 'dist/synctjik').write_text('#!/bin/sh\necho old-release\n')
            with self.assertRaises(subprocess.CalledProcessError):
                self.install(app, prefix, managed=True)
            self.assertFalse((prefix / 'bin/synctjik').exists())

    def test_native_only_release_cannot_replace_protection_engine(self):
        with tempfile.TemporaryDirectory() as directory:
            app, prefix = self.fixture(Path(directory))
            installed = prefix / 'bin/synctjik'
            shutil.copy2(app / 'dist/synctjik', installed)
            before = installed.read_bytes()
            (app / 'dist/synctjik').write_text("#!/bin/sh\necho '{\"native_backends\":1}'\n")
            self.install(app, prefix, managed=True)
            self.assertEqual(installed.read_bytes(), before)

    def fixture(self, root):
        app, prefix = root / 'app', root / 'custom prefix'
        (app / 'scripts').mkdir(parents=True)
        (app / 'dist').mkdir()
        (prefix / 'bin').mkdir(parents=True)
        shutil.copy2(ROOT / 'scripts/install.sh', app / 'scripts/install.sh')
        (app / 'scripts/fetch-release.sh').write_text('#!/bin/sh\nexit 0\n')
        (app / 'dist/synctjik').write_text("#!/bin/sh\nif [ \"$1\" = capabilities ]; then echo '{\"cloudflare_backup\":true,\"private_assets\":true,\"native_backends\":1,\"protection_backends\":1}'; else printf '%s\\n' \"$@\"; fi\n")
        (app / 'dist/synctjik').chmod(0o755)
        return app, prefix

    def install(self, app, prefix, managed):
        env = {**os.environ, 'TJIKUP_MANAGED_CONFIG': '1' if managed else '0'}
        subprocess.run(['bash', str(app / 'scripts/install.sh'), str(prefix)], env=env,
                       capture_output=True, text=True, check=True)
