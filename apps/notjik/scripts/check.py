import ast
import json
from pathlib import Path

root = Path(__file__).resolve().parent.parent
for path in root.rglob("*.py"):
    if "dist" not in path.parts:
        ast.parse(path.read_text(), filename=str(path))
for path in [root / "package.json", *root.glob("themes/*.json"), *root.glob("examples/*.json")]:
    json.loads(path.read_text())
print("notjik: Python syntax and JSON checked (QML requires a runtime check)")
