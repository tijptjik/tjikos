#!/usr/bin/env python3
"""Compare cached Docker ponysay with greetjik using the same pony and quote."""
import argparse
import datetime
import json
import os
import platform
import statistics
import subprocess
import time
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--runs', type=int, default=12)
    parser.add_argument('--warmups', type=int, default=2)
    parser.add_argument('--pony', default='twilight')
    parser.add_argument('--image', default='mpepping/ponysay:latest')
    parser.add_argument('--binary', default='greetjik')
    parser.add_argument('--output', type=Path, default=Path('benchmark.json'))
    args = parser.parse_args()
    if args.runs < 1 or args.warmups < 0:
        parser.error('runs must be positive and warmups nonnegative')
    image = json.loads(subprocess.check_output(['docker', 'image', 'inspect', args.image]))[0]
    commands = {
        'docker': ['xargs', '-0', 'docker', 'run', '--rm', '--pull=never', args.image, '-f', args.pony],
        'rust': [args.binary, '-f', args.pony],
    }
    quote = b'A fresh quote, a colourful pony, and a shell ready to work.\n'
    rows = []
    for iteration in range(args.warmups + args.runs):
        for name in (['docker', 'rust'] if iteration % 2 == 0 else ['rust', 'docker']):
            started = time.perf_counter_ns()
            result = subprocess.run(commands[name], input=quote, capture_output=True, check=True,
                                    timeout=30, env={**os.environ, 'TERM': 'xterm-kitty', 'COLUMNS': '80'})
            elapsed = (time.perf_counter_ns() - started) / 1_000_000
            if b'A fresh quote' not in result.stdout or len(result.stdout) < 100:
                raise RuntimeError(f'{name} did not render the expected quote and artwork')
            row = dict(implementation=name, iteration=iteration, warmup=iteration < args.warmups, ms=elapsed)
            rows.append(row)
            print(json.dumps(row), flush=True)
    summaries = {}
    for name in commands:
        values = [r['ms'] for r in rows if r['implementation'] == name and not r['warmup']]
        summaries[name] = dict(samples=len(values), median_ms=statistics.median(values),
                               mean_ms=statistics.mean(values), min_ms=min(values), max_ms=max(values))
    report = dict(timestamp=datetime.datetime.now(datetime.timezone.utc).isoformat(),
                  platform=platform.platform(), machine=platform.machine(), pony=args.pony,
                  quote=quote.decode(), docker_image=args.image, docker_image_id=image['Id'],
                  docker_repo_digests=image.get('RepoDigests', []),
                  rust_version=subprocess.check_output([args.binary, '--version'], text=True).strip(),
                  commands=commands, warmups_per_implementation=args.warmups, results=rows, summary=summaries)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(summaries, indent=2))


if __name__ == '__main__':
    main()
