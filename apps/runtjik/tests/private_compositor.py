"""Isolated Wayland environment for Runtjik UI checks and benchmarks."""
from contextlib import contextmanager
import ctypes
import errno
import os
from pathlib import Path
import select
import shutil
import signal
import subprocess
import tempfile
import time


# Some Python builds omit the pidfd APIs even on Linux. Use libc so cleanup
# always signals a stable process handle, never a potentially recycled PID.
_libc = ctypes.CDLL(None, use_errno=True)


def _pidfd_call(name, *args):
    result = getattr(_libc, name)(*args)
    if result < 0:
        error = ctypes.get_errno()
        raise OSError(error, os.strerror(error))
    return result


def _runtime_clients(runtime, compositor_pid):
    marker = os.fsencode(f"XDG_RUNTIME_DIR={runtime}")
    for path in Path("/proc").iterdir():
        if not path.name.isdecimal() or int(path.name) in (os.getpid(), compositor_pid):
            continue
        fd = None
        try:
            if path.stat().st_uid != os.getuid():
                continue
            fd = _pidfd_call("pidfd_open", int(path.name), 0)
            if marker in (path / "environ").read_bytes().split(b"\0"):
                yield int(path.name), fd
                fd = None  # The caller now owns this handle.
        except (FileNotFoundError, ProcessLookupError, PermissionError):
            pass
        finally:
            if fd is not None:
                os.close(fd)


def stop_runtime_clients(runtime, compositor_pid, grace=2):
    """Stop even detached clients belonging to this private runtime only."""
    clients = {}
    poll = select.poll()
    deadline = time.monotonic() + grace
    try:
        while True:
            for pid, fd in _runtime_clients(runtime, compositor_pid):
                if pid in clients:
                    os.close(fd)
                    continue
                clients[pid] = fd
                poll.register(fd, select.POLLIN)
                try:
                    _pidfd_call("pidfd_send_signal", fd, signal.SIGTERM, None, 0)
                except ProcessLookupError:
                    pass
            for fd, _ in poll.poll(0):
                poll.unregister(fd)
                os.close(fd)
                clients = {pid: handle for pid, handle in clients.items() if handle != fd}
            if not clients:
                return
            if time.monotonic() >= deadline:
                for fd in clients.values():
                    try:
                        _pidfd_call("pidfd_send_signal", fd, signal.SIGKILL, None, 0)
                    except ProcessLookupError:
                        pass
                if time.monotonic() >= deadline + 3:
                    raise RuntimeError(f"Private compositor clients did not exit: {list(clients)}")
            time.sleep(.05)
    finally:
        for fd in clients.values():
            os.close(fd)


def wait_for(fn, timeout=12):
    end = time.monotonic() + timeout
    last = None
    while time.monotonic() < end:
        try:
            value = fn()
            if value:
                return value
        except (OSError, ValueError) as error:
            last = error
        time.sleep(.08)
    raise ValueError("Timed out waiting for private compositor: " + str(last))


@contextmanager
def private_compositor(isolate_pointer=False):
    """Isolate capture geometry and input from desktop monitor automation."""
    directory=Path(tempfile.mkdtemp(prefix="bc-"))
    runtime=directory/"r"
    runtime.mkdir(mode=0o700)
    fontconfig=Path.home()/".config/fontconfig"
    if fontconfig.exists():shutil.copytree(fontconfig,directory/"config/fontconfig")
    config=directory/"hyprland.lua"
    config.write_text('hl.monitor({output="",mode="1000x680@60",position="0x0",scale=1})\n'
                      'hl.config({general={border_size=0},decoration={rounding=12,shadow={enabled=false}}})\n'
                      + ('hl.device({name="wl_pointer",enabled=false})\n' if isolate_pointer else ''))
    keys=("XDG_RUNTIME_DIR","WAYLAND_DISPLAY","HYPRLAND_INSTANCE_SIGNATURE","XDG_CONFIG_HOME","XDG_CACHE_HOME","XDG_DATA_HOME","TASKTJIK_HOME")
    previous={key:os.environ.get(key) for key in keys}
    env={**os.environ,"TASKTJIK_HOME":str(Path(__file__).resolve().parents[2]/"tasktjik"),"XDG_RUNTIME_DIR":str(runtime),"XDG_CONFIG_HOME":str(directory/"config"),"XDG_CACHE_HOME":str(directory/"cache"),"XDG_DATA_HOME":str(directory/"data")}
    parent_display=Path(os.environ.get("WAYLAND_DISPLAY","wayland-1"))
    env["WAYLAND_DISPLAY"]=str(parent_display if parent_display.is_absolute() else Path(os.environ["XDG_RUNTIME_DIR"])/parent_display)
    env.pop("HYPRLAND_INSTANCE_SIGNATURE",None)
    with (directory/"compositor.log").open("w") as log:
        process=subprocess.Popen(["Hyprland","--config",str(config)],env=env,stdout=log,stderr=log,start_new_session=True)
        print("Private compositor: "+str(directory),flush=True)
        try:
            signature=wait_for(lambda:next((p.parent.name for p in (runtime/"hypr").glob("*/.socket.sock")),None),timeout=20)
            os.environ.update({key:env[key] for key in keys if key in env})
            display=wait_for(lambda:next((p.name for p in runtime.glob("wayland-*") if not p.name.endswith(".lock")),None))
            os.environ.update(WAYLAND_DISPLAY=display,HYPRLAND_INSTANCE_SIGNATURE=signature)
            yield
        finally:
            try:
                try:
                    # Close clients while their display is still alive. Detached
                    # viewers and clipboard owners can outlive the test launcher.
                    stop_runtime_clients(runtime, process.pid)
                finally:
                    process.terminate()
                    try:
                        process.wait(timeout=8)
                    except subprocess.TimeoutExpired:
                        process.kill()
                        process.wait()
                    stop_runtime_clients(runtime, process.pid)
            finally:
                for key, value in previous.items():
                    if value is None:
                        os.environ.pop(key, None)
                    else:
                        os.environ[key] = value
