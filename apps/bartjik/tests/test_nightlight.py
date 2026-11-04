"""Native settings and scheduler against a synthetic Hyprsunset socket only."""

import datetime
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import time
import unittest

APP = Path(__file__).resolve().parents[1]
BIN = APP / "target/release/bartjik-core"


class NightlightTests(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory(prefix="bartjik-nightlight-")
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name)
        for name in ("config/hypr", "state/nightlight", "bin", "hypr/fixture"):
            (self.root / name).mkdir(parents=True)
        self.env = {
            **os.environ,
            "BARTJIK_HOME": str(APP),
            "XDG_RUNTIME_DIR": str(self.root),
            "XDG_CONFIG_HOME": str(self.root / "config"),
            "XDG_CACHE_HOME": str(self.root / "cache"),
            "XDG_STATE_HOME": str(self.root / "state"),
            "HYPRLAND_INSTANCE_SIGNATURE": "fixture",
            "DBUS_SYSTEM_BUS_ADDRESS": "unix:path=" + str(self.root / "absent-bus"),
            "PATH": str(self.root / "bin") + os.pathsep + os.environ["PATH"],
            "TZ": "UTC",
        }
        self.settings = self.root / "config/bartjik/nightlight.json"
        self.preference = self.root / "state/nightlight/state"
        self.preference.write_text("off\n")
        self.native_config = self.root / "config/hypr/hyprsunset.conf"
        self.native_config.write_text("profile {\n time = 7:30\n identity = true\n}\n"
                                      "profile {\n time = 21:00\n temperature = 5100\n gamma = 0.75\n}\n")

    def call(self, *args, success=True):
        result = subprocess.run([str(BIN), "helper", "nightlight", *args], env=self.env,
                                capture_output=True, text=True, timeout=15)
        if success:
            self.assertEqual(result.returncode, 0, result.stderr)
            return json.loads(result.stdout) if result.stdout else None
        self.assertNotEqual(result.returncode, 0)
        return json.loads(result.stderr)["error"]

    def action(self, name, value, **kwargs):
        return self.call("action", name, value, **kwargs)

    def wait(self, predicate):
        deadline = time.monotonic() + 6
        while time.monotonic() < deadline:
            try:
                if predicate():
                    return
            except (ConnectionError, FileNotFoundError):
                pass
            time.sleep(.05)
        self.fail("Night-light fixture did not reach expected state")

    def test_import_atomic_updates_validation_and_disabled_preference(self):
        original = self.native_config.read_bytes()
        row = self.call("status")
        self.assertEqual(row["panel"]["settings"]["temperature"], 5100)
        self.assertEqual(row["panel"]["settings"]["gamma"], 75)
        self.assertEqual(row["panel"]["settings"]["end"], "07:30")
        self.assertFalse(row["panel"]["enabled"])
        self.action("temperature", "4300")
        before = self.settings.read_bytes()
        self.assertIn("Temperature", self.action("temperature", "999", success=False))
        self.assertEqual(before, self.settings.read_bytes())
        self.assertIn("different", self.action("hours", '{"start":"22:00","end":"22:00"}', success=False))
        self.assertEqual(before, self.settings.read_bytes())
        self.action("hours", '{"start":"22:30","end":"6:15"}')
        saved = self.call("status")["panel"]["settings"]
        self.assertEqual((saved["start"], saved["end"]), ("22:30", "06:15"))
        self.assertEqual(self.settings.stat().st_mode & 0o777, 0o600)
        self.assertEqual(self.native_config.read_bytes(), original)
        self.assertEqual(self.preference.read_text(), "off\n")
        self.assertIn("disabled", self.call("check", success=False))
        self.assertIn("disabled", self.call("run", success=False))

    def test_location_failure_retains_fixed_schedule_and_city_fallback(self):
        self.action("gamma", "90")
        before = self.settings.read_bytes()
        self.assertIn("search for your city", self.action("mode", "solar", success=False))
        self.assertEqual(before, self.settings.read_bytes())
        cities = self.root / "bartjik-nightlight-cities.json"
        cities.write_text(json.dumps([dict(id=1, name="Synthetic city", latitude=22.3,
                                          longitude=114.2, timezone="Asia/Hong_Kong", source="city")]))
        self.assertIn("Search again", self.action("city-select", "2", success=False))
        self.action("city-select", "1")
        self.action("mode", "solar")
        row = self.call("status")["panel"]
        self.assertEqual(row["settings"]["location"]["name"], "Synthetic city")
        self.assertEqual(row["schedule"]["timezone"], "Asia/Hong_Kong")
        self.assertEqual(row["cities"], [])
        self.assertFalse(row["enabled"])
        self.assertIn("search for your city", self.action("locate", "", success=False))
        self.assertEqual(self.call("status")["panel"]["settings"]["mode"], "solar")
        self.action("mode", "fixed")
        self.assertEqual(self.call("status")["panel"]["schedule"]["timezone"], "UTC")

    def test_concurrent_setting_changes_do_not_overwrite_each_other(self):
        processes = [subprocess.Popen([str(BIN), "helper", "nightlight", "action", key, value],
                                      env=self.env, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
                     for key, value in [("temperature", "4200"), ("gamma", "90")]]
        for process in processes:
            _, error = process.communicate(timeout=8)
            self.assertEqual(process.returncode, 0, error)
        saved = json.loads(self.settings.read_text())
        self.assertEqual((saved["temperature"], saved["gamma"]), (4200, 90))

    def test_device_city_label_is_offline_without_changing_fix_or_schedule(self):
        self.action("temperature", "4300")
        stored = json.loads(self.settings.read_text())
        location = dict(name="Device location", latitude=51.50721, longitude=-0.12758,
                        source="device", timezone="Europe/London", updatedAt=1793764800)
        stored.update(mode="solar", location=location)
        self.settings.write_text(json.dumps(stored))
        original = self.settings.read_bytes()
        # No network service is available, even though the device city resolves.
        self.env.update(HTTPS_PROXY="http://127.0.0.1:9", HTTP_PROXY="http://127.0.0.1:9", ALL_PROXY="http://127.0.0.1:9")
        panel = self.call("status")["panel"]
        self.assertEqual(panel["settings"]["location"], {**location, "name": "Near London, GB"})
        self.assertEqual(panel["settings"]["mode"], "solar")
        self.assertEqual(self.settings.read_bytes(), original)
        self.assertFalse(panel["enabled"])
        stored["location"].update(latitude=0., longitude=0.)
        self.settings.write_text(json.dumps(stored))
        panel = self.call("status")["panel"]
        self.assertEqual(panel["settings"]["location"]["name"], "0.00° N, 0.00° E")
        self.assertEqual(panel["settings"]["location"]["updatedAt"], location["updatedAt"])
        # A manually selected city keeps its chosen name.
        stored["location"].update(source="city", name="Selected city")
        self.settings.write_text(json.dumps(stored))
        self.assertEqual(self.call("status")["panel"]["settings"]["location"]["name"], "Selected city")

    def test_supervisor_isolates_profiles_applies_changes_and_respects_idle_dimming(self):
        script = self.root / "bin/hyprsunset"
        script.write_text(f"#!{sys.executable}\n" + r'''
import json, os, socket, sys
from pathlib import Path
root = Path(os.environ['XDG_RUNTIME_DIR'])
(root / 'startup.json').write_text(json.dumps(dict(args=sys.argv[1:], config=str(Path(os.environ['XDG_CONFIG_HOME']) / 'hypr/hyprsunset.conf'))))
server = socket.socket(socket.AF_UNIX)
server.bind(str(root / 'hypr/fixture/.hyprsunset.sock'))
server.listen()
identity, gamma = True, '100'
while True:
    connection, _ = server.accept()
    with connection:
        request = connection.recv(4096).decode()
        with (root / 'requests.jsonl').open('a') as out:
            out.write(json.dumps(request) + '\n')
        reply = 'ok'
        if request == 'identity get': reply = str(identity).lower()
        elif request == 'identity': identity = True
        elif request.startswith('temperature '): identity = False
        elif request.startswith('gamma '): gamma = request.split()[1]
        elif request == 'gamma': reply = gamma
        connection.sendall(reply.encode())
''')
        script.chmod(0o755)
        self.preference.write_text("on\n")
        now = datetime.datetime.now(datetime.timezone.utc)

        def hours(start, end):
            return json.dumps(dict(start=(now + datetime.timedelta(hours=start)).strftime("%H:%M"),
                                   end=(now + datetime.timedelta(hours=end)).strftime("%H:%M")))

        self.action("hours", hours(2, 4))
        process = subprocess.Popen([str(BIN), "helper", "nightlight", "run"], env=self.env,
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        try:
            self.wait(lambda: self.call("status")["panel"]["running"])
            startup = json.loads((self.root / "startup.json").read_text())
            self.assertIn("--identity", startup["args"])
            self.assertNotEqual(startup["config"], str(self.native_config))
            self.assertNotIn("profile", Path(startup["config"]).read_text())
            panel = self.call("status")["panel"]
            self.assertTrue(panel["enabled"])
            self.assertFalse(panel["active"])
            self.action("temperature", "4300")
            self.action("hours", hours(-2, 2))
            self.wait(lambda: self.call("status")["panel"]["active"])
            requests = (self.root / "requests.jsonl").read_text()
            self.assertIn('"temperature 4300"', requests)
            self.assertIn('"gamma 75"', requests)

            def query(message):
                with socket.socket(socket.AF_UNIX) as peer:
                    peer.settimeout(2)
                    peer.connect(str(self.root / "hypr/fixture/.hyprsunset.sock"))
                    peer.sendall(message.encode())
                    peer.shutdown(socket.SHUT_WR)
                    return peer.recv(1024).decode()

            query("gamma 25")
            time.sleep(2.4)
            self.assertEqual(query("gamma"), "25")
            self.action("gamma", "90")
            self.wait(lambda: query("gamma") == "90")
            query("gamma 100")
            self.wait(lambda: query("gamma") == "90")
            self.action("hours", hours(2, 4))
            self.wait(lambda: query("identity get") == "true")
            self.assertEqual(query("gamma"), "100")
        finally:
            process.terminate()
            process.communicate(timeout=5)
        self.assertEqual(process.returncode, 0)


if __name__ == "__main__":
    unittest.main()
