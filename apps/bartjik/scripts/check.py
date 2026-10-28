import ast, hashlib, json, sys
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
fonts = root / "qml/fonts"
provenance = json.loads((fonts / "provenance.json").read_text())
font = (fonts / provenance["file"]).read_bytes()
if len(font) != provenance["size"] or hashlib.sha256(font).hexdigest() != provenance["sha256"]:
    raise SystemExit("Bundled Material Symbols Rounded font does not match its pinned provenance")
if not (fonts / "LICENSE.txt").is_file():
    raise SystemExit("Bundled Material Symbols Rounded license is missing")
print("bartjik: Python syntax, themes, effective configurations and bundled font checked")
