"""Isolated Wayland environment for Runtjik UI checks and benchmarks."""
from contextlib import contextmanager
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time


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
def private_compositor():
    """Isolate capture geometry and input from desktop monitor automation."""
    directory=Path(tempfile.mkdtemp(prefix="bc-"))
    runtime=directory/"r"
    runtime.mkdir(mode=0o700)
    fontconfig=Path.home()/".config/fontconfig"
    if fontconfig.exists():shutil.copytree(fontconfig,directory/"config/fontconfig")
    config=directory/"hyprland.lua"
    config.write_text('hl.monitor({output="",mode="1000x680@60",position="0x0",scale=1})\n'
                      'hl.config({general={border_size=0},decoration={rounding=12,shadow={enabled=false}}})\n')
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
            process.terminate()
            try:process.wait(timeout=8)
            except subprocess.TimeoutExpired:process.kill();process.wait()
            for key,value in previous.items():
                if value is None:os.environ.pop(key,None)
                else:os.environ[key]=value
