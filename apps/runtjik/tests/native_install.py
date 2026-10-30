"""Check native-only installation and atomic replacement in an isolated home."""

import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

APP = Path(__file__).resolve().parents[1]


class NativeInstallTests(unittest.TestCase):
    def test_install_removes_python_payload_and_preserves_running_binary(self):
        with tempfile.TemporaryDirectory(prefix="runtjik-native-install-") as temp:
            home = Path(temp)
            env = {
                **os.environ,
                "HOME": str(home),
                "RUNTJIK_INSTALL_HOME": str(home),
                "XDG_CONFIG_HOME": str(home / ".config"),
                "XDG_STATE_HOME": str(home / ".local/state"),
                "XDG_CACHE_HOME": str(home / ".cache"),
                "XDG_DATA_HOME": str(home / ".local/share"),
            }
            (home / ".config").mkdir()
            (home / ".config/user-dirs.dirs").write_text(
                'XDG_PICTURES_DIR="/var/tmp/runtjik-fixture-pictures"\nXDG_VIDEOS_DIR="/var/tmp/runtjik-fixture-videos"\n'
            )
            env.pop("RUNTJIK_HOME", None)
            env.pop("TASKTJIK_HOME", None)
            target = home / ".local/lib/runtjik"
            task = home / ".local/lib/tasktjik"
            target.mkdir(parents=True)
            task.mkdir(parents=True)
            for name in [
                "runtjik.py",
                "compat_host.py",
                "capture_worker.py",
                "browser_host.py",
            ]:
                (target / name).write_text("obsolete fixture")
            for name in ["tasktjik.py", "api.py", "client.py"]:
                (task / name).write_text("obsolete fixture")

            def install():
                subprocess.run(
                    ["bash", str(APP / "scripts/install.sh")],
                    env=env,
                    check=True,
                    capture_output=True,
                    text=True,
                )

            install()
            self.assertEqual(list(target.rglob("*.py")), [])
            self.assertEqual(list(task.rglob("*.py")), [])
            core = target / "runtjik-core"
            live = subprocess.Popen(
                [str(core), "serve", "--demo"],
                env={**env, "RUNTJIK_HOME": str(target)},
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                text=True,
            )
            try:
                initial = json.loads(live.stdout.readline())
                self.assertEqual(initial["kind"], "results", initial.get("message"))
                inode = core.stat().st_ino
                install()
                self.assertNotEqual(core.stat().st_ino, inode)
                live.stdin.write('{"op":"query","query":"fire"}\n')
                live.stdin.flush()
                self.assertEqual(
                    json.loads(live.stdout.readline())["results"][0]["name"], "Firefox"
                )
            finally:
                live.stdin.close()
                live.wait(timeout=10)
                live.stdout.close()
            manifest = home / "native hosts"
            subprocess.run(
                [
                    str(core),
                    "register-browser-host",
                    "--runtjik-dir",
                    str(target),
                    "--manifest-dir",
                    str(manifest),
                ],
                check=True,
                capture_output=True,
            )
            registration = json.loads(
                (manifest / "org.tjikos.runtjik_browser.json").read_text()
            )
            wrapper = Path(registration["path"])
            self.assertNotIn("python", wrapper.read_text())
            self.assertEqual(wrapper.stat().st_mode & 0o777, 0o700)
            self.assertIn(
                "tasktjik notify",
                (home / ".config/systemd/user/runtjik-todo.service").read_text(),
            )
            subprocess.run(
                [str(home / ".local/bin/runtjik"), "check"],
                env={**env, "RUNTJIK_BACKEND": "python"},
                check=True,
                capture_output=True,
            )
            result = subprocess.run(
                [str(home / ".local/bin/tasktjik"), "state", "--demo"],
                input="{}",
                env=env,
                text=True,
                capture_output=True,
                check=True,
            )
            self.assertEqual(json.loads(result.stdout)["tasks"], [])


if __name__ == "__main__":
    unittest.main(verbosity=2)
