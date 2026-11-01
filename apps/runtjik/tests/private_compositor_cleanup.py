"""Process-level teardown regressions; no desktop or private data required."""
import os
from pathlib import Path
import select
import signal
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

from private_compositor import _pidfd_call, private_compositor, stop_runtime_clients


class CleanupTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="compositor-cleanup-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.children = []
        self.addCleanup(self.reap_children)

    def reap_children(self):
        for child in self.children:
            if child.poll() is None:
                child.kill()
            child.wait(timeout=5)

    def client(self, runtime, stubborn=False):
        code = (
            "import signal,time; "
            + ("signal.signal(signal.SIGTERM, signal.SIG_IGN); " if stubborn else "")
            + "print('ready', flush=True); time.sleep(300)"
        )
        child = subprocess.Popen(
            [sys.executable, "-c", code],
            env={**os.environ, "XDG_RUNTIME_DIR": str(runtime)},
            start_new_session=True,
            stdout=subprocess.PIPE,
            text=True,
        )
        self.children.append(child)
        self.assertEqual(child.stdout.readline().strip(), "ready")
        child.stdout.close()
        return child

    def test_detached_clients_and_force_kill_are_scoped_to_exact_runtime(self):
        runtime = self.root / "runtime"
        graceful = self.client(runtime)
        stubborn = self.client(runtime, stubborn=True)
        other = self.client(self.root / "runtime-other")
        compositor = self.client(runtime)
        stop_runtime_clients(runtime, compositor.pid, grace=.15)
        self.assertEqual(graceful.wait(timeout=2), -signal.SIGTERM)
        self.assertEqual(stubborn.wait(timeout=2), -signal.SIGKILL)
        self.assertIsNone(other.poll())
        self.assertIsNone(compositor.poll())
        stop_runtime_clients(runtime, compositor.pid, grace=.15)

    def fake_compositor(self):
        executable = self.root / "Hyprland"
        executable.write_text(
            f"#!{sys.executable}\n"
            "import os, pathlib, time\n"
            "root = pathlib.Path(os.environ['XDG_RUNTIME_DIR'])\n"
            "socket = root / 'hypr/test/.socket.sock'\n"
            "socket.parent.mkdir(parents=True)\n"
            "socket.touch()\n"
            "(root / 'wayland-test').touch()\n"
            "time.sleep(300)\n"
        )
        executable.chmod(0o700)

    def test_client_survives_launcher_but_not_runtime_cleanup(self):
        runtime = self.root / "orphan-runtime"
        launcher = (
            "import subprocess, sys; "
            "child = subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(300)'], "
            "start_new_session=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL); "
            "print(child.pid)"
        )
        pid = int(subprocess.check_output(
            [sys.executable, "-c", launcher],
            env={**os.environ, "XDG_RUNTIME_DIR": str(runtime)},
            text=True,
        ))
        fd = _pidfd_call("pidfd_open", pid, 0)
        try:
            self.assertEqual(select.select([fd], [], [], 0)[0], [])
            stop_runtime_clients(runtime, -1, grace=.15)
            self.assertEqual(select.select([fd], [], [], 2)[0], [fd])
        finally:
            try:
                _pidfd_call("pidfd_send_signal", fd, signal.SIGKILL, None, 0)
            except ProcessLookupError:
                pass
            os.close(fd)

    def test_context_cleans_clients_and_restores_environment_on_failure(self):
        self.fake_compositor()
        with patch.dict(os.environ, {
            "PATH": str(self.root) + os.pathsep + os.environ["PATH"],
            "XDG_RUNTIME_DIR": str(self.root / "desktop"),
        }):
            before = dict(os.environ)
            with self.assertRaisesRegex(ValueError, "synthetic test failure"):
                with private_compositor():
                    child = self.client(os.environ["XDG_RUNTIME_DIR"])
                    raise ValueError("synthetic test failure")
            self.assertEqual(child.wait(timeout=2), -signal.SIGTERM)
            self.assertEqual(dict(os.environ), before)

    def test_context_cleans_clients_on_normal_exit(self):
        self.fake_compositor()
        with patch.dict(os.environ, {
            "PATH": str(self.root) + os.pathsep + os.environ["PATH"],
            "XDG_RUNTIME_DIR": str(self.root / "desktop"),
        }):
            with private_compositor():
                child = self.client(os.environ["XDG_RUNTIME_DIR"])
            self.assertEqual(child.wait(timeout=2), -signal.SIGTERM)


if __name__ == "__main__":
    unittest.main()
