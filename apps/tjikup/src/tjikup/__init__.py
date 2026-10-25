"""Run explicitly configured scripts and display their states."""

from __future__ import annotations

import argparse
from concurrent.futures import ThreadPoolExecutor
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import threading
import time

from .config import load_config
from . import runtime
from .runtime import UpdateError

ACTIVE_REPORT_FILE = None
CHILDREN = set()
CHILDREN_LOCK = threading.Lock()
CANCELLED = threading.Event()


def stop_children():
    CANCELLED.set()
    with CHILDREN_LOCK:
        for process in CHILDREN:
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass


def execute(step, command, env):
    started = time.monotonic()
    timeout = step.get("timeout", 300)
    output = ""
    if step.get("output", "quiet") == "stream":
        code = runtime.run_stream(
            command, Path(step["cwd"]), env=env, timeout=timeout, check=False
        )
    else:
        # Parallel commands cannot read the terminal; diagnostics are buffered per step.
        with CHILDREN_LOCK:
            if CANCELLED.is_set():
                raise UpdateError("run cancelled")
            process = subprocess.Popen(
                command,
                cwd=step["cwd"],
                env=env,
                stdin=subprocess.DEVNULL,
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
                text=True,
                errors="replace",
                start_new_session=True,
            )
            CHILDREN.add(process)
        try:
            output, _ = process.communicate(timeout=timeout)
            code = process.returncode
        except BaseException:
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            output, _ = process.communicate()
            if output:
                print(output, file=sys.stderr, end="")
            raise
        finally:
            with CHILDREN_LOCK:
                CHILDREN.discard(process)
    return code, output, time.monotonic() - started


def finish(step, result):
    code, output, elapsed = result
    label = step.get("label", step["id"])
    if code in step.get("skip_codes", []):
        if step.get("summary", True):
            runtime.stage_unchanged(label)
        return False
    if code in step.get("stop_codes", []):
        if output:
            print(output, end="" if output.endswith("\n") else "\n")
        runtime.stage_label("WARN", "!", label, "workflow stopped; follow-up required")
        return True
    if code:
        if output:
            print(output, file=sys.stderr, end="" if output.endswith("\n") else "\n")
        raise UpdateError(f"{label}: command failed ({code})")
    if step.get("summary", True):
        runtime.stage_result(step.get("verb", "RUN"), label, f"{elapsed:.2f}s")
    return False


def show_headings(item):
    if "banner" in item:
        runtime.repo_header(item["banner"]["title"], item["banner"]["url"])
    if "section" in item:
        runtime.section_header(item["section"])


def run_workflow(config, *, dry_run=False, flags=None):
    CANCELLED.clear()
    flags = flags or {}
    # Make the tool's Python helpers available to opt-in scripts in either a source
    # checkout or a self-contained zipapp; shell scripts need only the env protocol.
    module_path = str(Path(__file__).resolve().parent.parent)
    for group in config["steps"]:
        show_headings(group)
        parallel = "parallel" in group
        steps = group["parallel"] if parallel else [group]
        pending = []
        for step in steps:
            command = step.get("preview") if dry_run else step["command"]
            if command is None:
                if step.get("summary", True):
                    runtime.stage_skip(
                        step.get("label", step["id"]), "no preview configured"
                    )
                continue
            env = {
                **step["_env"],
                **flags,
                "TJIKUP_REPORT_FILE": str(ACTIVE_REPORT_FILE),
                "TJIKUP_COLUMNS": str(runtime.terminal_columns()),
                "TJIKUP_NO_SPIN": "1",
                "TJIKUP_COLOR": "1" if sys.stdout.isatty() else "0",
                "TJIKUP_DRY_RUN": "1" if dry_run else "0",
                "PYTHONUNBUFFERED": "1",
                "PYTHONDONTWRITEBYTECODE": "1",
            }
            env["PYTHONPATH"] = module_path + (
                os.pathsep + env["PYTHONPATH"] if env.get("PYTHONPATH") else ""
            )
            pending.append((step, command, env))
        stopped = False
        errors = []
        if parallel and pending:
            with ThreadPoolExecutor(max_workers=min(4, len(pending))) as pool:
                futures = [pool.submit(execute, *args) for args in pending]
                try:
                    for (step, _, _), future in zip(pending, futures):
                        try:
                            stopped = finish(step, future.result()) or stopped
                        except Exception as error:
                            runtime.stage_label(
                                "FAILED", "✗", step.get("label", step["id"]), str(error)
                            )
                            errors.append(error)
                except BaseException:
                    stop_children()
                    for future in futures:
                        future.cancel()
                    raise
        else:
            for step, command, env in pending:
                try:
                    stopped = finish(step, execute(step, command, env)) or stopped
                except Exception as error:
                    runtime.stage_label(
                        "FAILED", "✗", step.get("label", step["id"]), str(error)
                    )
                    errors.append(error)
        if errors:
            raise UpdateError(str(errors[0]))
        if stopped:
            return 0
    return 0


