"""Export reference defaults and presentation metadata; never user configuration."""
import json
import html.entities
from pathlib import Path
import sys
root = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(root))
import runtjik as r
value = {name: getattr(r, name) for name in ('DEFAULTS', 'APP_NAMES', 'APP_ICONS', 'COMMAND_ICONS', 'HISTORY_ALIASES')}
value.update({name: sorted(getattr(r, name)) for name in ('HIDDEN_APPS', 'HIDDEN_COMMANDS', 'GUI_WRAPPERS', 'GUI_COMMANDS')})
value['demo_entries'] = [r.asdict(e) for e in r.demo_entries()]
value['demo_windows'] = r.demo_windows()
value['html_entities'] = html.entities.html5
value['featured'] = list(r.FEATURED_SETTINGS.items())
value['shortcuts'] = r.SHORTCUTS
value['plugin_order'] = list(r.DEFAULTS['plugin_settings'])
value['option_order'] = {name: list(options) for name, options in r.DEFAULTS['plugin_settings'].items()}
value['providers'] = [{'name': name, 'commands': list(provider.commands.items()),
                       'aliases': list(getattr(provider, 'aliases', {}).items()),
                       'icon': getattr(provider, 'icon', 'accessories-dictionary'),
                       'mode': getattr(provider, 'mode', '')}
                      for name, provider in (r.BUNDLED_PROVIDERS | r.OPTIONAL_PROVIDERS).items()]
value['casefold'] = {chr(n): chr(n).casefold() for n in range(sys.maxunicode + 1)
                     if chr(n).casefold() != chr(n).lower()}
for provider in value['providers']:
    icon = Path(provider['icon'])
    if icon.is_absolute() and icon.is_relative_to(root):
        provider['icon'] = '@app/' + str(icon.relative_to(root))
rendered = json.dumps(value, indent=2) + '\n'
if '--check' in sys.argv:
    if (root / 'src/contract.json').read_text() != rendered:
        raise SystemExit('Rust contract is stale; run scripts/export_contract.py')
else:
    (root / 'src/contract.json').write_text(rendered)
