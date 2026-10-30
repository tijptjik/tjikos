from pathlib import Path
import ast
import subprocess
root = Path(__file__).resolve().parents[1]
for path in root.rglob('*.py'):
    if 'dist' not in path.parts:
        ast.parse(path.read_text(), filename=str(path))
for path in [root / 'bin/runtjik', *root.glob('scripts/*.sh')]:
    subprocess.run(['bash', '-n', str(path)], check=True)
subprocess.run(['cargo', 'fmt', '--check', '--manifest-path', str(root / 'Cargo.toml')], check=True)
subprocess.run(['python3', '-B', str(root / 'scripts/export_contract.py'), '--check'], check=True)
print('Runtjik Python, shell, Rust formatting and contract checks passed')
