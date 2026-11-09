"""Measure owned frontend/backend process trees without touching their services.

Report PSS (shared pages apportioned), RSS, CPU time and context switches. Context
switches are not hardware wakeups. Pass only identified frontend roots; unrelated
persistent desktop services are deliberately excluded and should be reported separately.
"""
import argparse
import json
import os
from pathlib import Path
import time


def snapshot(root):
    processes = {}
    for path in Path('/proc').iterdir():
        if not path.name.isdigit():
            continue
        try:
            stat = (path / 'stat').read_text().rsplit(') ', 1)[1].split()
            processes[int(path.name)] = (int(stat[1]), stat)
        except (OSError, ValueError):
            continue
    owned = {root}
    while True:
        expanded = owned | {pid for pid, (parent, _) in processes.items() if parent in owned}
        if expanded == owned:
            break
        owned = expanded
    rows = []
    for pid in sorted(owned):
        try:
            path = Path('/proc') / str(pid)
            stat = processes[pid][1]
            memory = dict(line.split(':', 1) for line in (path / 'smaps_rollup').read_text().splitlines()[1:])
            switches = 0
            for thread in (path / 'task').iterdir():
                status = dict(line.split(':', 1) for line in (thread / 'status').read_text().splitlines())
                switches += sum(int(status[k]) for k in ('voluntary_ctxt_switches', 'nonvoluntary_ctxt_switches'))
            rows.append({'pid': pid, 'start_ticks': int(stat[19]),
                         'name': (path / 'comm').read_text().strip(),
                         'pss_kib': int(memory['Pss'].split()[0]),
                         'rss_kib': int(memory['Rss'].split()[0]),
                         'cpu_ticks': int(stat[11]) + int(stat[12]),
                         'context_switches': switches})
        except (OSError, ValueError, KeyError):
            continue
    if not any(row['pid'] == root for row in rows):
        raise RuntimeError(f'Process root {root} unavailable')
    return rows


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', action='append', required=True, help='label=pid')
    parser.add_argument('--seconds', type=float, default=30)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    if args.seconds <= 0:
        parser.error('--seconds must be positive')
    roots = dict((label, int(pid)) for label, pid in (value.split('=', 1) for value in args.root))
    before = {label: snapshot(pid) for label, pid in roots.items()}
    start = time.monotonic()
    time.sleep(args.seconds)
    elapsed = time.monotonic() - start
    result = {'elapsed_seconds': elapsed, 'scope': 'frontend and descendants; external shared services excluded', 'measurements': {}}
    for label, pid in roots.items():
        after = snapshot(pid)
        initial = {(row['pid'], row['start_ticks']): row for row in before[label]}
        final = {(row['pid'], row['start_ticks']): row for row in after}
        if initial.keys() != final.keys():
            raise RuntimeError(f'{label}: process tree changed during measurement; retry when settled')
        cpu = sum(row['cpu_ticks'] - initial[key]['cpu_ticks'] for key, row in final.items()) / os.sysconf('SC_CLK_TCK')
        switches = sum(row['context_switches'] - initial[key]['context_switches'] for key, row in final.items())
        if cpu < 0 or switches < 0:
            raise RuntimeError(f'{label}: counters reset or threads exited; retry when settled')
        result['measurements'][label] = {'processes': after, 'pss_mib': sum(r['pss_kib'] for r in after) / 1024,
            'rss_mib': sum(r['rss_kib'] for r in after) / 1024, 'cpu_percent_one_core': cpu / elapsed * 100,
            'context_switches_per_second': switches / elapsed}
    args.output.write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(result, indent=2))


if __name__ == '__main__':
    main()
