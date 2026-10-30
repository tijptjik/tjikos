"""Compatibility adapter: task ownership belongs to Tasktjik."""
import os
from pathlib import Path
import sys

_source = Path(__file__).resolve().parents[1] / 'tasktjik'
_installed = Path(os.environ.get('TASKTJIK_HOME', Path.home() / '.local/lib/tasktjik'))
sys.path.insert(0, str(_source if (_source / 'tasktjik.py').is_file() else _installed))
from tasktjik import *  # noqa: F403

if __name__ == '__main__':
    main()
