"""Private-bus systemd fixture; fake recorder is sleep and never captures a screen."""
import json
import os
from pathlib import Path
import selectors
import shutil
import socket
import subprocess
import sys
import tempfile
import threading

ROOT = Path(__file__).resolve().parents[1]


def serve(pid_file):
    import dbus
    import dbus.service
    import dbus.mainloop.glib
    from gi.repository import GLib

    dbus.mainloop.glib.DBusGMainLoop(set_as_default=True)
    bus = dbus.SessionBus()
    name = dbus.service.BusName("org.freedesktop.systemd1", bus)

    class Service(dbus.service.Object):
        @dbus.service.method("org.freedesktop.DBus.Properties", in_signature="ss", out_signature="v")
        def Get(self, interface, prop):
            assert interface == "org.freedesktop.systemd1.Service" and prop == "MainPID"
            return dbus.UInt32(int(Path(pid_file).read_text()))

    service = Service(bus, "/org/freedesktop/systemd1/unit/screen_2drecord_2eservice")
    print("ready", flush=True)
    GLib.MainLoop().run()


def main():
    assert os.environ.get("DBUS_SESSION_BUS_ADDRESS", "").startswith("unix:path=/tmp/")
    with tempfile.TemporaryDirectory(prefix="screen-record-status-") as directory:
        temp = Path(directory)
        pid_file = temp / "pid"
        pid_file.write_text("0")
        socket_dir = temp / "runtjik-capture"
        socket_dir.mkdir()
        fake_recorder = temp / "gpu-screen-recorder"
        shutil.copy2("/usr/bin/sleep", fake_recorder)
        with subprocess.Popen(["/usr/bin/python3", __file__, "serve", str(pid_file)], stdout=subprocess.PIPE, text=True) as server:
            assert server.stdout.readline().strip() == "ready"
            helper = subprocess.Popen([str(ROOT / "dist/bin/bartjik-core"), "helper", "screen-record-status"], stdout=subprocess.PIPE,
                                      env={**os.environ, "XDG_RUNTIME_DIR": str(temp)})
            selector = selectors.DefaultSelector()
            selector.register(helper.stdout, selectors.EVENT_READ)

            def expect(available, recording):
                assert selector.select(5), "No status update"
                assert json.loads(helper.stdout.readline()) == {"available": available, "recording": recording}

            try:
                expect(True, False)
                pid_file.write_text(str(os.getpid()))
                with subprocess.Popen([str(fake_recorder), "30"]) as recorder:
                    expect(True, True)
                    recorder.terminate()
                    recorder.wait(timeout=3)
                expect(True, False)
                pid_file.write_text("0")
                assert not selector.select(.6), "Unchanged state must not be emitted"
                server.terminate()
                server.wait(timeout=3)
                expect(False, False)
                # Runtjik owns a socket, independent of the legacy systemd unit.
                for stopping in (False, True):
                    with socket.socket(socket.AF_UNIX) as listener:
                        listener.bind(str(socket_dir / "recorder.sock"))
                        listener.listen()
                        listener.settimeout(5)

                        def reply():
                            connection, _ = listener.accept()
                            with connection:
                                assert connection.recv(64) == b"status\n"
                                connection.sendall(json.dumps({"file": "synthetic.mp4", "mode": "region",
                                                               "stopping": stopping}).encode())

                        thread = threading.Thread(target=reply)
                        thread.start()
                        expect(True, True)
                        thread.join(timeout=5)
                    # A stale socket after a crash must clear the indicator.
                    expect(False, False)
                    (socket_dir / "recorder.sock").unlink()
            finally:
                helper.terminate()
                try:
                    helper.wait(timeout=3)
                except subprocess.TimeoutExpired:
                    helper.kill()
                    helper.wait(timeout=3)
                    raise
                selector.close()
                if server.poll() is None:
                    server.terminate()
        print("Screen recorder: systemd lifecycle, Runtjik recording/finalization, stale socket and change-only output passed")


if __name__ == "__main__":
    if sys.argv[1:2] == ["serve"]:
        serve(sys.argv[2])
    else:
        main()
