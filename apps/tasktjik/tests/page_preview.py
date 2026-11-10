"""Run shared native preview contract checks with synthetic state."""
from pathlib import Path
import runpy

runpy.run_path(str(Path(__file__).resolve().parents[3] / 'packages/page-preview/tests/integration.py'), run_name='__main__')
