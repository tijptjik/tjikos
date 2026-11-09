"""Native notification server acceptance; private D-Bus, Wayland and synthetic data."""
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT.parent / "runtjik/tests"))
from private_compositor import private_compositor, wait_for


def main():
    host = dict(os.environ)
    def hypr(*args):
        return subprocess.check_output(["hyprctl", *args], env=host, text=True)
    old = {c["address"] for c in json.loads(hypr("-j", "clients"))}
    rule = f"notjik_native_test_{os.getpid()}"
    result = hypr("eval", f'_G.{rule}=hl.window_rule({{name="{rule}",workspace="7 silent",no_initial_focus=true,no_focus=true,match={{initial_class="^aquamarine$"}}}})')
    assert "error" not in result.lower(), result
    work = Path(tempfile.mkdtemp(prefix="notjik-native-gtk-"))
    try:
        with private_compositor(isolate_pointer=True):
            nested = wait_for(lambda: next((c for c in json.loads(hypr("-j", "clients")) if c["initialClass"] == "aquamarine" and c["address"] not in old), None))
            assert nested["workspace"]["id"] == 7
            assert json.loads(hypr("-j", "activewindow")).get("address") != nested["address"]
            subprocess.run(["hyprctl", "output", "create", "headless", "NOTJIK"], check=True, capture_output=True)
            subprocess.run(["hyprctl", "eval", 'hl.monitor({output="WAYLAND-1",disabled=true}); hl.monitor({output="NOTJIK",mode="1000x900@60",position="0x0",scale=1})'], check=True, capture_output=True)
            config = work / "config.json"
            config.write_text(json.dumps({"theme":"rose-punk", "placement":{"monitors":"all","edge":"b","position":"r","offsetY":0}, "grouping":{"enabled":False}, "history":{"logTransient":True}}))
            bus = subprocess.Popen(["dbus-daemon", "--session", "--nofork", "--print-address"], stdout=subprocess.PIPE, text=True)
            address = bus.stdout.readline().strip()
            env = {**os.environ,"DBUS_SESSION_BUS_ADDRESS":address,"NOTJIK_HOME":str(ROOT / "dist"),"NOTJIK_CONFIG":str(config),"NOTJIK_STATE_DIR":str(work/"state"),"NOTJIK_TELEGRAM_DIR":str(work/"telegram"),"NOTJIK_SOCKET":str(work/"notjik.sock"),"GTK_A11Y":"none","GSK_RENDERER":"cairo"}
            log = (work/"daemon.log").open("w")
            daemon = subprocess.Popen([str(ROOT/"dist/bin/notjik-core"),"launch"],env=env,stdout=log,stderr=log)
            def request(value):
                with socket.socket(socket.AF_UNIX) as client:
                    client.settimeout(5)
                    client.connect(env["NOTJIK_SOCKET"])
                    client.sendall(json.dumps(value).encode()+b"\n")
                    return json.loads(client.makefile("rb").readline())
            def ready():
                assert daemon.poll() is None, (work/"daemon.log").read_text()
                try:
                    v=request({"op":"status"})
                    return v if v.get("ok") and v.get("lockState")=="unlocked" and v.get("monitors") else None
                except (OSError, ValueError): return None
            def send(title, **kwargs):
                v=request({"op":"send","notice":{"title":title,"app":"notjik demo","data":{"demo":True},**kwargs}})
                assert v["ok"], v
                return v["id"]
            def dbus(code):
                return subprocess.check_output(["/usr/bin/python3","-c","import dbus\nb=dbus.SessionBus()\nn=dbus.Interface(b.get_object('org.freedesktop.Notifications','/org/freedesktop/Notifications'),'org.freedesktop.Notifications')\n"+code],env=env,text=True).strip()
            signals = (work/"signals.log").open("w")
            watcher = subprocess.Popen(["dbus-monitor", "--session", "interface=org.freedesktop.Notifications"], env=env, stdout=signals, stderr=subprocess.DEVNULL)
            try:
                wait_for(ready, timeout=15)
                presence=request({"op":"status"})
                assert isinstance(presence["idleSeconds"], int)
                assert presence["idle"] == (presence["idleSeconds"] >= 120)
                other=subprocess.run([str(ROOT/"dist/bin/notjik-gtk"),"--serve"],env=env,capture_output=True,text=True,timeout=8)
                assert other.returncode != 0 and "already has an owner" in other.stderr, other
                assert "Notjik" in dbus("print(n.GetServerInformation())")
                a=send("Replace me",id="stable",timeout=0)
                assert send("Replacement",id="stable",timeout=0)==a
                assert len(request({"op":"list"})["notices"])==1
                request({"op":"pause"})
                send("Paused expiry",id="timed",timeout=300)
                time.sleep(.6)
                assert request({"op":"status"})["queued"]==2
                request({"op":"resume"})
                wait_for(lambda: request({"op":"status"})["queued"]==1)
                native=int(dbus("print(n.Notify('notjik demo',0,'','D-Bus sample','Synthetic body',['default','Open'],{'resident':True},0))"))
                assert int(dbus(f"print(n.Notify('notjik demo',{native},'','D-Bus replacement','Synthetic body',['default','Open'],{{'resident':True}},0))"))==native
                assert request({"op":"action","id":f"dbus-{native}","index":0})["ok"]
                assert any(n["id"]==f"dbus-{native}" for n in request({"op":"list"})["notices"])
                wait_for(lambda: "member=ActionInvoked" in (work/"signals.log").read_text())
                dbus(f"n.CloseNotification({native})")
                wait_for(lambda: "member=NotificationClosed" in (work/"signals.log").read_text())
                wait_for(lambda: request({"op":"status"})["queued"]==1)
                image=int(dbus("print(n.Notify('notjik demo',0,'','Image-data','Synthetic pixels',[],{'image-data':dbus.Struct((2,2,8,True,8,4,dbus.ByteArray(bytes([255,83,166,255]*4))),signature='iiibiiay')},0))"))
                records=request({"op":"list"})["notices"]
                assert Path(next(n['image'] for n in records if n['id']==f'dbus-{image}')).is_file()
                send("Sign-in verification",type="auth-code",data={"code":"804 216","demo":True},timeout=0)
                time.sleep(1)
                layers=json.loads(subprocess.check_output(["hyprctl","-j","layers"],text=True))
                (work/"layers.json").write_text(json.dumps(layers,indent=2))
                assert "notjik" in json.dumps(layers), layers
                subprocess.run(["grim","-o","NOTJIK",str(work/"popups.png")],check=True,capture_output=True)
                marker = work / "action-result"
                command_id=send("Command action", resident=True, actions=[{"label":"Run synthetic command","command":["/usr/bin/python3","-c",f"from pathlib import Path; Path({str(marker)!r}).write_text('once')"]}])
                assert request({"op":"action","id":command_id,"index":0})["ok"]
                wait_for(lambda: marker.exists())
                assert marker.read_text()=="once"
                cli=subprocess.run([str(ROOT/"dist/bin/notjik-core"),"status"],env=env,capture_output=True,text=True,check=True)
                assert json.loads(cli.stdout)["renderer"]=="gtk"
                config.write_text(json.dumps({"theme":"monochrome","placement":{"monitors":"all"},"actions":{"blacklist":["notjik demo"]}}))
                wait_for(lambda: request({"op":"status"})["theme"]=="monochrome")
                history=request({"op":"history-data"})
                assert history["ok"] and history["items"]
                bad=request({"op":"send","notice":{"title":"Invalid","actions":[{"label":"No shell","command":"touch /tmp/notjik-bad"}]}})
                assert not bad["ok"]
                request({"op":"dismiss","id":"all"})
                wait_for(lambda: request({"op":"status"})["queued"]==0)
                print("Native GTK daemon: ownership, IPC, replacement, expiry holds, D-Bus actions/images, config reload, history, validation and layer surfaces passed")
            finally:
                daemon.terminate()
                daemon.wait(timeout=8)
                log.close()
                watcher.terminate()
                watcher.wait(timeout=5)
                signals.close()
                bus.terminate()
                bus.wait(timeout=5)
            output=(work/"daemon.log").read_text()
            assert "panicked" not in output and "Gtk-CRITICAL" not in output and "Gtk-WARNING" not in output, output
            print(f"Native GTK artifacts: {work}")
    finally:
        hypr("eval",f'if _G.{rule} then _G.{rule}:set_enabled(false); _G.{rule}=nil end')

if __name__=="__main__":main()
