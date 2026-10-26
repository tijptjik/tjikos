import ast, json, sys
from pathlib import Path

root = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(root))
from bartjik_core.config import load

for path in root.rglob("*.py"):
    if "dist" not in path.parts:
        ast.parse(path.read_text(), filename=str(path))
for path in root.glob("examples/*.json"):
    load(path)
for path in root.glob("themes/*.json"):
    json.loads(path.read_text())
print("bartjik: Python syntax, themes and effective example configurations checked")
