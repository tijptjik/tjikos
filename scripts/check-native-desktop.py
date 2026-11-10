"""Prevent retired frontend payloads or entrypoints returning to the three native apps."""
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
for name in ('bartjik', 'runtjik', 'notjik'):
    app = ROOT / 'apps' / name
    assert not (app / 'qml').exists(), f'{name}: retired frontend directory'
    for path in app.rglob('*'):
        if any(part in {'target', 'dist', '__pycache__', 'node_modules', '.git'} for part in path.relative_to(app).parts):
            continue
        if path.is_symlink():
            assert path.exists(), f'{path}: dangling resource link'
            assert 'qml' not in path.resolve().parts, f'{path}: resource depends on a retired UI directory'
        if not path.is_file():
            continue
        assert path.suffix != '.qml', path
        try:
            source = path.read_text()
        except (UnicodeError, OSError):
            continue
        assert 'quickshell' not in source.lower(), f'{path}: retired frontend reference'
assert not (ROOT / 'packages/notification-history').exists(), 'retired shared history package'
for name in ('bartjik_showcase.py', 'os_showcase.py', 'showcase_close.py', 'showcase_close_app.py', 'CapturePlayback.qml'):
    assert not (ROOT / 'scripts' / name).exists(), f'{name}: retired desktop recorder'
for name in ('suite_showcase.py', 'showcase_runtime.py'):
    text = (ROOT / 'scripts' / name).read_text()
    for app in ('bartjik', 'runtjik', 'notjik'):
        assert f'apps/{app}/qml' not in text, f'{name}: retired desktop payload'
print('PASS: three native apps contain no retired renderer payloads or references')
