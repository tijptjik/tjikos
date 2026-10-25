#!/usr/bin/env python3
"""Build a dependency-free, self-contained Python executable."""

import argparse
from pathlib import Path
import tempfile
import shutil
import zipapp

root = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--output", type=Path, default=root / "dist/tjikup")
output = parser.parse_args().output
output.parent.mkdir(parents=True, exist_ok=True)
with tempfile.TemporaryDirectory() as temporary:
    source = Path(temporary) / "src"
    shutil.copytree(
        root / "src", source, ignore=shutil.ignore_patterns("__pycache__", "*.pyc")
    )
    # zipapp's main= generates a call without SystemExit and loses failure codes.
    (source / "__main__.py").write_text(
        "from tjikup import cli\nraise SystemExit(cli())\n"
    )
    zipapp.create_archive(source, output, interpreter="/usr/bin/env python3")
