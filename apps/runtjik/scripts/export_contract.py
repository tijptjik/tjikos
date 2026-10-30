"""Export reference defaults and presentation metadata; never user configuration."""
import json
from pathlib import Path
import sys
root = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(root))
import runtjik as r
value = {name: getattr(r, name) for name in ('DEFAULTS', 'APP_NAMES', 'APP_ICONS', 'COMMAND_ICONS', 'HISTORY_ALIASES')}
value.update({name: sorted(getattr(r, name)) for name in ('HIDDEN_APPS', 'HIDDEN_COMMANDS', 'GUI_WRAPPERS', 'GUI_COMMANDS')})
value['demo_entries'] = [r.asdict(e) for e in r.demo_entries()]
value['demo_windows'] = r.demo_windows()
value['casefold'] = {chr(n): chr(n).casefold() for n in range(sys.maxunicode + 1)
                     if chr(n).casefold() != chr(n).lower()}
rendered = json.dumps(value, indent=2) + '\n'
if '--check' in sys.argv:
    if (root / 'src/contract.json').read_text() != rendered:
        raise SystemExit('Rust contract is stale; run scripts/export_contract.py')
else:
    (root / 'src/contract.json').write_text(rendered)
