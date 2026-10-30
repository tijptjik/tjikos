"""Synthetic worker benchmark; no compositor, network or user configuration."""
import json
import os
from pathlib import Path
import select
import statistics
import subprocess
import sys
import tempfile
import time

APP = Path(__file__).resolve().parents[1]

def read(process):
    assert select.select([process.stdout], [], [], 10)[0], 'Worker timeout'
    return json.loads(process.stdout.readline())

def send(process, value):
    process.stdin.write(json.dumps(value).encode() + b'\n')
    process.stdin.flush()


def trial(command, env):
    start = time.perf_counter()
    process = subprocess.Popen(command + ['serve', '--demo'], env=env, stdin=subprocess.PIPE,
                               stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, bufsize=0)
    try:
        send(process, {'op':'screens', 'screens':[{'name':'SYNTHETIC','width':1920,'height':1080,'scale':1}]})
        while not read(process).get('bars'):
            pass
        startup = (time.perf_counter() - start) * 1000
        time.sleep(.1)
        while select.select([process.stdout], [], [], 0)[0]:
            read(process)
        timings = []
        for _ in range(30):
            start = time.perf_counter()
            send(process, {'op':'reload'})
            while not read(process).get('bars'):
                pass
            timings.append((time.perf_counter() - start) * 1000)
        rss = int(next(line.split()[1] for line in Path(f'/proc/{process.pid}/status').read_text().splitlines() if line.startswith('VmRSS:')))
        return {'startup_ms':startup,'reload_median_ms':statistics.median(timings),'rss_kib':rss}
    finally:
        process.stdin.close()
        process.wait(timeout=5)
        process.stdout.close()


def main():
    report = {'method':'First launch (OS caches not flushed), then 10 warm launches; 30 synchronous config reloads per worker. Synthetic minimal preset; backend RSS excludes Quickshell.'}
    with tempfile.TemporaryDirectory(prefix='bartjik-benchmark-') as temporary:
        env = {**os.environ, 'BARTJIK_HOME':str(APP), 'BARTJIK_CONFIG':str(APP/'examples/minimal.json'),
               'XDG_CONFIG_HOME':temporary, 'XDG_CACHE_HOME':temporary, 'XDG_STATE_HOME':temporary,
               'XDG_RUNTIME_DIR':temporary}
        env.pop('HYPRLAND_INSTANCE_SIGNATURE', None)
        env.pop('BARTJIK_DEMO_FIXTURE', None)
        for name, command in [('python',[sys.executable,'-B',str(APP/'tests/reference/bartjik.py')]),
                              ('rust',[str(APP/'target/release/bartjik-core')])]:
            samples = [trial(command,env) for _ in range(11)]
            report[name] = {'first_launch_ms':samples[0]['startup_ms'],
                'warm_launch_median_ms':statistics.median(s['startup_ms'] for s in samples[1:]),
                'reload_median_ms':statistics.median(s['reload_median_ms'] for s in samples),
                'rss_median_kib':statistics.median(s['rss_kib'] for s in samples),'samples':samples}
    print(json.dumps(report,indent=2))

if __name__ == '__main__': main()
