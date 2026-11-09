"""GTK demo and recording acceptance, with no QML in the test installation."""
import json
import os
from pathlib import Path
import socket
import shutil
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT.parent / "runtjik/tests"))
from private_compositor import private_compositor, wait_for


def main():
    work = Path(tempfile.mkdtemp(prefix="notjik-gtk-demo-",dir="/var/tmp"))
    host = dict(os.environ)
    def hypr(*args):
        return subprocess.check_output(["hyprctl", *args], env=host, text=True)
    old = {c["address"] for c in json.loads(hypr("-j", "clients"))}
    rule = f"notjik_demo_test_{os.getpid()}"
    hypr("eval", f'_G.{rule}=hl.window_rule({{name="{rule}",workspace="7 silent",no_initial_focus=true,no_focus=true,match={{initial_class="^aquamarine$"}}}})')
    try:
        with private_compositor(isolate_pointer=True):
            nested = wait_for(lambda: next((c for c in json.loads(hypr("-j", "clients")) if c["initialClass"] == "aquamarine" and c["address"] not in old), None))
            assert nested["workspace"]["id"] == 7
            assert json.loads(hypr("-j", "activewindow")).get("address") != nested["address"]
            subprocess.run(["hyprctl", "output", "create", "headless", "NOTJIK"], check=True, capture_output=True)
            subprocess.run(["hyprctl", "eval", 'hl.monitor({output="WAYLAND-1",disabled=true}); hl.monitor({output="NOTJIK",mode="1000x680@60",position="0x0",scale=1})'], check=True, capture_output=True)
            app = work / "app"
            app.mkdir()
            for name in ["lib", "assets", "themes"]:
                (app / name).symlink_to(ROOT / "dist" / name)
            shutil.copytree(ROOT/'dist/bin',app/'bin')
            shutil.copytree(ROOT/'dist/examples',app/'examples',ignore=shutil.ignore_patterns('*.qml','demo-patches.json','reference-artwork.js'))
            assert not (app / "qml").exists()
            config = work / "config.json"
            config.write_text(json.dumps({"theme":"rose-punk", "placement":{"primaryMonitor":"NOTJIK"}}))
            env = {**os.environ,"PATH":str(Path.home()/".cache/notjik/comparison-reference/usr/bin")+":"+os.environ["PATH"],"NOTJIK_HOME":str(app),"NOTJIK_CONFIG":str(config),"NOTJIK_SOCKET":str(work/"comparison/notjik.sock"),"NOTJIK_STATE_DIR":str(work/"state"),"NOTJIK_DEMO_CYCLE_DIR":str(work/"comparison"),"NOTJIK_DEMO_CYCLE_MONITOR":"NOTJIK","NOTJIK_GTK_LAYOUT_TRACE":str(work/"layout.jsonl"),"GTK_A11Y":"none","GSK_RENDERER":"cairo"}
            launcher_env={**env,"HOME":str(work/'empty-home')}
            launcher_env.pop('NOTJIK_HOME',None)
            subprocess.run([str(app/'bin/notjikctl'),'config'],env=launcher_env,check=True,capture_output=True)
            binary = str(app / "bin/notjik-core")
            def cli(*args, timeout=45):
                result = subprocess.run([binary,*args],env=env,capture_output=True,text=True,timeout=timeout)
                assert result.returncode == 0, result.stdout + result.stderr
                return result.stdout
            def request(value):
                with socket.socket(socket.AF_UNIX) as client:
                    client.connect(str(work/"comparison/notjik.sock"))
                    client.sendall(json.dumps(value).encode()+b"\n")
                    result=json.loads(client.makefile().readline())
                    assert result['ok'],result
                    return result
            try:
                cli("demo", "standard")
                assert request({"op":"status"})['renderer']=='gtk'
                assert not (work/'comparison/app/qml').exists()
                for name in json.loads((ROOT/'examples/demo-order.json').read_text()):
                    cli("demo",name)
                    assert request({"op":"list"})['notices'],name
                sentinel=request({"op":"send","notice":{"app":"Acceptance sentinel","id":"keep","title":"Keep this ordinary notification","timeout":0}})["id"]
                cli("demo","mix")
                wait_for(lambda: request({"op":"status"})['visible']==6)
                def latest():
                    for line in reversed((work/'layout.jsonl').read_text().splitlines()):
                        try: f=json.loads(line)
                        except ValueError: continue
                        if len(f['rows'])==6 and all(r['extent']==1 for r in f['rows']): return f
                frame=wait_for(latest)
                # All six cards must be reachable, even on a short recording output.
                protocol=ROOT.parent/'runtjik/tests/virtual_pointer.xml'
                subprocess.run(['wayland-scanner','client-header',str(protocol),str(work/'pointer.h')],check=True)
                subprocess.run(['wayland-scanner','private-code',str(protocol),str(work/'protocol.c')],check=True)
                flags=subprocess.check_output(['pkg-config','--cflags','--libs','wayland-client'],text=True).split()
                subprocess.run(['cc',str(ROOT.parent/'runtjik/tests/pointer.c'),str(work/'protocol.c'),'-I'+str(work),'-o',str(work/'pointer'),*flags],check=True)
                subprocess.run([str(work/'pointer'),'NOTJIK'],input=f"m {int((frame['x']+180)*10)} {int((frame['y']+80)*10000/680)}\ns 1000\n",text=True,check=True,capture_output=True)
                wait_for(lambda: latest() and latest().get('scroll',0)>0)
                time.sleep(.2)
                subprocess.run(['grim','-o','NOTJIK',str(work/'mix.png')],check=True,capture_output=True)
                cli("demo","clear")
                assert [n['id'] for n in request({"op":"list"})['notices']]==[sentinel]
            finally:
                cli("compare","stop")
            # Run every directed scene, including actual GTK controls for synthetic
            # replies/quotes/recording. Produce an export to verify the recorder too.
            output=work/'showcase.mp4'
            cli("demo","record",str(output),"--monitor","NOTJIK","--speed","1",timeout=180)
            probe=json.loads(subprocess.check_output(['ffprobe','-v','quiet','-show_streams','-of','json',str(output)],text=True))
            assert probe['streams'][0]['width']==1000 and probe['streams'][0]['height']==680
            assert float(probe['streams'][0]['duration'])>5
            scenes=json.loads(output.with_suffix('.chapters.json').read_text())['scenes']
            assert len(scenes)==14
            for name,at in [('intro',1.5),('showcase',next(s['start'] for s in scenes if s['scene']=='error')+2.2)]:
                subprocess.run(['ffmpeg','-loglevel','error','-ss',str(at),'-i',str(output),'-frames:v','1',str(work/f'{name}.png')],check=True)
            print(f'GTK demos: all named fixtures, six-card mix, 14 directed scenes and MP4 export passed; no QML installation. Artifacts: {work}')
    finally:
        hypr("eval", f'if _G.{rule} then _G.{rule}:set_enabled(false); _G.{rule}=nil end')


if __name__ == '__main__':
    main()
