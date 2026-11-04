"""Native idle policy and power commands. Every session command is mocked."""

import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest

APP = Path(__file__).resolve().parents[1]
BIN = APP / "target/release/bartjik-core"
NORMAL = """# Preserve this native policy.
general {
    lock_cmd = pidof hyprlock || ~/.local/bin/tailscale-lock
    before_sleep_cmd = loginctl lock-session
    after_sleep_cmd = hyprctl dispatch dpms on
}
listener {
    timeout = 60 # original dim delay
    on-timeout = hyprctl hyprsunset gamma 10
    on-resume = hyprctl hyprsunset gamma 100
}
listener {
    timeout = 30
    on-timeout = brightnessctl -sd rgb:kbd_backlight set 0
    on-resume = brightnessctl -rd rgb:kbd_backlight
}
listener {
    timeout = 180
    on-timeout = loginctl lock-session
}
listener {
    timeout = 600
    on-timeout = hyprctl dispatch dpms off && sleep 0.2 && hyprctl dispatch dpms off
    on-resume = hyprctl dispatch dpms on && hyprctl hyprsunset gamma 100
}
listener {
    timeout = 1800
    on-timeout = systemctl suspend
}
"""
GAME = """general {
    lock_cmd = pidof hyprlock || ~/.local/bin/tailscale-lock
    before_sleep_cmd = loginctl lock-session
    after_sleep_cmd = hyprctl dispatch dpms on
}
listener {
    timeout = 15
    on-timeout = hyprctl dispatch dpms off
    on-resume = hyprctl dispatch dpms on
}
"""


