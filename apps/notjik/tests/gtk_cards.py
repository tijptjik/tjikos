"""Synthetic card/media acceptance on the current compositor, silently on workspace 7.

Measures a finite render/decode job, not idle CPU or steady-state GPU utilization.
Use --full only with a visible workspace: action pagination needs frame callbacks.
"""

import os, json, time, subprocess, statistics, hashlib, sys, tempfile, signal
from pathlib import Path

APP = Path(__file__).resolve().parents[1]
if "--inside" not in sys.argv:
    raise SystemExit(
        subprocess.call(
            [
                "dbus-run-session",
                "--config-file",
                str(APP.parent / "bartjik/tests/bus.conf"),
                "--",
                sys.executable,
                __file__,
                "--inside",
                *sys.argv[1:],
            ]
        )
    )
BASE = Path(
    os.environ.get("NOTJIK_GTK_ARTIFACTS", tempfile.mkdtemp(prefix="notjik-gtk-cards-"))
)
BASE.mkdir(parents=True, exist_ok=True)
binary = Path(os.environ.get("NOTJIK_GTK_TEST_BINARY", APP / "dist/bin/notjik-gtk"))


def hypr(*args):
    return subprocess.check_output(["hyprctl", *args], text=True)


def tree(pid):
    result = {pid}
    for _ in range(6):
        for p in list(result):
            try:
                result.update(
                    map(int, Path(f"/proc/{p}/task/{p}/children").read_text().split())
                )
            except OSError:
                pass
    return result


def sample(pid):
    rows = []
    for p in tree(pid):
        try:
            vals = {
                line.split(":")[0]: int(line.split()[1])
                for line in Path(f"/proc/{p}/smaps_rollup").read_text().splitlines()
                if line.startswith(("Pss:", "Rss:"))
            }
            rows.append(
                {
                    "pid": p,
                    "name": Path(f"/proc/{p}/comm").read_text().strip(),
                    "pss_kib": vals["Pss"],
                }
            )
        except OSError:
            pass
    return {"pss_mib": sum(v["pss_kib"] for v in rows) / 1024, "processes": rows}


rule = f"notjik_media_acceptance_{os.getpid()}"
assert (
    "error"
    not in hypr(
        "eval",
        f'_G.{rule}=hl.window_rule({{name="{rule}",workspace="7 silent",float=true,no_initial_focus=true,no_focus=true,match={{initial_class="^org.tjikos.notjik.cards$"}}}})',
    ).lower()
)
report = {
    "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
    "method": "19 synthetic cards, explicit render snapshots and media frame progression; workspace 7 inactive; includes renderer children; no real notification authority or device actions",
    "runs": [],
}
try:
    for renderer, theme, width in (
        ("gl", "rose-punk", 460),
        ("cairo", "rose-punk", 460),
        ("gl", "monochrome", 380),
        ("cairo", "monochrome", 380),
    ):
        folder = BASE / f"{theme}-{renderer}"
        folder.mkdir(exist_ok=True)
        env = {
            **os.environ,
            "NOTJIK_HOME": str(binary.parent.parent),
            "GTK_A11Y": "none",
            "GSK_RENDERER": renderer,
            "GIO_USE_VFS": "local",
            "SDL_AUDIODRIVER": "dummy",
        }
        cmd = [
            "/usr/bin/time",
            "-f",
            "%e %U %S %M",
            "-o",
            str(folder / "time.txt"),
            str(binary),
            "--demo",
            "--self-test" if "--full" in sys.argv else "--render-test",
            "--theme",
            theme,
            "--width",
            str(width),
            "--snapshot-dir",
            str(folder),
            "--screenshot",
            str(folder / "gallery.png"),
        ]
        start = time.monotonic()
        samples = []
        placed = False
        with (
            (folder / "stdout.log").open("w") as out,
            (folder / "stderr.log").open("w") as err,
        ):
            p = subprocess.Popen(
                cmd, env=env, stdout=out, stderr=err, start_new_session=True
            )
            try:
                while p.poll() is None:
                    if "--full" in sys.argv:
                        assert any(
                            m["activeWorkspace"]["id"] == 7
                            for m in json.loads(hypr("-j", "monitors"))
                        ), "Workspace 7 became hidden during full interaction test"
                    if time.monotonic() - start > 65:
                        raise RuntimeError("gallery timeout")
                    samples.append(sample(p.pid))
                    if not placed:
                        for c in json.loads(hypr("-j", "clients")):
                            if c.get("pid") in tree(p.pid):
                                assert c["workspace"]["id"] == 7
                                assert (
                                    json.loads(hypr("-j", "activewindow")).get(
                                        "address"
                                    )
                                    != c["address"]
                                )
                                placed = True
                    time.sleep(0.2)
            finally:
                if p.poll() is None:
                    os.killpg(p.pid, signal.SIGTERM)
                    p.wait(timeout=5)
        timing = (folder / "time.txt").read_text().strip().splitlines()[-1].split()
        elapsed, user, system, maxrss = map(float, timing)
        row = {
            "renderer": renderer,
            "theme": theme,
            "width": width,
            "exit": p.returncode,
            "placed": placed,
            "elapsed_s": elapsed,
            "cpu_s": user + system,
            "cpu_percent_one_core": 100 * (user + system) / elapsed,
            "median_pss_mib": statistics.median(v["pss_mib"] for v in samples),
            "peak_sampled_pss_mib": max(v["pss_mib"] for v in samples),
            "samples": samples,
            "card_images": len(list(folder.glob("card-*.png"))),
        }
        report["runs"].append(row)
        (BASE / "media-report.json").write_text(json.dumps(report, indent=2))
        print({k: v for k, v in row.items() if k != "samples"}, flush=True)
        assert p.returncode == 0 and placed and row["card_images"] == 19, (
            folder / "stderr.log"
        ).read_text()
        assert (
            "GTK card acceptance:" if "--full" in sys.argv else "GTK media acceptance:"
        ) in (folder / "stdout.log").read_text()
        assert "Gtk-CRITICAL" not in (folder / "stderr.log").read_text()
    print(f"GTK artifacts: {BASE}", flush=True)
finally:
    hypr("eval", f"if _G.{rule} then _G.{rule}:set_enabled(false); _G.{rule}=nil end")
