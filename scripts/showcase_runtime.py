"""Shared transport for terminal-tool showcases in a separately isolated lab.

Desktop applications use their own native fixtures. Do not run this nested-compositor
recorder on the managed desktop; it is retained for terminal showcase production only.
"""
from contextlib import contextmanager
import json
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import time

ROOT = Path(__file__).resolve().parents[1]
HERE = Path(__file__).resolve().parent
WIDTH, HEIGHT, FPS = 1200, 720, 60

def checked(args, **kwargs):
    return subprocess.check_output(args, text=True, stderr=subprocess.PIPE, **kwargs).strip()



def wait_for(fn, timeout=15):
    deadline = time.monotonic() + timeout
    last = None
    while time.monotonic() < deadline:
        try:
            result = fn()
            if result:
                return result
        except (OSError, ValueError, subprocess.SubprocessError) as error:
            last = error
        time.sleep(.08)
    raise ValueError("Timed out waiting for native showcase: " + str(last))



@contextmanager
def native_session(directory, scale=1):
    """A nested compositor, private runtime, configuration, state, and D-Bus."""
    runtime = directory / "r"
    runtime.mkdir(mode=0o700)
    config_home = directory / "config"
    fonts = Path.home() / ".config/fontconfig"
    if fonts.exists():
        shutil.copytree(fonts, config_home / "fontconfig")
    styles = Path.home() / ".local/share/quickshell/styles"
    if styles.exists():
        shutil.copytree(styles, directory / "data/quickshell/styles")
    config = directory / "hyprland.lua"
    compositor_config = ('hl.monitor({output="",mode="1200x720@60",position="0x0",scale=1})\n'
                      'hl.monitor({output="HEADLESS-1",mode="1200x720@60",position="10000x10000",scale=1})\n'
                      'hl.config({general={border_size=0},decoration={rounding=0,shadow={enabled=false}},misc={disable_watchdog_warning=true,disable_hyprland_logo=true,disable_splash_rendering=true}})\n')
    config.write_text(compositor_config.replace("scale=1}", "scale="+str(scale)+"}"))
    env = {**os.environ, "XDG_RUNTIME_DIR": str(runtime), "XDG_CONFIG_HOME": str(config_home),
           "XDG_CACHE_HOME": str(directory / "cache"), "XDG_DATA_HOME": str(directory / "data"),
           "XDG_STATE_HOME": str(directory / "state"), "QT_LOGGING_RULES": "qt.qpa.wayland.textinput=false"}
    parent_display = Path(os.environ["WAYLAND_DISPLAY"])
    env["WAYLAND_DISPLAY"] = str(parent_display if parent_display.is_absolute() else Path(os.environ["XDG_RUNTIME_DIR"]) / parent_display)
    env.pop("HYPRLAND_INSTANCE_SIGNATURE", None)
    env.pop("DBUS_STARTER_ADDRESS", None)
    env.pop("DBUS_STARTER_BUS_TYPE", None)
    children = []
    with (directory / "compositor.log").open("w") as log:
        try:
            compositor = subprocess.Popen(["Hyprland", "--config", str(config)], env=env, stdout=log, stderr=log, start_new_session=True)
            children.append(compositor)
            signature = wait_for(lambda: next((p.parent.name for p in (runtime / "hypr").glob("*/.socket.sock")), None))
            display = wait_for(lambda: next((p.name for p in runtime.glob("wayland-*") if not p.name.endswith(".lock")), None))
            env.update(WAYLAND_DISPLAY=display, HYPRLAND_INSTANCE_SIGNATURE=signature)
            before = {m["name"] for m in json.loads(checked(["hyprctl", "-j", "monitors"], env=env))}
            checked(["hyprctl", "output", "create", "headless"], env=env)
            monitor = wait_for(lambda: next((m["name"] for m in json.loads(checked(["hyprctl", "-j", "monitors"], env=env)) if m["name"] not in before), None))
            result = checked(["hyprctl", "eval", "hl.monitor({output=" + json.dumps(monitor) + ',mode="1200x720@60",position="10000x10000",scale='+str(scale)+'})'], env=env)
            if result != "ok":
                raise ValueError(result)
            # Disable the nested window after the headless output is ready.
            for name in before:
                checked(["hyprctl", "eval", "hl.monitor({output=" + json.dumps(name) + ",disabled=true})"], env=env)
            checked(["hyprctl", "eval", "hl.dispatch(hl.dsp.focus({monitor=" + json.dumps(monitor) + "}))"], env=env)
            wait_for(lambda: any(m["name"] == monitor and m["width"] == WIDTH and m["height"] == HEIGHT for m in json.loads(checked(["hyprctl", "-j", "monitors"], env=env))))
            bus = subprocess.Popen(["dbus-daemon", "--nofork", "--config-file=" + str(HERE / "showcase-bus.conf"), "--print-address=1", "--address=unix:path=" + str(directory / "bus")], env=env, stdout=subprocess.PIPE, text=True)
            children.append(bus)
            address = bus.stdout.readline().strip()
            if address != "unix:path=" + str(directory / "bus") and not address.startswith("unix:path=" + str(directory / "bus") + ","):
                raise ValueError("Private D-Bus did not start")
            env["DBUS_SESSION_BUS_ADDRESS"] = address
            yield env, monitor
        finally:
            for process in reversed(children):
                if process.poll() is None:
                    process.terminate()
                    try:
                        process.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        process.kill()
                        process.wait()



def build_pointer(directory):
    protocol = HERE / "protocols/virtual-pointer.xml"
    checked(["wayland-scanner", "client-header", str(protocol), str(directory / "pointer.h")])
    checked(["wayland-scanner", "private-code", str(protocol), str(directory / "pointer.c")])
    flags = shlex.split(checked(["pkg-config", "--cflags", "--libs", "wayland-client"]))
    target = directory / "pointer"
    checked(["cc", "-I" + str(directory), str(HERE / "showcase_pointer.c"), str(directory / "pointer.c"), "-o", str(target), *flags])
    return target



class Director:
    def command(self, command):
        self.pointer.stdin.write(command + "\n")
        self.pointer.stdin.flush()
        if self.pointer.stdout.readline().strip() != "ok":
            raise ValueError("Native pointer disconnected")


    def move(self, x, y, duration=.7):
        start_x, start_y = self.position
        started = time.monotonic()
        frames = max(1, round(duration * FPS))
        for frame in range(1, frames + 1):
            t = frame / frames
            eased = t*t*t*(t*(t*6-15)+10)
            self.command(f"M {round(start_x+(x-start_x)*eased)} {round(start_y+(y-start_y)*eased)}")
            time.sleep(max(0, started + frame/FPS - time.monotonic()))
        self.position = x, y


    def frame(self, name):
        checked(["grim", "-o", self.env["SHOWCASE_MONITOR"], str(self.review / (name + ".png"))], env=self.env)
