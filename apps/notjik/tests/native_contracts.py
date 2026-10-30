"""Native executable acceptance with synthetic state and no desktop/account access.
Python is a development test driver only, never installed with Notjik.
"""
import json
import os
from pathlib import Path
import select
import sqlite3
import subprocess
import tempfile
import time
import unittest

ROOT = Path(__file__).resolve().parents[1]
BINARY = Path(os.environ.get("NOTJIK_TEST_BINARY", ROOT / "dist/bin/notjik-core"))

class NativeContracts(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="notjik-native-")
        self.addCleanup(self.temp.cleanup)
        self.path = Path(self.temp.name)
        self.env = {**os.environ, "NOTJIK_HOME": str(ROOT), "NOTJIK_CONFIG": str(self.path / "config.json"),
                    "NOTJIK_STATE_DIR": str(self.path / "state"), "NOTJIK_TELEGRAM_DIR": str(self.path / "telegram"),
                    "XDG_RUNTIME_DIR": str(self.path), "WAYLAND_DISPLAY": "absent"}

    def run_cli(self, *args, check=True, **kwargs):
        return subprocess.run([str(BINARY), *args], env=self.env, capture_output=True, text=True, check=check, **kwargs)

    def config(self, value):
        (self.path / "config.json").write_text(json.dumps(value))
        return self.run_cli("config", check=False)

    def test_config_rejects_invalid_types_and_bounds(self):
        values = [[], {"placement":{"edge":1}}, {"placement":{"position":False}},
                  {"placement":{"monitors":[0]}}, {"placement":{"offsetX":"101%"}},
                  {"maxVisible":True}, {"maxVisible":2.5}, {"maxVisible":10,"maxQueued":9},
                  {"history":{"maxEvents":0,"maxAgeDays":0}}, {"history":{"maxAgeDays":True}},
                  {"style":{"text":"red"}}, {"style":{"width":2400}},
                  {"actions":{"nativeHooks":[{"caller":"a","identifier":"b","command":["x\u0000"]}]}},
                  {"telegram":{"dismissIsRead":["9007199254740992"]}},
                  {"telegram":{"dismissReadMode":"unsafe"}}, {"components":{"bad":"remote.qml"}}]
        for value in values:
            with self.subTest(value=value):
                result = self.config(value)
                self.assertNotEqual(result.returncode, 0, result.stdout)
                self.assertNotIn("panicked", result.stderr)

    def test_defaults_theme_components_and_read_only_config(self):
        component = self.path / "Task.qml"
        component.write_text("import QtQuick\nItem {}\n")
        data = {"theme":"rose-punk","style":{"width":500},"history":{"maxEvents":0,"maxAgeDays":7,"drawerLimit":0},
                "placement":{"offsetX":"-20%","monitors":["DP-1",2]},"components":{"task":"Task.qml"}}
        result = self.config(data)
        self.assertEqual(result.returncode, 0, result.stderr)
        effective = json.loads(result.stdout)
        self.assertEqual(effective["style"]["accent"], "#ff53a6")
        self.assertEqual(effective["style"]["width"], 500)
        self.assertEqual(effective["components"]["task"], component.as_uri())
        self.assertEqual((self.path / "config.json").read_text(), json.dumps(data))
        self.assertNotEqual(self.run_cli("init", check=False).returncode, 0)

    def test_handler_contract(self):
        rule = {"id":"invoice","match":{"app":"billing"},"pattern":"Invoice (.+): ([0-9]+)",
                "card":"key-value","captures":{"heading":1,"total":2},"fields":[{"label":"Total","key":"total"}]}
        self.assertEqual(self.config({"handlers":[rule]}).returncode, 0)
        for mutation in [{"captures":{"heading":4}}, {"match":{"title":"anything"}},
                         {"captures":{"constructor":1}}, {"fields":[]}, {"pattern":"("}]:
            self.assertNotEqual(self.config({"handlers":[{**rule,**mutation}]}).returncode, 0)

    def test_worker_retention_preferences_lock_and_nonexecutable_history(self):
        self.config({"history":{"maxEvents":2}})
        process = subprocess.Popen([str(BINARY), "worker"], env=self.env, stdin=subprocess.PIPE,
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        self.addCleanup(lambda: process.kill() if process.poll() is None else None)
        self.addCleanup(process.stdout.close)
        self.addCleanup(process.stderr.close)
        buffer = b""
        received = []
        def until(predicate, seconds=5):
            nonlocal buffer
            deadline = time.monotonic()+seconds
            while time.monotonic()<deadline:
                if predicate(received): return
                if not select.select([process.stdout],[],[],.1)[0]: continue
                block = os.read(process.stdout.fileno(), 65536)
                self.assertTrue(block, "worker closed stdout unexpectedly")
                buffer += block
                while b"\n" in buffer:
                    raw,buffer=buffer.split(b"\n",1);received.append(json.loads(raw))
            self.fail(f"worker condition not met: {received}")
        until(lambda rows:any("history" in r for r in rows))
        self.assertEqual(received[0], {"paused":False})
        self.assertIn({"lockState":"unknown"}, received)
        for i in range(3):
            event={"event":"received","session":"synthetic","notice":{"id":f"n-{i}","app":"fixture","title":"test",
                   "received":i,"image":"image:private-handle","actions":[{"label":"Open","command":["must-not-persist"]}]}}
            process.stdin.write(json.dumps(event).encode()+b"\n")
        process.stdin.write(b'{"op":"pausePreference","paused":true}\n');process.stdin.flush()
        until(lambda rows:any(len(r.get("history",[]))==2 for r in rows))
        process.stdin.close();process.wait(timeout=3)
        self.assertEqual(process.returncode,0,process.stderr.read().decode())
        db_path=self.path/"state/history.sqlite3"
        self.assertEqual(db_path.stat().st_mode&0o777,0o600)
        with sqlite3.connect(db_path) as db:
            payloads=db.execute("select payload from events").fetchall()
            self.assertEqual(len(payloads),2)
            self.assertNotIn("must-not-persist",str(payloads))
        latest=next(r["history"] for r in reversed(received) if len(r.get("history",[]))==2)
        self.assertTrue(all(n["actions"]==[] and n["image"]=="" for n in latest))
        self.assertEqual(json.loads((self.path/"state/preferences.json").read_text()),{"paused":True})
        result=self.run_cli("worker",input="")
        self.assertEqual(json.loads(result.stdout.splitlines()[0]),{"paused":True})

    def test_ipc_arguments_never_contain_notification_payload(self):
        bindir=self.path/"bin";bindir.mkdir()
        fake=bindir/"quickshell"
        fake.write_text('#!/bin/sh\nprintf "%s\\n" "$@" > "$XDG_RUNTIME_DIR/args"\nfor file do :; done\nstat -c %a "$file" > "$XDG_RUNTIME_DIR/mode"\ncat "$file" > "$XDG_RUNTIME_DIR/payload"\nprintf \'{"ok":true,"id":"synthetic"}\\n\'\n')
        fake.chmod(0o755)
        self.env["PATH"]=str(bindir)+":"+self.env["PATH"]
        result=self.run_cli("send","PRIVATE-TITLE","PRIVATE-BODY","--timeout","2m")
        self.assertEqual(result.stdout.strip(),"synthetic")
        argv=(self.path/"args").read_text()
        self.assertNotIn("PRIVATE",argv)
        self.assertEqual((self.path/"mode").read_text().strip(),"600")
        self.assertEqual(json.loads((self.path/"payload").read_text())["notice"]["timeout"],120000)
        self.assertFalse(Path(argv.splitlines()[-1]).exists())

    def test_native_sticker_conversion_and_external_asset_rejection(self):
        import gzip
        library = list((Path.home()/".local/share/notjik/telegram-venv/lib").glob("python*/site-packages/rlottie_python/librlottie.so.0.2"))
        if library: self.env["NOTJIK_RLOTTIE_LIBRARY"] = str(library[0])
        for filename,kind,signature in [("demo-sticker.webp","webp",b"\x89PNG"),("demo-sticker.tgs","tgs",b"GIF"),("demo-video-sticker.webm","webm",b"GIF")]:
            output=self.path/kind
            self.run_cli("media-render",str(ROOT/"examples"/filename),str(output),kind,timeout=40)
            self.assertTrue(output.read_bytes().startswith(signature))
            self.assertEqual(output.stat().st_mode&0o777,0o600)
        with gzip.open(ROOT/"examples/demo-sticker.tgs","rb") as stream: data=json.load(stream)
        data["assets"]=[{"u":"/etc/","p":"passwd"}]
        source=self.path/"external.tgs"
        with gzip.open(source,"wb") as stream: stream.write(json.dumps(data).encode())
        output=self.path/"external.gif"
        result=self.run_cli("media-render",str(source),str(output),"tgs",check=False)
        self.assertNotEqual(result.returncode,0)
        self.assertFalse(output.exists())

    def test_synthetic_recording_stop_review_and_cleanup(self):
        process=subprocess.Popen([str(BINARY),"record","--synthetic"],env=self.env,
                                 stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
        try:
            self.assertEqual(json.loads(process.stdout.readline())["state"],"recording")
            time.sleep(.8)
            process.stdin.write(b'{"action":"stop"}\n');process.stdin.flush()
            ready=None; deadline=time.monotonic()+12
            while time.monotonic()<deadline:
                if not select.select([process.stdout],[],[],.2)[0]:continue
                raw=process.stdout.readline()
                self.assertTrue(raw)
                event=json.loads(raw)
                self.assertNotEqual(event.get("state"),"error",event)
                if event.get("state")=="ready":ready=event;break
            self.assertIsNotNone(ready)
            draft=self.path/"notjik-voice"/(ready["token"]+".ogg")
            self.assertEqual(draft.stat().st_mode&0o777,0o600)
            process.stdin.close();process.wait(timeout=5)
            self.assertEqual(process.returncode,0)
            self.assertFalse(draft.exists())
        finally:
            if process.poll() is None:process.kill();process.wait()
            for stream in (process.stdin,process.stdout,process.stderr):stream.close()

    def test_showcase_marker_blocks_real_account_and_microphone_helpers(self):
        (self.path/".synthetic-only").write_text("synthetic test")
        self.env["NOTJIK_HOME"]=str(self.path)
        for args in [("telegram","doctor"),("record",)]:
            result=self.run_cli(*args,check=False,timeout=3)
            self.assertNotEqual(result.returncode,0)
            self.assertIn("disabled",result.stderr)
        self.assertFalse((self.path/"notjik-voice").exists())

    def test_demo_commands_are_native_and_plan_is_read_only(self):
        self.assertIn("telegram-voice",self.run_cli("demo","list").stdout)
        self.assertIn("Text replies",self.run_cli("demo","cycle","--plan").stdout)
        self.assertIn("markup",self.run_cli("compare","cases").stdout)
        self.assertFalse((self.path/"state").exists())

if __name__ == "__main__": unittest.main()
