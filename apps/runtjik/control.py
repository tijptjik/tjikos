"""Short-lived, serialized controller; no resident controller process."""
import fcntl
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import time

HOME = Path(os.environ['RUNTJIK_HOME']).resolve()
QML = str(HOME / 'qml')


def identity(pid):
    try:
        fields = Path(f'/proc/{pid}/stat').read_text().rsplit(')', 1)[1].split()
        if fields[0] == 'Z':
            return None
        return fields[19]
    except (OSError, IndexError):
        return None


def ipc(method):
    try:
        reply = subprocess.run(['quickshell', 'ipc', '--path', QML, 'call', 'runtjik', method],
                               capture_output=True, text=True, timeout=1)
        if reply.returncode:
            return None
        return json.loads(reply.stdout) if method == 'status' else True
    except (OSError, ValueError, subprocess.TimeoutExpired):
        return None


def alive(record):
    if not record or identity(record['pid']) != record['start']:
        return False
    try:
        args = Path(f"/proc/{record['pid']}/cmdline").read_bytes().split(b'\0')
        return QML.encode() in args
    except OSError:
        return False


def stop(record):
    if not ipc('quit'):
        ipc('close')
    deadline = time.monotonic() + 3
    while time.monotonic() < deadline:
        if not alive(record) and ipc('status') is None:
            return
        time.sleep(.025)
    if alive(record):
        os.kill(record['pid'], signal.SIGTERM)
        deadline = time.monotonic() + 2
        while alive(record) and time.monotonic() < deadline:
            time.sleep(.025)
    if alive(record) or ipc('status') is not None:
        raise RuntimeError('Previous launcher did not close; refusing to start a duplicate')


def control(action):
    runtime = os.environ.get('XDG_RUNTIME_DIR')
    if not runtime or not os.environ.get('WAYLAND_DISPLAY'):
        raise RuntimeError('Open Runtjik from a Wayland desktop session')
    key = hashlib.sha256((QML + '\0' + os.environ['WAYLAND_DISPLAY']).encode()).hexdigest()[:16]
    directory = Path(runtime) / 'runtjik' / key
    directory.mkdir(parents=True, exist_ok=True, mode=0o700)
    state_path = directory / 'process.json'
    log_path = Path(os.environ.get('XDG_STATE_HOME', str(Path.home()/'.local/state'))) / 'runtjik' / 'launcher.log'
    log_path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    with (directory / 'control.lock').open('a') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        try:
            record = json.loads(state_path.read_text())
        except (OSError, ValueError):
            record = None
        current = ipc('status')
        if current and current.get('pid'):
            record = {'pid': current['pid'], 'start': identity(current['pid'])}
        if action == 'quit':
            stop(record)
            state_path.unlink(missing_ok=True)
            return
        if action == 'close' or (action == 'toggle' and current and current.get('visible', True)):
            if current:
                ipc('close')
                deadline = time.monotonic() + 3
                while time.monotonic() < deadline:
                    hidden = ipc('status')
                    if hidden is None or (not hidden.get('visible', True) and hidden.get('ready')):
                        return
                    time.sleep(.005)
                raise RuntimeError('Launcher did not finish hiding')
            elif alive(record):
                stop(record)
                state_path.unlink(missing_ok=True)
            return
        if current and current.get('ready'):
            if current.get('visible', True):
                return
            if ipc('reopen'):
                deadline = time.monotonic() + 3
                while time.monotonic() < deadline:
                    shown = ipc('status')
                    if shown and shown.get('visible') and shown.get('ready'):
                        errors = [shown.get(key) for key in ('placementError', 'clickWatchError') if shown.get(key)]
                        if errors:
                            raise RuntimeError('; '.join(errors))
                        return
                    time.sleep(.005)
            # Recover if the resident process died or failed to remap.
        # A failed/half-started process must not turn later requests into silent no-ops.
        if current or alive(record):
            stop(record)
        state_path.unlink(missing_ok=True)
        if log_path.exists() and log_path.stat().st_size > 256 * 1024:
            log_path.replace(log_path.with_suffix('.previous.log'))
        with log_path.open('a') as log:
            os.chmod(log_path, 0o600)
            log.write(time.strftime('\n%Y-%m-%d %H:%M:%S ') + 'Starting Runtjik\n')
            log.flush()
            process = subprocess.Popen(['quickshell', '--path', QML, '--no-duplicate'],
                                       stdin=subprocess.DEVNULL, stdout=log, stderr=log, start_new_session=True)
        record = {'pid': process.pid, 'start': identity(process.pid)}
        state_path.write_text(json.dumps(record))
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            if process.poll() is not None:
                state_path.unlink(missing_ok=True)
                raise RuntimeError(f'Launcher exited during startup; see {log_path}')
            current = ipc('status')
            if current and current.get('ready'):
                errors = [current.get(key) for key in ('placementError', 'clickWatchError') if current.get(key)]
                if errors:
                    stop(record)
                    state_path.unlink(missing_ok=True)
                    raise RuntimeError('; '.join(errors))
                return
            time.sleep(.025)
        stop(record)
        state_path.unlink(missing_ok=True)
        raise RuntimeError(f'Launcher did not become ready within 10 seconds; see {log_path}')


def main():
    try:
        control(sys.argv[1])
    except (OSError, RuntimeError) as exc:
        message = str(exc)
        print('Runtjik: ' + message, file=sys.stderr)
        try:
            subprocess.run(['notify-send', '--app-name=Runtjik', 'Runtjik could not open', message],
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=2)
        except (OSError, subprocess.TimeoutExpired):
            pass
        return 1
    return 0


if __name__ == '__main__':
    sys.exit(main())
