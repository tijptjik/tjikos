"""Allowlisted cooperative applications for the private Runtjik capture only."""
import json
import os
from pathlib import Path

CLASSES={'firefox':'hk.type.RuntjikShowcaseFirefox','kitty':'hk.type.RuntjikShowcaseTerminal','obsidian':'hk.type.RuntjikShowcaseNotes'}


def clients():
    from runtjik import windows
    allowed=json.loads(Path(os.environ['RUNTJIK_SHOWCASE_FIXTURES']).read_text())
    pids={row['pid'] for row in allowed}
    return [client for client in windows() if client.get('pid') in pids and client.get('class') in CLASSES.values()]


def close(launcher,entry,address='',pending=None):
    from runtjik import Store,matched_windows
    targets=matched_windows(entry,clients(),launcher.settings)
    if not targets:raise ValueError('Only the private showcase fixtures may be closed')
    if pending:raise ValueError('The showcase demonstrates graceful close only')
    launcher.store=launcher.store or Store()
    launcher.settings['history']['enabled']=False
    launcher.demo=False
    try:
        return launcher.close_app(entry,address,pending)
    finally:
        launcher.demo=True
