"""Executable native client tests. All data and subprocess peers are synthetic."""

import datetime
import json
import os
from pathlib import Path
import subprocess
import tempfile
import tomllib
import unittest

APP = Path(__file__).resolve().parents[1]


class NativeClientTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix="tasktjik-native-cli-")
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.env = {
            **os.environ,
            "XDG_CONFIG_HOME": str(self.root / "config"),
            "XDG_STATE_HOME": str(self.root / "state"),
        }
        self.config = self.root / "config/tasktjik/tasktjik.conf"
        self.config.parent.mkdir(parents=True)
        self.config.write_text(
            '[tasktjik]\nauthority=""\ndirectory='
            + json.dumps(str(self.root / "tasks"))
            + "\n"
        )
        self.bin = self.root / "bin"
        self.bin.mkdir()
        self.env["PATH"] = str(self.bin) + ":" + os.environ["PATH"]

    def call(self, *args, body=None, check=True):
        result = subprocess.run(
            [str(APP / "tasktjik-api"), *args],
            input=json.dumps(body) if body is not None else None,
            text=True,
            capture_output=True,
            env=self.env,
            timeout=15,
        )
        if check:
            self.assertEqual(result.returncode, 0, result.stderr)
            return json.loads(result.stdout)
        return result

    def script(self, name, text):
        path = self.bin / name
        path.write_text("#!/usr/bin/python3\n" + text)
        path.chmod(0o700)

    def test_local_cli_and_native_ssh_serve(self):
        task = self.call(
            "request", body={"action": "create", "fields": {"title": "Synthetic task"}}
        )
        self.assertEqual(self.call("state", body={})["tasks"][0]["id"], task["id"])
        self.call("done", task["id"], "once")
        self.assertTrue(self.call("state", body={})["tasks"][0]["done"])
        result = self.call(
            "serve", body={"directory": str(self.root / "tasks"), "action": "list"}
        )
        self.assertEqual(result["result"][0]["id"], task["id"])
        self.assertEqual(self.call("state", "--demo", body={})["tasks"], [])

    def test_native_ssh_transport(self):
        log = self.root / "ssh.json"
        self.script(
            "ssh",
            "import sys,json,pathlib\npathlib.Path("
            + repr(str(log))
            + ').write_text(json.dumps(sys.argv[1:]))\nrequest=json.load(sys.stdin)\nprint(json.dumps({"result":[]}))\n',
        )
        self.config.write_text('[tasktjik]\nauthority="synthetic"\n')
        self.assertEqual(self.call("state", body={})["tasks"], [])
        args = json.loads(log.read_text())
        self.assertEqual(args[-1], ".local/lib/tasktjik/tasktjik-api serve")
        self.assertFalse(any("python" in arg for arg in args))
        self.assertIn("-oStrictHostKeyChecking=yes", args)

    def test_enrollment_preserves_configuration_and_registers_device(self):
        self.config.write_text(
            '# preserved\n[tasktjik]\nvault="Synthetic vault"\nsocket="/old/socket"\n'
        )
        log = self.root / "requests.jsonl"
        self.script(
            "curl",
            "import sys,json,pathlib\nargs=sys.argv[1:]\nbody=sys.stdin.read()\nwith pathlib.Path("
            + repr(str(log))
            + ').open("a") as f:f.write(json.dumps({"args":args,"body":body})+"\\n")\nresult={"login":"synthetic@example.test"} if args[-1].endswith("/identity") else {}\nsys.stdout.write(json.dumps(result)+"\\n200")\n',
        )
        result = self.call(
            "connect", "https://si.synthetic.ts.net/tasktjik", "--notifications"
        )
        self.assertTrue(result["device"]["notifications"])
        content = self.config.read_text()
        self.assertIn("# preserved", content)
        self.assertIn("Synthetic vault", content)
        self.assertEqual(tomllib.loads(content)["tasktjik"]["socket"], "")
        self.assertEqual(self.config.stat().st_mode & 0o777, 0o600)
        self.assertEqual(len(list(self.config.parent.glob("*.pre-api-*"))), 1)
        requests = [json.loads(s) for s in log.read_text().splitlines()]
        self.assertEqual(len(requests), 2)
        self.assertEqual(requests[0]["args"][0], "--disable")
        self.assertEqual(json.loads(requests[1]["body"])["id"], result["device"]["id"])

    def test_notification_delivery_ack_and_failed_delivery_release(self):
        task = self.call(
            "request",
            body={
                "action": "create",
                "fields": {
                    "title": "Synthetic reminder",
                    "due": datetime.date.today().isoformat(),
                },
            },
        )
        payload = self.root / "notice.json"
        argv = self.root / "notice-argv.json"
        self.script(
            "quickshell",
            "import sys,json,pathlib\npath=pathlib.Path(sys.argv[-1])\nassert path.stat().st_mode & 0o777 == 0o600\npathlib.Path("
            + repr(str(payload))
            + ").write_text(path.read_text())\npathlib.Path("
            + repr(str(argv))
            + ').write_text(json.dumps(sys.argv))\nprint(json.dumps({"ok":False,"error":"Synthetic delivery failure"}))\n',
        )
        failed = self.call("notify", check=False)
        self.assertNotEqual(failed.returncode, 0)
        self.assertEqual(
            self.call("request", body={"action": "list"})[0]["notifications"], {}
        )
        self.assertNotIn("Synthetic reminder", argv.read_text())
        script = self.bin / "quickshell"
        script.write_text(script.read_text().replace('"ok":False', '"ok":True'))
        self.assertEqual(self.call("notify")["delivered"], 1)
        self.assertEqual(self.call("notify")["delivered"], 0)
        self.assertEqual(
            json.loads(payload.read_text())["notice"]["data"]["taskId"], task["id"]
        )
        reservations = self.call("request", body={"action": "list"})[0]["notifications"]
        self.assertEqual(next(iter(reservations.values()))["state"], "delivered")


if __name__ == "__main__":
    unittest.main(verbosity=2)
