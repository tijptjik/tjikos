"""Own one recorder independently of launcher reloads; stop only our child via IPC."""
import fcntl
import json
import os
from pathlib import Path
import signal
import socket
import subprocess
import sys
import time

from desktop_plugins import hypr, runtime_socket, visible_workspace


def recorder_argv(spec):
    if spec['mode'] == 'window':
        return ['gpu-screen-recorder', '-w', 'portal', '-restore-portal-session', 'no',
                '-f', str(spec['fps']), '-k', 'h264', '-fallback-cpu-encoding', 'yes',
                *(['-ac', 'aac', '-a', 'default_output'] if spec['audio'] else []), '-o', spec['file']]
    audio = []
    if spec['audio']:
        sink = subprocess.check_output(['pactl', 'get-default-sink'], text=True, timeout=3).strip()
        if not sink:
            raise ValueError('No system audio output is available')
        audio = ['--audio=' + sink + '.monitor']
    return ['wf-recorder', '--no-dmabuf', '-r', str(spec['fps']),
            *audio, *(['-o', spec['output']] if spec['output'] else []),
            *(['-g', spec['area']] if spec['area'] else []), '-f', spec['file']]


def main(spec):
    os.umask(0o077)
    path = runtime_socket()
    child = None
    stopped = False
    stopping_at = None

    def stop(*_):
        nonlocal stopped, stopping_at
        if not stopped:
            stopped = True
            stopping_at = time.monotonic()
            if child is not None and child.poll() is None:
                child.send_signal(signal.SIGINT)

    signal.signal(signal.SIGTERM, stop)
    signal.signal(signal.SIGINT, stop)
    with path.with_suffix('.lock').open('a') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        path.unlink(missing_ok=True)
        logdir = Path(os.environ.get('XDG_STATE_HOME', str(Path.home() / '.local/state'))) / 'runtjik'
        logdir.mkdir(parents=True, exist_ok=True, mode=0o700)
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as server, (logdir / 'capture.log').open('w') as log:
            server.bind(str(path))
            server.listen(4)
            server.settimeout(.2)
            try:
                child = subprocess.Popen(recorder_argv(spec), stdout=log, stderr=log)
                if stopped:
                    child.send_signal(signal.SIGINT)
                time.sleep(.3)
                if child.poll() is not None:
                    raise RuntimeError('Recorder exited during startup; inspect ' + str(logdir / 'capture.log'))
                print(json.dumps({'ready': True}), flush=True)
                while child.poll() is None:
                    try:
                        connection, _ = server.accept()
                    except socket.timeout:
                        connection = None
                    if connection is not None:
                        with connection:
                            connection.settimeout(.5)
                            try:
                                action = connection.recv(64).decode().strip()
                                if action == 'stop':
                                    stop()
                                connection.sendall(json.dumps({'file': spec['file'], 'mode': spec['mode'], 'stopping': stopped}).encode())
                            except (OSError, UnicodeError):
                                pass
                    if spec['mode'] == 'workspace' and not stopped:
                        try:
                            monitor = next((m for m in hypr('monitors') if m['name'] == spec['output']), None)
                            if not monitor or visible_workspace(monitor)['id'] != spec['workspace']:
                                stop()
                        except Exception:
                            stop()
                    if stopping_at is not None and time.monotonic() - stopping_at > 20:
                        # A hung encoder must not keep capturing after Stop.
                        child.kill()
                        child.wait()
                        raise RuntimeError('Recorder did not finalize within 20 seconds; output may be incomplete')
                code = child.wait()
                message = ('Saved ' + spec['file']) if code in (0, 130, -signal.SIGINT) and Path(spec['file']).is_file() and Path(spec['file']).stat().st_size > 0 else 'Recording failed; inspect ' + str(logdir / 'capture.log')
                try:
                    subprocess.run(['notify-send', '--app-name=Runtjik', 'Screen recording', message], timeout=3, check=False)
                except (OSError, subprocess.TimeoutExpired):
                    pass
            finally:
                stop()
                if child is not None and child.poll() is None:
                    try:
                        child.wait(timeout=20)
                    except subprocess.TimeoutExpired:
                        child.kill()
                        child.wait()
                path.unlink(missing_ok=True)


if __name__ == '__main__':
    try:
        main(json.loads(sys.argv[1]))
    except Exception as exc:
        print(json.dumps({'error': str(exc)}), flush=True)
        sys.exit(1)
