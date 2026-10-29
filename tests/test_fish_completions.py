"""Exercise installed completions with synthetic homes and inert CLI stubs."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


class FishCompletionsTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.home = Path(self.temp.name)
        self.prefix = self.home / 'prefix with spaces'
        subprocess.run(['bash', str(ROOT / 'scripts/install-completions.sh'), str(self.prefix)], check=True)
        self.vendor = self.prefix / 'share/fish/vendor_completions.d'
        self.bin = self.home / 'bin'
        self.bin.mkdir()
        for name in ('tjikup', 'synctjik', 'bartjik', 'bartjikctl', 'notjik', 'notjikctl', 'runtjik', 'mailtjik',
                     'greetjik', 'dotjik', 'systjik', 'tjix', 'tjixx', 'fleet-ssh',
                     'fitjikup', 'litjikup', 'sitjikup', 'device-archives'):
            path = self.bin / name
            path.write_text('#!/bin/sh\ntouch "$HOME/unexpected-execution"\nexit 99\n')
            path.chmod(0o755)
        self.env = {**os.environ, 'HOME': str(self.home), 'XDG_CONFIG_HOME': str(self.home / 'config'),
                    'XDG_DATA_HOME': str(self.home / 'data'), 'XDG_CACHE_HOME': str(self.home / 'cache'),
                    'PATH': str(self.bin) + ':/usr/bin:/bin', 'COMPLETION_VENDOR': str(self.vendor)}

    def complete(self, line):
        result = subprocess.run(['fish', '--no-config', '-c',
                                 'set -p fish_complete_path "$COMPLETION_VENDOR"; complete -C "$COMPLETION_LINE"'],
                                env={**self.env, 'COMPLETION_LINE': line}, text=True, capture_output=True, check=True)
        self.assertEqual(result.stderr, '')
        self.assertFalse((self.home / 'unexpected-execution').exists(), 'Tab must not run a service or workload')
        return {row.split('\t', 1)[0] for row in result.stdout.splitlines()}

    def test_every_public_app_and_control_command(self):
        for line, expected in {'tjikup ': 'config', 'synctjik ': 'restore', 'mailtjik ': 'read',
                               'bartjik --': '--demo', 'bartjikctl ': 'spotify', 'notjik --': '--daemon',
                               'notjikctl ': 'send', 'runtjik ': 'show', 'greetjik --': '--pony'}.items():
            with self.subTest(line=line):
                self.assertIn(expected, self.complete(line))

    def test_nested_commands_and_option_values(self):
        for line, expected in {
            'tjikup config home ': 'apply', 'tjikup config system ': 'source-path',
            'tjikup workflow --repo "/tmp/repo with spaces" files home-files ': 'check',
            'tjikup workflow --repo=/tmp/config files system ': 'plan',
            'tjikup system --policy /tmp/policy.json provision ': 'mergerfs',
            'tjikup new device --role ': 'server',
            'synctjik --config /tmp/policy.json app-pull ': 'health',
            'synctjik private-assets mount ': 'siprivate',
            'synctjik enroll --replace --role ': 'server',
            'bartjikctl --config /tmp/bars.json init --theme ': 'rose-punk',
            'notjikctl send "A title" --urgency ': 'critical',
            'notjikctl send --replace local-1 "A title" --type ': 'progress',
            'runtjik migrate-config --': '--enable-bundled-plugins',
        }.items():
            with self.subTest(line=line):
                self.assertIn(expected, self.complete(line))

    def test_wrappers_and_native_sync_delegation(self):
        for line, expected in {'dotjik ': 'apply', 'systjik ': 'check', 'tjix --': '--dry-run',
                               'tjixx --': '--verbose', 'fleet-ssh --transport ': 'tailnet',
                               'fitjikup --': '--plan', 'litjikup --': '--apply-dotfiles',
                               'sitjikup --': '--apply-sysfiles', 'device-archives ': 'backup',
                               'tjikup sync private-assets ': 'restore',
                               'tjikup sync plan --': '--snapshot'}.items():
            with self.subTest(line=line):
                self.assertIn(expected, self.complete(line))

    def test_subcommands_do_not_leak_to_other_contexts(self):
        self.assertNotIn('apply', self.complete('tjikup config '))
        self.assertNotIn('--force', self.complete('synctjik restore collection --'))
        self.assertNotIn('--urgency', self.complete('notjikctl logs --'))
        self.assertNotIn('read', self.complete('mailtjik read '))
        self.assertNotIn('config', self.complete('tjikup workflow package run fish -- '))

    def test_path_options_complete_files_with_spaces(self):
        target = self.home / 'policy with spaces.json'
        target.write_text('{}')
        for command in ('mailtjik read --config', 'synctjik apply --resolutions', 'tjikup files plan --manifest'):
            values = self.complete(command + ' ' + str(self.home) + '/pol')
            self.assertTrue(any('policy' in v and 'spaces.json' in v for v in values), values)

    def test_all_files_parse(self):
        for path in self.vendor.glob('*.fish'):
            subprocess.run(['fish', '--no-config', '--no-execute', str(path)], check=True)

    def test_selected_install_does_not_install_unrelated_apps(self):
        prefix = self.home / 'selected'
        subprocess.run(['bash', str(ROOT / 'scripts/install-completions.sh'), str(prefix), 'synctjik'], check=True)
        names = {p.name for p in (prefix / 'share/fish/vendor_completions.d').iterdir()}
        self.assertEqual(names, {'__tjikos.fish', 'synctjik.fish', 'device-archives.fish'})

    def test_invalid_selection_does_not_partially_install(self):
        prefix = self.home / 'invalid'
        result = subprocess.run(['bash', str(ROOT / 'scripts/install-completions.sh'), str(prefix),
                                 'synctjik', 'unknown'], capture_output=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(prefix.exists())


if __name__ == '__main__':
    unittest.main()