def main():
    global ACTIVE_REPORT_FILE
    parser = argparse.ArgumentParser(description=__doc__)
    default = (
        Path(os.environ.get("XDG_CONFIG_HOME", str(Path.home() / ".config")))
        / "tjikup/config.json"
    )
    parser.add_argument(
        "--config",
        default=os.environ.get("TJIKUP_CONFIG", str(default)),
        help="workflow JSON path",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="run only explicitly configured preview commands",
    )
    parser.add_argument(
        "--check",
        action="store_true",
        help="validate configuration without running commands",
    )
    # Help remains available before installation of a configuration.
    if any(arg in sys.argv[1:] for arg in ("-h", "--help")):
        try:
            index = sys.argv.index("--config") if "--config" in sys.argv else None
            config = load_config(
                sys.argv[index + 1]
                if index is not None
                else parser.get_default("config")
            )
        except (UpdateError, IndexError):
            config = {"flags": {}}
    else:
        probe = argparse.ArgumentParser(add_help=False)
        probe.add_argument("--config", default=parser.get_default("config"))
        known, _ = probe.parse_known_args()
        config = load_config(known.config)
    for name, flag in config.get("flags", {}).items():
        parser.add_argument("--" + name, action="store_true", help=flag["help"])
    args = parser.parse_args()
    if args.check:
        print("Configuration valid")
        return 0
    descriptor, filename = tempfile.mkstemp(prefix="tjikup-report-", suffix=".tsv")
    os.close(descriptor)
    ACTIVE_REPORT_FILE = Path(filename)
    os.environ["TJIKUP_REPORT_FILE"] = filename
    flags = {
        flag["env"]: "1" if getattr(args, name.replace("-", "_")) else "0"
        for name, flag in config.get("flags", {}).items()
    }
    runtime.splash()
    if "banner" in config:
        show_headings(config)
    else:
        runtime.section_header(config.get("title", "Workflow"))
    return run_workflow(config, dry_run=args.dry_run, flags=flags)


def cli():
    global ACTIVE_REPORT_FILE
    from .notifications import capture_focus_command, notify_completion

    ACTIVE_REPORT_FILE = None
    previous_report = os.environ.get("TJIKUP_REPORT_FILE")
    started = time.monotonic()
    focus_command = capture_focus_command()
    interrupted = False
    try:
        result = main()
    except KeyboardInterrupt:
        stop_children()
        interrupted = True
        result = 130
        if ACTIVE_REPORT_FILE:
            runtime.record("FAILED", "✗", "Tjikup", "interrupted")
        print("tjikup: interrupted", file=sys.stderr)
    except Exception as error:
        result = 1
        if ACTIVE_REPORT_FILE:
            with ACTIVE_REPORT_FILE.open("a") as report:
                report.write(
                    "FAILED\t✗\tTjikup\t" + " ".join(str(error).split()) + "\n"
                )
        print(f"tjikup: {error}", file=sys.stderr)
    # argparse's SystemExit bypasses completion notifications.
    try:
        if ACTIVE_REPORT_FILE:
            runtime.report_summary(
                ACTIVE_REPORT_FILE, dry_run="--dry-run" in sys.argv[1:]
            )
        if "--check" not in sys.argv[1:]:
            notify_completion(
                ACTIVE_REPORT_FILE,
                result,
                time.monotonic() - started,
                dry_run="--dry-run" in sys.argv[1:],
                interrupted=interrupted,
                focus_command=focus_command,
            )
    finally:
        if ACTIVE_REPORT_FILE:
            ACTIVE_REPORT_FILE.unlink(missing_ok=True)
        if previous_report is None:
            os.environ.pop("TJIKUP_REPORT_FILE", None)
        else:
            os.environ["TJIKUP_REPORT_FILE"] = previous_report
    return result
