"""Exercise the native app opener against synthetic desktop entries and Hyprland."""

import json
import os
from pathlib import Path
import re
import socketserver
import subprocess
import sys
import tempfile
import threading
import unittest


APP = Path(__file__).resolve().parents[1]
BIN = APP / "target/release/bartjik-core"


class AppOpenTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="bartjik-app-open-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.events = self.root / "events.jsonl"
        self.clients = []
        self.special = ""
        self.launch_class = "Chatgpt"
        self.env = {
            **os.environ,
            "XDG_RUNTIME_DIR": str(self.root),
            "HYPRLAND_INSTANCE_SIGNATURE": "fixture",
            "XDG_DATA_HOME": str(self.root / "user"),
            "XDG_DATA_DIRS": str(self.root / "system"),
            "PATH": str(self.root / "bin") + os.pathsep + os.environ["PATH"],
        }
        for directory in ("user/applications", "system/applications", "bin", "hypr/fixture"):
            (self.root / directory).mkdir(parents=True)
        self.desktop("chatgpt.desktop", "ChatGPT")
        self.desktop("FFPWA-SYNTHETIC.desktop", "Codex LB", "FFPWA-SYNTHETIC")
        for name in ("hyprctl", "systemd-run"):
            script = self.root / "bin" / name
            script.write_text(
                f"#!{sys.executable}\n"
                "import json, sys\n"
                f"with open({str(self.events)!r}, 'a') as output:\n"
                f"    output.write(json.dumps([{name!r}, *sys.argv[1:]]) + '\\n')\n"
                "print('ok')\n"
            )
            script.chmod(0o755)
        owner = self

        class Handler(socketserver.BaseRequestHandler):
            def handle(self):
                query = self.request.recv(4096).decode()
                clients = list(owner.clients)
                if any(event[0] == "systemd-run" for event in owner.recorded()):
                    clients.append(owner.client("0xabc", owner.launch_class))
                if query == "j/clients":
                    response = clients
                elif query == "j/monitors":
                    response = [{"id": 0, "name": "SYNTHETIC", "specialWorkspace": {"name": owner.special}}]
                elif query == "j/activewindow":
                    targets = [re.search(r'window = "address:(0x[0-9a-f]+)"', e[-1]) for e in owner.recorded()]
                    address = next((match[1] for match in reversed(targets) if match), "")
                    response = {"address": address}
                else:
                    response = {}
                self.request.sendall(json.dumps(response).encode())

        server = socketserver.UnixStreamServer(str(self.root / "hypr/fixture/.socket.sock"), Handler)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()

        def close():
            server.shutdown()
            server.server_close()
            thread.join(timeout=2)

        self.addCleanup(close)

    def desktop(self, filename, name, wmclass="", directory="user", hidden=False):
        path = self.root / directory / "applications" / filename
        path.write_text(f"[Desktop Entry]\nType=Application\nName={name}\nExec=fixture\n"
                        + (f"StartupWMClass={wmclass}\n" if wmclass else "")
                        + ("Hidden=true\n" if hidden else "")
                        + "[Desktop Action Ignore]\nName=Wrong app\nStartupWMClass=wrong\n")
        return path

    def client(self, address, wmclass, workspace="1", recent=0, **extra):
        return {"address": address, "class": wmclass, "initialClass": wmclass,
                "monitor": 0, "mapped": True, "focusHistoryID": recent,
                "workspace": {"name": workspace}, **extra}

    def recorded(self):
        return [json.loads(line) for line in self.events.read_text().splitlines()] if self.events.exists() else []

    def open_app(self, app):
        result = subprocess.run([str(BIN), "helper", "open-app", app], env=self.env,
                                capture_output=True, text=True, timeout=5)
        self.assertEqual(result.returncode, 0, result.stderr)
        return self.recorded()

    def test_existing_chatgpt_reveals_special_without_launching_or_toggling_it_off(self):
        self.clients = [self.client("0xaaa", "Chatgpt", "special:codex")]
        events = self.open_app("chatgpt")
        self.assertFalse(any(e[0] == "systemd-run" for e in events))
        self.assertIn('hl.dsp.workspace.toggle_special("codex")', [e[-1] for e in events])
        self.special = "special:codex"
        self.events.unlink()
        events = self.open_app("chatgpt")
        self.assertFalse(any("toggle_special" in e[-1] for e in events))
        self.assertIn('window = "address:0xaaa"', events[-1][-1])

    def test_percentage_focuses_most_recent_codex_lb_without_matching_chatgpt(self):
        self.clients = [self.client("0xaaa", "Chatgpt"),
                        self.client("0xbbb", "FFPWA-SYNTHETIC", recent=4),
                        self.client("0xccc", "firefox", recent=2, initialClass="ffpwa-synthetic"),
                        self.client("0xddd", "FFPWA-SYNTHETIC", mapped=False)]
        self.special = "special:codex"
        events = self.open_app("codex-lb")
        self.assertFalse(any(e[0] == "systemd-run" for e in events))
        self.assertIn('hl.dsp.workspace.toggle_special("codex")', [e[-1] for e in events])
        self.assertIn('window = "address:0xccc"', events[-1][-1])

    def test_closed_apps_launch_installed_entry_once_and_focus_new_window(self):
        for app, filename, wmclass in [("chatgpt", "chatgpt.desktop", "Chatgpt"),
                                       ("codex-lb", "FFPWA-SYNTHETIC.desktop", "FFPWA-SYNTHETIC")]:
            with self.subTest(app=app):
                self.events.unlink(missing_ok=True)
                self.launch_class = wmclass
                events = self.open_app(app)
                launches = [e for e in events if e[0] == "systemd-run"]
                self.assertEqual(len(launches), 1)
                self.assertEqual(launches[0][1:], ["--user", "--collect", "--quiet", "--", "gio", "launch",
                                                  str(self.root / "user/applications" / filename)])
                self.assertIn('window = "address:0xabc"', events[-1][-1])
                self.open_app(app)
                self.assertEqual(sum(e[0] == "systemd-run" for e in self.recorded()), 1)

    def test_hidden_user_entry_masks_system_launcher(self):
        self.desktop("chatgpt.desktop", "ChatGPT", hidden=True)
        self.desktop("chatgpt.desktop", "ChatGPT", directory="system")
        result = subprocess.run([str(BIN), "helper", "open-app", "chatgpt"], env=self.env,
                                capture_output=True, text=True, timeout=5)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("not installed", result.stderr)
        self.assertEqual(self.recorded(), [])


if __name__ == "__main__":
    unittest.main()
