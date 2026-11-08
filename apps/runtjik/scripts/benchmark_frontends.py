#!/usr/bin/env python3
"""Paired GTK/Quickshell benchmark, synthetic index and isolated compositor only.

Run through dev-check. The host's dedicated nested window is placed on workspace
7 silently before launch by the GTK test harness; all monitor/input operations
below explicitly target the private compositor. No user index or history is read.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import statistics
import subprocess
import sys
import time

APP = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(APP / "tests"))
from gtk_ui import silent_compositor
from rust_parity import fixture


def run(args, env, check=True):
    return subprocess.run([str(a) for a in args], env=env, check=check,
                          text=True, capture_output=True, timeout=30).stdout.strip()


def wait(check, timeout=20):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            result = check()
            if result:
                return result
        except (subprocess.CalledProcessError, json.JSONDecodeError, KeyError):
            pass
        time.sleep(.005)
    raise RuntimeError("Benchmark endpoint timed out")


def process_ids(pid):
    ids = {pid}
    pending = [pid]
    while pending:
        parent = pending.pop()
        for task in Path(f"/proc/{parent}/task").glob("*"):
            try:
                for child in (task / "children").read_text().split():
                    child = int(child)
                    if child not in ids:
                        ids.add(child)
                        pending.append(child)
            except FileNotFoundError:
                pass
    return ids


def sample(pid):
    rows = []
    for child in sorted(process_ids(pid)):
        try:
            stat = Path(f"/proc/{child}/stat").read_text().rsplit(")", 1)[1].split()
            smaps = dict(line.split(":", 1) for line in Path(f"/proc/{child}/smaps_rollup").read_text().splitlines() if ":" in line)
            rows.append({"pid": child, "name": Path(f"/proc/{child}/comm").read_text().strip(),
                         "start": stat[19], "cpu_ticks": int(stat[11]) + int(stat[12]),
                         "pss_kib": int(smaps["Pss"].split()[0]), "rss_kib": int(smaps["Rss"].split()[0])})
        except FileNotFoundError:
            pass
    assert rows, pid
    return {"pss_mib": sum(r["pss_kib"] for r in rows) / 1024,
            "rss_mib": sum(r["rss_kib"] for r in rows) / 1024, "processes": rows}


def idle(pid, seconds):
    before = sample(pid)
    start = time.monotonic()
    time.sleep(seconds)  # No status polling during the idle CPU sample.
    after = sample(pid)
    duration = time.monotonic() - start
    prior = {(r["pid"], r["start"]): r["cpu_ticks"] for r in before["processes"]}
    ticks = sum(r["cpu_ticks"] - prior.get((r["pid"], r["start"]), r["cpu_ticks"]) for r in after["processes"])
    return {**after, "cpu_percent_one_core": 100 * ticks / os.sysconf("SC_CLK_TCK") / duration,
            "sample_seconds": duration}


class Frontend:
    def __init__(self, name, home, env, private_hypr):
        self.name, self.home, self.env, self.hypr = name, home, env, private_hypr

    def command(self, action):
        args = [self.home / "bin/runtjik"]
        if self.name == "gtk":
            args.append("--gtk")
        return run(args + [action], self.env)

    def state(self):
        return json.loads(self.command("status"))

    def mapped(self, state):
        if self.name == "gtk" and state.get("surface") != "layer":
            return any(c["pid"] == state["pid"] and c["mapped"] for c in json.loads(self.hypr("-j", "clients")))
        namespace = "runtjik-gtk" if self.name == "gtk" else "runtjik"
        return any(layer["namespace"] == namespace for monitor in json.loads(self.hypr("-j", "layers")).values()
                   for levels in monitor["levels"].values() for layer in levels)

    def settled(self, query=""):
        state = self.state()
        if self.name == "quickshell":
            ready = state["ready"] and not state["busy"] and state["query"] == query and state["resultQuery"] == query
        else:
            ready = state["ready"] and not state["busy"] and state["query"] == query
        return state if ready and state["visible"] else None

    def show(self):
        start = time.perf_counter()
        self.command("show")
        state = wait(lambda: self.settled())
        wait(lambda: self.mapped(state))
        return (time.perf_counter() - start) * 1000, state

    def close(self):
        self.command("close")
        wait(lambda: not self.state()["visible"])
        time.sleep(.15)

    def quit(self):
        self.command("quit")
        time.sleep(.15)

    def search(self, query):
        start = time.perf_counter()
        run(["wtype", "-M", "ctrl", "a", "-m", "ctrl", "-k", "BackSpace", query], self.env)
        state = wait(lambda: self.settled(query))
        ids = [r["id"] for r in state["rows"]]
        return {"query": query, "ms": (time.perf_counter() - start) * 1000,
                "ids": ids, "total": state["total"]}


def summarize(rows):
    output = {}
    for renderer in sorted({r["renderer"] for r in rows}):
        output[renderer] = {}
        for frontend in ("quickshell", "gtk"):
            subset = [r for r in rows if r["renderer"] == renderer and r["frontend"] == frontend]
            if not subset:
                continue
            metrics = {"cold_ready_ms": [r["cold_ready_ms"] for r in subset],
                       "warm_ready_ms": [v for r in subset for v in r["warm_ready_ms"]],
                       "search_ms": [s["ms"] for r in subset for s in r["searches"]],
                       "open_pss_mib": [r["open"]["pss_mib"] for r in subset],
                       "hidden_pss_mib": [r["hidden"]["pss_mib"] for r in subset],
                       "open_idle_cpu_pct": [r["open"]["cpu_percent_one_core"] for r in subset],
                       "hidden_idle_cpu_pct": [r["hidden"]["cpu_percent_one_core"] for r in subset]}
            output[renderer][frontend] = {key: {"median": round(statistics.median(values), 3),
                                                "min": round(min(values), 3), "max": round(max(values), 3), "n": len(values)}
                                             for key, values in metrics.items()}
    return output


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    parser.add_argument("--trials", type=int, default=5)
    parser.add_argument("--entries", type=int, default=3000)
    parser.add_argument("--idle-seconds", type=float, default=3)
    parser.add_argument("--renderers", nargs="+", choices=["software", "opengl"], default=["software", "opengl"])
    args = parser.parse_args()
    assert not args.output.exists(), "Choose a new output path"
    assert args.trials > 0 and args.entries >= 3000 and args.idle_seconds >= 1, "Use at least 3,000 entries for the fixed search workload"
    result = {"date": time.strftime("%Y-%m-%dT%H:%M:%S%z"), "kernel": platform.release(),
              "machine": platform.machine(), "entries": args.entries, "trials_per_frontend_per_renderer": args.trials,
              "quickshell_version": run(["quickshell", "--version"], os.environ),
              "hashes": {name: hashlib.sha256((APP / "dist" / name).read_bytes()).hexdigest() for name in ["runtjik-core", "runtjik-gtk"]},
              "method": {"endpoint": "CLI invocation to results ready plus compositor mapping; search is keyboard command to result-model update, not frame presentation",
                         "caches": "OS caches retained; one unmeasured launch per frontend/renderer primes application caches",
                         "order": "alternates each trial", "size": "720x600 list, 100 results per page",
                         "memory": "sum PSS of frontend and all descendant backend/helpers; compositor excluded",
                         "cpu": "no polling during idle sample; percent of one CPU core",
                         "instrumentation": "private QML copy adds read-only query/busy/rows fields to status; no production changes",
                         "limitations": "Both frontends use layer-shell; measures launcher list workload, not full provider parity; no physical frame latency or GPU memory measured"}, "rows": []}
    def save():
        result["summary"] = summarize(result["rows"])
        args.output.write_text(json.dumps(result, indent=2) + "\n")
    with silent_compositor():
        runtime = Path(os.environ["XDG_RUNTIME_DIR"])
        instance = os.environ["HYPRLAND_INSTANCE_SIGNATURE"]
        assert str(runtime).startswith("/tmp/bc-") and (runtime / "hypr" / instance / ".socket.sock").is_socket()
        base_env = dict(os.environ)
        def hypr(*arguments):
            return run(["hyprctl", "--instance", instance, *arguments], base_env)
        hypr("output", "create", "headless", "BENCH")
        hypr("eval", 'hl.monitor({output="WAYLAND-1",disabled=true}); hl.monitor({output="BENCH",mode="1000x680@60",position="0x0",scale=1}); hl.dispatch(hl.dsp.focus({monitor="BENCH"})); hl.window_rule({name="gtk-benchmark",float=true,no_anim=true,match={title="^Runtjik · GTK preview$"}})')
        root = runtime.parent / "frontend-benchmark"
        root.mkdir()
        home = root / "app"
        shutil.copytree(APP / "dist", home)
        # Observation only: retain normal non-demo warm lifecycle and real input paths.
        qml = home / "qml/shell.qml"
        source = qml.read_text()
        needle = 'JSON.stringify({version:root.runtimeVersion,pid:Quickshell.processId'
        assert source.count(needle) == 1
        qml.write_text(source.replace(needle, 'JSON.stringify({query:root.query,resultQuery:root.resultQuery,busy:root.busy,rows:root.results,version:root.runtimeVersion,pid:Quickshell.processId'))
        data = root / "fixture"
        data.mkdir()
        env = fixture(data, args.entries)
        env.update(RUNTJIK_HOME=str(home), GTK_A11Y="none", QT_QUICK_CONTROLS_STYLE="Basic",
                   GDK_BACKEND="wayland", QT_QPA_PLATFORM="wayland", QSG_INFO="1")
        env.pop("RUNTJIK_DEMO", None)
        env["HYPRLAND_INSTANCE_SIGNATURE"] = instance  # fixture() removes it for backend-only checks.
        Path(env["RUNTJIK_CONFIG"]).write_text('[index]\npath_commands=false\nrefresh_seconds=3600\n[launch]\nisolate=false\n[ui]\nview="list"\nwidth=720\nheight=600\nmax_results=100\nauto_launch_applications=false\nauto_launch_commands=false\n[theme]\nicon_theme="Adwaita"\n[stats]\nenabled=false\n[history]\nenabled=false\n')
        result["private_logs"] = str(root)
        expected = {}
        for renderer in args.renderers:
            renderer_env = dict(env)
            renderer_env.pop("QT_QUICK_BACKEND", None)
            renderer_env.pop("GSK_RENDERER", None)
            renderer_env.pop("QSG_RHI_BACKEND", None)
            if renderer == "software":
                renderer_env.update(GSK_RENDERER="cairo", QT_QUICK_BACKEND="software")
            else:
                renderer_env.update(GSK_RENDERER="gl", QSG_RHI_BACKEND="opengl")
            frontends = {name: Frontend(name, home, renderer_env, hypr) for name in ("quickshell", "gtk")}
            try:
                for frontend in frontends.values():
                    frontend.show()
                    time.sleep(.5)
                    frontend.quit()
                for trial in range(args.trials):
                    order = ["quickshell", "gtk"] if trial % 2 == 0 else ["gtk", "quickshell"]
                    for name in order:
                        frontend = frontends[name]
                        cold, state = frontend.show()
                        pid = state["pid"]
                        time.sleep(.5)
                        row = {"renderer": renderer, "frontend": name, "trial": trial + 1,
                               "cold_ready_ms": cold, "open": idle(pid, args.idle_seconds)}
                        searches = []
                        for query in ["Fixture App 0299", "Fixture App 1", "no-such-fixture", "Fixture App 00", "Fixture App 2999"]:
                            search = frontend.search(query)
                            comparable = (search["ids"], search["total"])
                            if query in expected:
                                assert comparable == expected[query], (name, query, comparable, expected[query])
                            else:
                                expected[query] = comparable
                            assert (query == "no-such-fixture") == (search["total"] == 0), search
                            searches.append(search)
                        row["searches"] = searches
                        frontend.close()
                        row["hidden"] = idle(pid, args.idle_seconds)
                        row["warm_ready_ms"] = []
                        for _ in range(5):
                            warm, current = frontend.show()
                            assert current["pid"] == pid
                            row["warm_ready_ms"].append(warm)
                            frontend.close()
                        frontend.quit()
                        result["rows"].append(row)
                        save()
                        print(f'{renderer} {name} trial {trial + 1}: cold {cold:.1f} ms, warm {statistics.median(row["warm_ready_ms"]):.1f} ms, PSS {row["open"]["pss_mib"]:.1f} MiB', flush=True)
            finally:
                for frontend in frontends.values():
                    frontend.quit()
        result["correctness"] = "Ordered IDs and totals agree across every query, frontend, trial and renderer"
        save()
    print(json.dumps(result["summary"], indent=2))


if __name__ == "__main__":
    main()
