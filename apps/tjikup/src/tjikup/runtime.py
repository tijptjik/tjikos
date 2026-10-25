"""Presentation and bounded subprocess execution, independent of workflow policy."""

from __future__ import annotations
import codecs
import os
from pathlib import Path
import re
import selectors
import shutil
import signal
import subprocess
import sys
import time
import unicodedata

COMMAND_TIMEOUT_SECONDS = 300
ACTIVE_STAGE = False


class UpdateError(RuntimeError):
    pass


def terminal_columns() -> int:
    """Carry terminal geometry through piped apply commands."""
    try:
        return max(24, int(os.environ.get("TJIKUP_COLUMNS", "")))
    except ValueError:
        return max(24, shutil.get_terminal_size((80, 24)).columns)


def display_width(value: str) -> int:
    return sum(
        0
        if unicodedata.combining(c)
        else 2
        if unicodedata.east_asian_width(c) in {"W", "F"}
        else 1
        for c in value
    )


def shorten_status(value: str, width: int) -> str:
    value = " ".join(value.splitlines())
    if display_width(value) <= width:
        return value
    result = ""
    for char in value:
        if display_width(result + char) > width - 1:
            break
        result += char
    return result + "…"


def run(
    command: list[str],
    cwd: Path | None = None,
    *,
    capture_output: bool = False,
    env: dict[str, str] | None = None,
) -> None:
    try:
        result = subprocess.run(
            command,
            cwd=cwd,
            env=env,
            text=True,
            capture_output=capture_output,
            check=False,
            timeout=COMMAND_TIMEOUT_SECONDS,
        )
    except subprocess.TimeoutExpired as error:
        raise UpdateError(
            f"command timed out after {COMMAND_TIMEOUT_SECONDS}s: {' '.join(command)}"
        ) from error
    except OSError as error:
        raise UpdateError(f"could not run {' '.join(command)}: {error}") from error
    if result.returncode:
        if capture_output:
            output = (result.stdout or "") + (result.stderr or "")
            if output:
                print(output, file=sys.stderr, end="")
        raise UpdateError(f"command failed ({result.returncode}): {' '.join(command)}")


