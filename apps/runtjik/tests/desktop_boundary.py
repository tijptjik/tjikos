"""Keep compositor IPC out of Rust application/controller/widget code."""
from pathlib import Path
import re

root = Path(__file__).resolve().parents[1]
for path in (root / "src").rglob("*.rs"):
    assert not re.search(r'hyprctl|HYPRLAND_INSTANCE_SIGNATURE|initialClass|focusHistoryID|specialWorkspace|activeWorkspace|\.socket(?:2)?\.sock|hl\.(?:dsp|dispatch|get_)', path.read_text()), path
print("Rust compositor boundary passed")
