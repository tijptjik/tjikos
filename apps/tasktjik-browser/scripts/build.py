#!/usr/bin/env python3
"""Package source assets and Tasktjik's shared recurrence/workflow helpers."""
from pathlib import Path
import shutil
import json
import zipfile

root = Path(__file__).resolve().parents[1]
dist = root / 'dist'
version = json.loads((root / 'extension/manifest.json').read_text())['version']
archive_path = dist / f'tasktjik-browser-{version}.zip'
extension = dist / 'extension'
# Signing tools leave private upload markers in the staging directory.
# Rebuild from source so those never enter the distributable archive.
if extension.exists():
    shutil.rmtree(extension)
extension.mkdir(parents=True)
for path in (root / 'extension').iterdir():
    shutil.copy2(path, extension / path.name)
shutil.copy2(root / 'LICENSE', extension / 'LICENSE')
for source, name in [('Repeat.js', 'repeat.js'), ('Workflow.js', 'workflow.js')]:
    shutil.copy2(root.parent / 'tasktjik/qml' / source, extension / name)
# Reuse the Tasktjik workflow editor rather than maintaining a second schema.
source = (root.parent / 'tasktjik/web/app.js').read_text()
start = source.index('function newStep(')
end = source.index('async function run(', start)
editor = source[start:end].replace("host.hidden=!['todo','template'].includes(editingKind);if(host.hidden)return;", '')
(extension / 'workflow-editor.js').write_text('// Generated from Tasktjik web/app.js for the Tasktjik browser companion.\n' + editor)
shutil.copy2(root / 'host/tasktjik.py', dist / 'tasktjik-host')
with zipfile.ZipFile(archive_path, 'w', zipfile.ZIP_DEFLATED) as archive:
    for path in sorted(extension.iterdir()):
        archive.write(path, path.name)
print(archive_path)
