"""Meter playback and the BRIO microphone via buffered PulseAudio streams.

Only runs while the panel is open. Capture processes follow each playback stream's
actual sink and are re-created on routing changes. No PipeWire graph parameters
or device volume settings are changed.
"""
import array
import json
import math
import os
import selectors
import signal
import subprocess
import time


def query(*args):
    return subprocess.check_output(['pactl', *args], text=True, timeout=2)


def watch():
    selector = selectors.DefaultSelector()
    captures = {}
    refreshed = 0
    default_sink = ''

    def stop(key):
        entry = captures.pop(key)
        selector.unregister(entry['process'].stdout)
        entry['process'].terminate()
        try:
            entry['process'].wait(timeout=1)
        except subprocess.TimeoutExpired:
            entry['process'].kill()
            entry['process'].wait()
        entry['process'].stdout.close()

    def interrupted(*_):
        raise SystemExit

    signal.signal(signal.SIGTERM, interrupted)
    try:
        while True:
            now = time.monotonic()
            if now - refreshed > 1:
                refreshed = now
                try:
                    sinks = {s['index']: s for s in json.loads(query('-f', 'json', 'list', 'sinks'))}
                    default_sink = query('get-default-sink').strip()
                    wanted = {}
                    for stream in json.loads(query('-f', 'json', 'list', 'sink-inputs')):
                        props = stream.get('properties', {})
                        title = props.get('application.name') or props.get('node.description') or props.get('node.name', '')
                        if stream.get('corked') or stream.get('mute') or 'speech-dispatcher-dummy' in (title + props.get('node.name', '')).lower():
                            continue
                        sink = sinks.get(stream['sink'])
                        if sink:
                            wanted[(stream['index'], sink['name'])] = (title, sink['monitor_source'], ['--monitor-stream=' + str(stream['index'])])
                    for source in json.loads(query('-f', 'json', 'list', 'sources')):
                        if source['name'].startswith('alsa_input.usb-046d_Logitech_BRIO_') and not source.get('mute'):
                            wanted[('microphone', source['name'])] = ('__input', source['name'], [])
                    for key in list(captures):
                        if key not in wanted or captures[key]['process'].poll() is not None:
                            stop(key)
                    for key, (title, monitor, options) in wanted.items():
                        if key in captures:
                            continue
                        process = subprocess.Popen(['parec', '--raw', '--format=float32ne', '--rate=48000', '--channels=2',
                            '--latency-msec=100', '--process-time-msec=50', '--client-name=Audio panel levels',
                            '--stream-name=' + ('Microphone level' if title == '__input' else 'Playback level'),
                            '--device=' + monitor] + options,
                            stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
                        os.set_blocking(process.stdout.fileno(), False)
                        captures[key] = dict(process=process, title=title, peak=0, tail=b'')
                        selector.register(process.stdout, selectors.EVENT_READ, key)
                except (OSError, ValueError, subprocess.SubprocessError):
                    for key in list(captures):
                        stop(key)
            for entry in captures.values():
                entry['peak'] *= 0.65
            for event, _ in selector.select(timeout=0.05):
                entry = captures[event.data]
                data = os.read(event.fileobj.fileno(), 65536)
                if not data:
                    stop(event.data)
                    continue
                data = entry['tail'] + data
                end = len(data) - len(data) % 4
                entry['tail'] = data[end:]
                samples = array.array('f', data[:end])
                peak = max((abs(v) for v in samples if math.isfinite(v)), default=0)
                entry['peak'] = max(entry['peak'], min(1, peak))
            levels = {}
            for (_, sink), entry in captures.items():
                levels[entry['title']] = max(levels.get(entry['title'], 0), entry['peak'])
                if sink == default_sink:
                    levels['__output'] = max(levels.get('__output', 0), entry['peak'])
            print(json.dumps(levels), flush=True)
            # Bound the update rate even when several captures are readable.
            time.sleep(0.03)
    finally:
        for key in list(captures):
            stop(key)
        selector.close()


if __name__ == '__main__':
    watch()