def run_stream(
    command: list[str],
    cwd: Path,
    *,
    env: dict[str, str] | None = None,
    timeout: int = COMMAND_TIMEOUT_SECONDS,
    stdin=None,
    formatter=None,
    check: bool = True,
) -> int:
    stream_env = {
        **(os.environ if env is None else env),
        "TJIKUP_COLOR": (
            os.environ.get("TJIKUP_COLOR", "0")
            if os.environ.get("TJIKUP_CHILD") == "1"
            else "1"
            if sys.stdout.isatty()
            else "0"
        ),
        "TJIKUP_CHILD": "1",
        "TJIKUP_NO_SPIN": "1",
        "TJIKUP_COLUMNS": str(terminal_columns()),
    }
    try:
        # Gum 2 probes the terminal even when its spinner cannot read the
        # replies (for example, with skip-conflicts feeding stdin). Give
        # apply scripts a pipe so their helpers avoid the spinner. Preserve
        # status colours via TJIKUP_COLOR when forwarding to a terminal.
        # Keep stderr and stdin attached for prompts, and forward stdout live.
        with subprocess.Popen(
            command,
            cwd=cwd,
            env=stream_env,
            stdin=stdin,
            stdout=subprocess.PIPE,
            start_new_session=os.environ.get("TJIKUP_CHILD") != "1",
        ) as process:
            deadline = time.monotonic() + timeout
            decoder = codecs.getincrementaldecoder(sys.stdout.encoding or "utf-8")(
                "replace"
            )
            pending = ""

            def forward_output(value: str, *, final: bool = False) -> None:
                nonlocal pending
                pending += value
                lines = pending.splitlines(keepends=True)
                pending = ""
                if lines and not lines[-1].endswith(("\n", "\r")) and not final:
                    pending = lines.pop()
                for line in lines:
                    ending = ""
                    if line.endswith("\r\n"):
                        line, ending = line[:-2], "\r\n"
                    elif line.endswith(("\n", "\r")):
                        line, ending = line[:-1], line[-1:]
                    sys.stdout.write((formatter(line) if formatter else line) + ending)
                sys.stdout.flush()

            try:
                with selectors.DefaultSelector() as selector:
                    selector.register(process.stdout, selectors.EVENT_READ)
                    while True:
                        remaining = deadline - time.monotonic()
                        if remaining <= 0 or not selector.select(remaining):
                            raise subprocess.TimeoutExpired(command, timeout)
                        chunk = process.stdout.read1(65536)
                        forward_output(
                            decoder.decode(chunk, final=not chunk), final=not chunk
                        )
                        if not chunk:
                            break
                returncode = process.wait(timeout=max(0, deadline - time.monotonic()))
            except BaseException:
                try:
                    if os.environ.get("TJIKUP_CHILD") == "1":
                        process.kill()
                    else:
                        os.killpg(process.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                process.wait()
                raise
    except subprocess.TimeoutExpired as error:
        raise UpdateError(
            f"command timed out after {timeout}s: {' '.join(command)}"
        ) from error
    except OSError as error:
        raise UpdateError(f"could not run {' '.join(command)}: {error}") from error
    if returncode and check:
        raise UpdateError(f"command failed ({returncode}): {' '.join(command)}")
    return returncode


def use_color():
    return os.environ.get("TJIKUP_COLOR", "1" if sys.stdout.isatty() else "0") == "1"


def record(verb, icon, subject, note=""):
    filename = os.environ.get("TJIKUP_REPORT_FILE")
    if filename:
        try:
            fields = [
                " ".join(str(field).split())
                for field in (verb, icon, subject, note or "")
            ]
            with open(filename, "a") as report:
                report.write("\t".join(fields) + "\n")
        except OSError:
            pass


def status_msg(verb, icon, subject, note=None):
    record(verb, icon, subject, note)
    prefix = f"{verb:<7} {icon} "
    available = max(1, terminal_columns() - 1 - display_width(prefix))
    if note:
        note = shorten_status(str(note).strip(), max(1, (available - 2) // 2))
        subject = shorten_status(subject, max(1, available - display_width(note) - 2))
        padding = max(2, available - display_width(subject) - display_width(note))
    else:
        subject = shorten_status(subject, available)
    if use_color():
        color = "90" if verb == "SKIP" else "31;1" if icon in {"✗", "X"} else "36;1"
        icon_color = "32" if icon == "✓" else "33" if icon == "!" else color
        prefix = f"\033[{color}m{verb:<7}\033[0m \033[{icon_color}m{icon}\033[0m "
        # Match the Fish presenter: qualifiers and right-hand metadata recede.
        # Apply styling after measuring so ANSI escapes never affect alignment.
        subject = re.sub(
            r"(\[[^]]+\]|\([^)]*\))$",
            lambda match: f"\033[90m{match[0]}\033[0m",
            subject,
        )
        if note:
            note = f"\033[90m{note}\033[0m"
    content = subject + (" " * padding + note if note else "")
    print(prefix + content, flush=True)


def stage_label(verb, icon, subject, note=None):
    global ACTIVE_STAGE
    if ACTIVE_STAGE:
        sys.stdout.write("\r\033[2K")
    ACTIVE_STAGE = False
    status_msg(verb, icon, subject, note)


def stage_started(verb, subject):
    global ACTIVE_STAGE
    if sys.stdout.isatty():
        if ACTIVE_STAGE:
            sys.stdout.write("\r\033[2K")
        sys.stdout.write(
            f"{verb:<7} … " + shorten_status(subject, terminal_columns() - 11)
        )
        sys.stdout.flush()
        ACTIVE_STAGE = True


def stage_result(verb, subject, note=None):
    stage_label(verb, "✓", subject, note)


def stage_unchanged(subject, note="no changes"):
    stage_label("SKIP", "✓", subject, note)


def stage_expected(subject, note):
    stage_unchanged(subject, note)


def stage_skip(subject, note=None):
    stage_label("SKIP", "-", subject, note)


def section_header(title, *, color="12"):
    heading = f"\033[94;1m{title}\033[0m" if use_color() else title
    print(f"\n{heading}\n", flush=True)


def repo_header(title, url):
    if use_color():
        title = f"\033[95;1m{title}\033[0m"
        url = f"\033[90m{url}\033[0m"
    print(f"\n{title}\n{url}", flush=True)


def report_summary(report_file, *, dry_run=False):
    issues = []
    try:
        for line in report_file.read_text().splitlines():
            fields = tuple(line.split("\t", 3))
            if (
                len(fields) == 4
                and fields[1] in {"!", "✗", "X"}
                and fields not in issues
            ):
                issues.append(fields)
    except OSError:
        print("\nReport unavailable")
        return
    message = "NEEDS ATTENTION" if issues else "ALL SYSTEMS GO"
    padding = " " * max(0, (terminal_columns() - display_width(message)) // 2)
    if use_color():
        if issues:
            message = f"\033[31;1m{message}\033[0m"
        else:
            # Preserve the original Fish banner's six-colour wave palette.
            colors = ("255;95;95", "255;175;0", "95;255;135",
                      "95;215;255", "95;135;255", "215;135;255")
            message = "".join(
                character if character == " " else
                f"\033[1;38;2;{colors[index % len(colors)]}m{character}\033[0m"
                for index, character in enumerate(message)
            )
    print(f"\n{padding}{message}", flush=True)
    if issues:
        section_header("Preview issues" if dry_run else "Issues")
        # Do not append the summary to its own report.
        for verb, icon, subject, note in issues:
            print(f"{verb:<7} {icon} {subject}" + (f"  {note}" if note else ""))


SPLASH = """
                        _   _        ____  ____  ____
                     __| | / |  ____|_  _||   _||_   |
                    |__  || |__|____|/  \\ |  |_   || |
                       |_| \\___|    |_/\\_||____|  \\__/
"""


def splash() -> None:
    """Print Tjikup's startup splash once, before repository output begins."""
    if not sys.stdout.isatty():
        return
    colors = (196, 208, 226, 46, 39, 129)
    color_index = 0
    for character in SPLASH:
        if character in {" ", "\n"}:
            sys.stdout.write(character)
            continue
        sys.stdout.write(f"\033[1;38;5;{colors[color_index]}m{character}")
        color_index = (color_index + 1) % len(colors)
    sys.stdout.write("\033[0m\n")
