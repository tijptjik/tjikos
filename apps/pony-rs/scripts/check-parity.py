#!/usr/bin/env python3
"""Compare every shipped pony's visible layout with pinned upstream Python.

Usage: python3 scripts/check-parity.py /path/to/erkin-ponysay
The reference checkout is used only in development, never at runtime.
"""
import contextlib
import io
import re
import subprocess
import sys
from pathlib import Path

root = Path(__file__).resolve().parents[1]
reference = Path(sys.argv[1]).resolve()
sys.path.insert(0, str(reference / 'src'))
from backend import Backend
from balloon import Balloon

ansi = re.compile(r'\x1b\]P[0-9A-Fa-f]{7}|\x1b\]R|\x1b\[[0-?]*[ -/]*[@-~]')
def normalize(text):
    return '\n'.join(line.rstrip() for line in ansi.sub('', text).splitlines()).rstrip()

failures = []
reference_errors = []
known_upstream_bugs = {
    "ttyponies/twilightrage.pony",
    "extrattyponies/velvetremedy.pony",
}
fixed_layouts = []
count = 0
for group, tty in [('ponies', False), ('extraponies', False), ('ttyponies', True), ('extrattyponies', True)]:
    for path in sorted((root / 'assets' / group).glob('*.pony')):
        args = [str(root / 'target/release/pony-rs'), '-f', path.stem, '-W', '240']
        if group.startswith('extra'):
            args.append('--extra')
        if tty:
            args.append('--tty')
        result = subprocess.run(args + ['Hi'], capture_output=True, text=True, check=True)
        backend = Backend('Hi', str(path), 240, None, Balloon.fromFile(None, False), '', '', '', '', 0)
        try:
            with contextlib.redirect_stdout(io.StringIO()):
                backend.parse()
        except TypeError as error:
            if f"{group}/{path.name}" not in known_upstream_bugs:
                raise
            reference_errors.append(f"{group}/{path.name}: {error}")
            continue
        expected, actual = normalize(backend.output), normalize(result.stdout)
        count += 1
        if expected != actual:
            if f"{group}/{path.name}" in known_upstream_bugs:
                fixed_layouts.append(f"{group}/{path.name}")
                continue
            failures.append(f'{group}/{path.name}')
            if len(failures) <= 3:
                import difflib
                print('\n'.join(difflib.unified_diff(expected.splitlines(), actual.splitlines(), fromfile=str(path), tofile='pony-rs')))
print(f'{count - len(failures) - len(fixed_layouts)} layouts match upstream; {len(fixed_layouts) + len(reference_errors)} known upstream TTY bugs covered by Rust layout tests')
print('Fixed upstream layouts:', fixed_layouts)
print('Upstream renderer errors:', reference_errors)
if failures:
    print('Mismatches:', ', '.join(failures))
    sys.exit(1)
