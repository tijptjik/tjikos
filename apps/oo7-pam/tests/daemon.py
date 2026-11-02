"""Exercise the installed oo7 daemon using only private, synthetic test state.

Run after `cargo test`: python3 tests/daemon.py
The parent always creates a new D-Bus session. No live keyring is opened.
"""
import ctypes
import os
from pathlib import Path
import pwd
import subprocess
import sys
import tempfile
import time

import dbus


APP = Path(__file__).resolve().parents[1]
SECRET_SERVICE = "org.freedesktop.secrets"
COLLECTION = "/org/freedesktop/secrets/collection/login"


def bus(*args):
    return subprocess.check_output(
        ["busctl", "--user", *args], text=True, stderr=subprocess.DEVNULL
    ).strip()


def locked():
    return bus(
        "get-property", SECRET_SERVICE, COLLECTION,
        "org.freedesktop.Secret.Collection", "Locked",
    ) == "b true"


def wait_for(predicate):
    deadline = time.monotonic() + 8
    while time.monotonic() < deadline:
        try:
            if predicate():
                return
        except (subprocess.CalledProcessError, FileNotFoundError):
            pass
        time.sleep(0.05)
    raise AssertionError("isolated daemon did not reach the expected state")


def transaction(root, option="", finish=True):
    module = APP / "dist/pam_oo7_tjik.so"
    tokens = APP / "target/debug/examples/libpam_test_tokens.so"
    assert module.is_file() and tokens.is_file()
    (root / "auth-test").write_text(
        f"auth required {tokens} {option}\n"
        f"auth optional {module} unlock_on_auth\n"
    )
    # Hyprlock links libpam globally. Match that loader scope so modules can
    # resolve pam_get_item/pam_set_data just as they do in the real caller.
    pam = ctypes.CDLL("libpam.so.0", mode=ctypes.RTLD_GLOBAL)
    pointer = ctypes.c_void_p

    class Conversation(ctypes.Structure):
        _fields_ = [("callback", pointer), ("data", pointer)]

    pam.pam_start_confdir.argtypes = [
        ctypes.c_char_p, ctypes.c_char_p, ctypes.POINTER(Conversation),
        ctypes.c_char_p, ctypes.POINTER(pointer),
    ]
    pam.pam_authenticate.argtypes = [pointer, ctypes.c_int]
    pam.pam_end.argtypes = [pointer, ctypes.c_int]
    handle = pointer()
    conversation = Conversation(None, None)
    username = pwd.getpwuid(os.getuid()).pw_name.encode()
    assert pam.pam_start_confdir(
        b"auth-test", username, ctypes.byref(conversation),
        os.fsencode(root), ctypes.byref(handle),
    ) == 0
    status = pam.pam_authenticate(handle, 0)
    assert locked(), "keyring unlocked before PAM completed"
    assert (status != 0) if option == "reject" else (status == 0)
    assert pam.pam_end(handle, status) == 0
    if finish:
        wait_for(lambda: not locked())
    else:
        time.sleep(0.2)
        assert locked(), "rejected or passwordless authentication unlocked keyring"


def inside_session(directory):
    root = Path(directory)
    assert root.is_dir() and root.stat().st_mode & 0o777 == 0o700
    assert os.environ["OO7_PAM_SOCKET"] == str(root / "pam.sock")
    assert os.environ["XDG_DATA_HOME"] == str(root / "data")
    assert os.environ.get("OO7_TEST_PRIVATE_SESSION") == "1"
    log_path = root / "daemon.log"
    with log_path.open("wb") as log:
        # --login bypasses the real user's boot-time login-helper socket.
        daemon = subprocess.Popen(
            ["/usr/libexec/oo7-daemon", "--login"],
            stdin=subprocess.PIPE, stdout=log, stderr=log,
        )
        try:
            daemon.stdin.write(b"old-password")
            daemon.stdin.close()
            wait_for(lambda: not locked() and (root / "pam.sock").exists())
            # An empty keyring has no encrypted item against which oo7 can
            # validate a password. Store a synthetic item before testing it.
            connection = dbus.SessionBus()
            service = dbus.Interface(
                connection.get_object(SECRET_SERVICE, "/org/freedesktop/secrets"),
                "org.freedesktop.Secret.Service",
            )
            _, session = service.OpenSession("plain", dbus.String("", variant_level=1))
            collection = dbus.Interface(
                connection.get_object(SECRET_SERVICE, COLLECTION),
                "org.freedesktop.Secret.Collection",
            )
            collection.CreateItem(
                dbus.Dictionary({
                    "org.freedesktop.Secret.Item.Label": "Synthetic PAM test",
                    "org.freedesktop.Secret.Item.Attributes": dbus.Dictionary(
                        {"fixture": "oo7-pam"}, signature="ss",
                    ),
                }, signature="sv"),
                dbus.Struct((
                    session, dbus.Array([], signature="y"),
                    dbus.ByteArray(b"synthetic-fixture-value"), "text/plain",
                )),
                True,
            )
            bus("call", SECRET_SERVICE, "/org/freedesktop/secrets",
                "org.freedesktop.Secret.Service", "Lock", "ao", "1", COLLECTION)
            wait_for(locked)
            for option in ["reject", "wrong_token", "no_token", "empty_token"]:
                transaction(root, option, finish=False)
            transaction(root)
            bus("call", SECRET_SERVICE, "/org/freedesktop/secrets",
                "org.freedesktop.Secret.Service", "Lock", "ao", "1", COLLECTION)
            wait_for(locked)
            transaction(root)
            print("PASS: isolated oo7 keyring unlocks only after successful password PAM completion")
        except BaseException:
            # Contains only synthetic keyring activity, never real credentials.
            sys.stderr.write(log_path.read_text())
            raise
        finally:
            daemon.terminate()
            try:
                daemon.wait(timeout=5)
            except subprocess.TimeoutExpired:
                daemon.kill()
                daemon.wait()


if __name__ == "__main__":
    if len(sys.argv) == 3 and sys.argv[1] == "--inside-session":
        inside_session(sys.argv[2])
    else:
        with tempfile.TemporaryDirectory(prefix="oo7-pam-daemon-") as temporary:
            root = Path(temporary)
            (root / "data").mkdir()
            (root / "runtime").mkdir(mode=0o700)
            environment = {
                **os.environ,
                "XDG_DATA_HOME": str(root / "data"),
                "XDG_CONFIG_HOME": str(root / "config"),
                "XDG_CACHE_HOME": str(root / "cache"),
                "XDG_RUNTIME_DIR": str(root / "runtime"),
                "OO7_PAM_SOCKET": str(root / "pam.sock"),
                "OO7_TEST_PRIVATE_SESSION": "1",
            }
            environment.pop("DBUS_SESSION_BUS_ADDRESS", None)
            environment.pop("CREDENTIALS_DIRECTORY", None)
            result = subprocess.run(
                ["dbus-run-session", "--", sys.executable, __file__, "--inside-session", temporary],
                env=environment, timeout=45,
            )
            raise SystemExit(result.returncode)
