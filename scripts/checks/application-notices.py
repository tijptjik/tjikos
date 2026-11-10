"""Verify built notices and supported installation paths using isolated homes."""
import hashlib
import os
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
EXPECTED = {
    "bartjik": ("tjik-desktop", "tjik-gtk", "notjik"),
    "notjik": ("tjik-desktop", "tjik-gtk"),
    "runtjik": ("tjik-desktop", "tjik-gtk", "tasktjik-gtk", "tasktjik"),
    "tasktjik": (),
}


def run(*command, env):
    subprocess.run(list(command), cwd=ROOT, env=env, check=True)


def verify(app, home, env):
    source = ROOT / "apps" / app / "dist"
    target = home / ".local/lib" / app
    run("python3", "-B", "scripts/package-notices.py", "verify", str(source), str(target), env=env)
    for name in ("LICENSE", "NOTICE"):
        assert (source / name).read_bytes() == (ROOT / "apps" / app / name).read_bytes()
    directories = sorted(p.name for p in (source / "licenses").iterdir() if p.is_dir())
    assert directories == sorted(EXPECTED[app]), (app, directories)
    for component in EXPECTED[app]:
        parent = "apps" if component in ("notjik", "tasktjik") else "packages"
        for name in ("LICENSE", "NOTICE"):
            assert (source / "licenses" / component / name).read_bytes() == (ROOT / parent / component / name).read_bytes()


with tempfile.TemporaryDirectory(prefix="tjik-notice-installs-") as directory:
    for app in EXPECTED:
        home = Path(directory) / app
        env = {**os.environ, "TJIKUP_MANAGED_CONFIG": "1",
               app.upper() + "_INSTALL_HOME": str(home),
               app.upper() + "_INSTALL_CONFIG": str(home / "config")}
        if app == "notjik":
            run("bash", "scripts/install.sh", "--from-build", "notjik", env=env)
        else:
            run("bash", f"apps/{app}/scripts/install.sh", env=env)
        verify(app, home, env)
        print(f"PASS: {app} build and full installer notices")
    for mode in ("--gtk-only", "--notification-assets-only", "--licenses-only"):
        home = Path(directory) / mode.removeprefix("--")
        target = home / ".local/lib/bartjik/bin/bartjik-gtk"
        target.parent.mkdir(parents=True)
        target.write_bytes(b"synthetic binary sentinel")
        before = hashlib.sha256(target.read_bytes()).digest()
        env = {**os.environ, "TJIKUP_MANAGED_CONFIG": "1",
               "BARTJIK_INSTALL_HOME": str(home), "BARTJIK_INSTALL_CONFIG": str(home / "config")}
        run("bash", "apps/bartjik/scripts/install.sh", mode, env=env)
        verify("bartjik", home, env)
        if mode != "--gtk-only":
            assert hashlib.sha256(target.read_bytes()).digest() == before, mode
        print(f"PASS: Bartjik {mode} notices and binary replacement boundary")