class IdleTests(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory(prefix="bartjik-idle-test-")
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name)
        for name in ("bin", "config/hypr", "home/.local/state/idle-inhibit", "runtime"):
            (self.root / name).mkdir(parents=True)
        self.env = {**os.environ, "HOME": str(self.root / "home"),
                    "XDG_CONFIG_HOME": str(self.root / "config"),
                    "XDG_STATE_HOME": str(self.root / "state"),
                    "XDG_RUNTIME_DIR": str(self.root / "runtime"),
                    "IDLE_FIXTURE": str(self.root),
                    "PATH": str(self.root / "bin") + os.pathsep + os.environ["PATH"]}
        self.preference = self.root / "home/.local/state/idle-inhibit/state"
        self.preference.write_text("on\n")
        self.normal = self.root / "config/hypr/hypridle.conf"
        self.normal.write_text(NORMAL)
        self.game = self.root / "config/hypr/hypridle_game.conf"
        self.game.write_text(GAME)
        self.saved = self.root / "config/bartjik/idle.json"
        self.lease = self.root / "state/bartjik/idle-timer.json"
        self.unit = self.root / "unit"
        self.unit.write_text(f"# /usr/lib/systemd/user/hypridle.service\n[Service]\nExecStart={self.root}/bin/hypridle\n")
        self.script("systemctl", r'''
import json, os, sys
from pathlib import Path
root=Path(os.environ['IDLE_FIXTURE']);args=sys.argv[1:]
with (root/'events').open('a') as f:f.write(json.dumps(['systemctl',*args])+'\n')
if args==['--user','cat','hypridle.service']:
    print((root/'unit').read_text());sys.exit(0)
if args==['--user','start','bartjik-idle.service']:
    sys.exit(1 if (root/'reject-timer').exists() else 0)
if args[:1]==['--user'] and args[-1]=='hypridle.service':
    if args[1]=='stop':(root/'running').unlink(missing_ok=True)
    if args[1] in ('start','restart'):
        if (root/'reject-start').exists():sys.exit(1)
        (root/'running').touch()
    sys.exit(0)
assert args in [['suspend'],['poweroff']],args
''')
        self.script("pgrep", r'''
import os,sys
from pathlib import Path
assert sys.argv[-2:]==['-x','hypridle']
sys.exit(0 if (Path(os.environ['IDLE_FIXTURE'])/'running').exists() else 1)
''')
        self.script("pkill", r'''
import os,sys
from pathlib import Path
assert sys.argv[-2:]==['-x','hypridle']
(Path(os.environ['IDLE_FIXTURE'])/'running').unlink(missing_ok=True)
''')
        self.script("loginctl", r'''
import json,os,sys
from pathlib import Path
root=Path(os.environ['IDLE_FIXTURE'])
assert sys.argv[1:]==['lock-session']
with (root/'events').open('a') as f:f.write(json.dumps(['loginctl',*sys.argv[1:]])+'\n')
sys.exit(1 if (root/'reject-lock').exists() else 0)
''')
        self.script("hypridle", r'''
import json,sys
from pathlib import Path
assert sys.argv[1]=='--config'
print(json.dumps({'config':sys.argv[2],'text':Path(sys.argv[2]).read_text()}))
''')

    def script(self, name, source):
        p = self.root / "bin" / name
        p.write_text(f"#!{sys.executable}\n" + source)
        p.chmod(0o755)

    def call(self, *args, success=True):
        r = subprocess.run([str(BIN), "helper", "idle", *args], env=self.env,
                           capture_output=True, text=True, timeout=15)
        if success:
            self.assertEqual(r.returncode, 0, r.stderr)
            return json.loads(r.stdout) if r.stdout else None
        self.assertNotEqual(r.returncode, 0)
        return json.loads(r.stderr)["error"]

    def action(self, name, value="", **kwargs):
        return self.call("action", name, value, **kwargs)

    def status(self):
        return self.call("status")["panel"]

    def events(self):
        return [json.loads(l) for l in (self.root / "events").read_text().splitlines()]

    def apply(self, **changes):
        panel = self.status()
        return self.action("delays", json.dumps(dict(revision=panel["revision"],
                                                    delays={**panel["delays"], **changes})))

    def expire(self):
        lease = json.loads(self.lease.read_text())
        lease["deadline"] = time.time() - 1
        self.lease.write_text(json.dumps(lease))

    def test_reads_effective_game_override_and_ignores_own_wrapper(self):
        with self.unit.open("a") as f:
            f.write(f"# /config/hypridle.service.d/override.conf\n[Service]\nExecStart=\nExecStart={self.root}/bin/hypridle -c %E/hypr/hypridle_game.conf\n")
            f.write("# /config/hypridle.service.d/zz-bartjik-idle.conf\n[Service]\nExecStart=\nExecStart=wrapper helper idle run\n")
        panel = self.status()
        self.assertTrue(panel["gamePolicy"])
        self.assertEqual(panel["delays"], dict(dim=0, lock=0, screenOff=15, suspend=0))
        self.apply(lock=300, screenOff=0)
        self.preference.write_text("off\n")
        result = self.call("run")
        self.assertIn("timeout = 300", result["text"])
        self.assertIn("on-timeout = loginctl lock-session", result["text"])
        self.assertNotIn("on-timeout = hyprctl dispatch dpms off", result["text"])
        self.assertEqual(self.game.read_text(), GAME)

    def test_updates_preserve_hooks_keyboard_native_config_and_inhibition(self):
        panel = self.status()
        self.assertEqual(panel["delays"], dict(dim=60, lock=180, screenOff=600, suspend=1800))
        self.apply(dim=120, lock=0)
        self.assertEqual(self.preference.read_text(), "on\n")
        self.assertEqual(self.normal.read_text(), NORMAL)
        self.assertNotIn(["systemctl", "--user", "restart", "hypridle.service"], self.events())
        self.assertEqual(self.saved.stat().st_mode & 0o777, 0o600)
        self.assertIn("Keep awake", self.call("run", success=False))
        self.preference.write_text("off\n")
        rendered = self.call("run")["text"]
        self.assertIn("timeout = 120 # original dim delay", rendered)
        self.assertIn("timeout = 30\n    on-timeout = brightnessctl", rendered)
        self.assertIn("before_sleep_cmd = loginctl lock-session", rendered)
        self.assertNotIn("on-timeout = loginctl lock-session", rendered)
        self.assertIn("sleep 0.2", rendered)

    def test_no_overrides_executes_original_policy_unchanged(self):
        self.preference.write_text("off\n")
        result = self.call("run")
        self.assertEqual(result, dict(config=str(self.normal), text=NORMAL))
        self.assertFalse(self.saved.exists())

    def test_validates_delays_and_rejects_stale_policy(self):
        before = self.status()
        for value in (-1, 86401, True, "60"):
            message = self.action("delays", json.dumps(dict(revision=before["revision"],
                                  delays={**before["delays"], "dim": value})), success=False)
            self.assertIn("Idle delays", message)
        self.normal.write_text(NORMAL + "# An external edit\n")
        message = self.action("delays", json.dumps(dict(revision=before["revision"], delays=before["delays"])), success=False)
        self.assertIn("changed", message)
        self.assertFalse(self.saved.exists())

    def test_timer_failure_does_not_change_preference_or_settings(self):
        (self.root / "reject-timer").touch()
        self.assertIn("unavailable", self.action("duration", "1800", success=False))
        self.assertFalse(self.lease.exists())
        self.assertFalse(self.saved.exists())
        self.assertEqual(self.preference.read_text(), "on\n")

    def test_timer_expires_after_initiating_process_exits(self):
        self.action("duration", "1800")
        self.assertGreater(self.status()["deadline"], time.time() + 1790)
        self.expire()
        worker = subprocess.Popen([str(BIN), "helper", "idle", "watch"], env=self.env,
                                  stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        try:
            deadline = time.monotonic() + 6
            while self.lease.exists() and time.monotonic() < deadline:
                time.sleep(.05)
            self.assertFalse(self.lease.exists())
            self.assertEqual(self.preference.read_text(), "off\n")
            self.assertTrue((self.root / "running").exists())
        finally:
            worker.terminate()
            try:
                worker.communicate(timeout=5)
            except subprocess.TimeoutExpired:
                worker.kill()
                worker.communicate(timeout=3)
                self.fail("Timer did not handle SIGTERM; run outside the process sandbox")

    def test_cancel_and_external_override_are_not_undone_by_expiry(self):
        self.action("duration", "3600")
        self.action("duration", "0")
        self.assertFalse(self.lease.exists())
        self.action("duration", "3600")
        self.expire()
        replacement = self.preference.with_suffix(".next")
        replacement.write_text("on\n")
        replacement.replace(self.preference)
        self.call("tick")
        self.assertFalse(self.lease.exists())
        self.assertEqual(self.preference.read_text(), "on\n")
        self.assertFalse((self.root / "running").exists())

    def test_expiry_retries_failure_without_losing_timer(self):
        self.action("duration", "1800")
        self.expire()
        (self.root / "reject-start").touch()
        self.call("tick", success=False)
        self.assertIn("Could not resume", self.status()["error"])
        self.assertEqual(self.preference.read_text(), "on\n")
        (self.root / "reject-start").unlink()
        self.call("tick")
        self.assertFalse(self.lease.exists())
        self.assertEqual(self.preference.read_text(), "off\n")

    def test_toggle_resumes_policy_and_reuses_selected_duration(self):
        self.action("duration", "7200")
        self.action("toggle")
        self.assertFalse(self.lease.exists())
        self.assertTrue(self.status()["running"])
        self.action("toggle")
        self.assertGreater(self.status()["deadline"], time.time() + 7190)
        self.assertFalse(self.status()["running"])

    def test_running_policy_applies_and_failed_restart_restores_saved_delays(self):
        self.preference.write_text("off\n")
        (self.root / "running").touch()
        self.apply(dim=120)
        self.assertIn(["systemctl", "--user", "restart", "hypridle.service"], self.events())
        previous = self.saved.read_bytes()
        panel = self.status()
        (self.root / "reject-start").touch()
        message = self.action("delays", json.dumps(dict(revision=panel["revision"],
                              delays={**panel["delays"], "dim": 300})), success=False)
        self.assertIn("previous settings restored", message)
        self.assertEqual(previous, self.saved.read_bytes())

    def test_duration_adopts_unsaved_manual_inhibition(self):
        self.preference.unlink()
        self.action("duration", "1800")
        self.assertEqual(self.preference.read_text(), "on\n")
        self.assertIsNotNone(self.status()["deadline"])

    def test_lock_suspend_and_confirmed_poweroff_only_call_mock_commands(self):
        self.assertIn("Confirm", self.action("poweroff", "", success=False))
        self.assertFalse((self.root / "events").exists())
        self.action("lock")
        self.action("suspend")
        self.action("poweroff", "confirm")
        self.assertEqual(self.events(), [["loginctl", "lock-session"], ["loginctl", "lock-session"],
                                        ["systemctl", "suspend"], ["systemctl", "poweroff"]])
        (self.root / "reject-lock").touch()
        self.action("suspend", success=False)
        self.assertEqual(self.events()[-1], ["loginctl", "lock-session"])


if __name__ == "__main__":
    unittest.main()
