"""Static Python syntax check; Go compilation is the build task."""
import ast
from pathlib import Path
for path in Path('backend').glob('*.py'):
    ast.parse(path.read_text(), filename=str(path))
print('Python syntax OK')
