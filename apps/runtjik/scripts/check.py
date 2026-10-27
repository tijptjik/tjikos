from pathlib import Path
import ast
import subprocess
root = Path(__file__).resolve().parents[1]
for path in root.rglob('*.py'):
    if 'dist' not in path.parts:
        ast.parse(path.read_text(), filename=str(path))
for path in [root / 'bin/runtjik', *root.glob('scripts/*.sh')]:
    subprocess.run(['bash', '-n', str(path)], check=True)
print('Runtjik syntax checks passed')
