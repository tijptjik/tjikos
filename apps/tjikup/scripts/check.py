from pathlib import Path
import ast

for path in Path(__file__).resolve().parents[1].rglob("*.py"):
    if "dist" not in path.parts:
        ast.parse(path.read_text(), filename=str(path))
print("Python syntax valid")
