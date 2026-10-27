"""Best-effort completion reports on the local or originating SSH desktop."""

from base64 import b64encode
from html import escape, unescape
import json
import os
from pathlib import Path
import re
import shutil
import socket
import subprocess
import sys
import uuid


def issue_details(records):
    """Prefer failures and expose only subjects plus structured dirty counts."""
    issues = [row for row in records if row[1] in {"!", "✗", "X"}]
    if any(row[1] in {"✗", "X"} and row[2] != "Tjikup" for row in issues):
        issues = [row for row in issues
                  if not (row[0] == "FAILED" and row[2] == "Tjikup")]
    details = []
    for stage, icon, subject, note in sorted(issues, key=lambda row: row[1] == "!"):
        subject = " ".join(subject.split())[:200]
        is_failure = icon in {"✗", "X"}
        # Accept reports from older fleet helpers as well as friendly labels.
        if subject.startswith("Dirty State "):
            name = Path(subject.removeprefix("Dirty State ")).name
            name = {"chezmoi": "Chezmoi", "chezetc": "Chezetc"}.get(name, name)
            subject = f"Dirty {name}"
        if subject.startswith("Dirty ") and not is_failure:
            detail = subject
            if re.fullmatch(r"\d+ files?", note):
                detail += f" — {note}"
        else:
            label = "Failed" if is_failure else "Warning"
            operation = "" if stage in {"WARN", "FAILED", "ERROR"} else f"{stage}: "
            detail = f"{label} · {operation}{subject}"
        if (is_failure, detail) not in details:
            details.append((is_failure, detail))
    return details


def completion_payload(
    report_file, result, seconds, *, dry_run=False, interrupted=False
):
    records = []
    unavailable = False
    try:
        if report_file is not None:
            for line in report_file.read_text().splitlines():
                fields = tuple(line.split("\t", 3))
                if len(fields) == 4 and fields not in records:
                    records.append(fields)
        else:
            unavailable = True
    except (OSError, UnicodeError):
        unavailable = True
    issues = [row for row in records if row[1] in {"!", "✗", "X"}]
    failed = result != 0 or any(row[1] in {"✗", "X"} for row in issues)
    if interrupted:
        state = "interrupted"
    elif failed:
        state = "failed"
    elif issues or unavailable:
        state = "needs attention"
    else:
        state = "complete"
    title = f"Tjikup{' preview' if dry_run else ''} {state}"
    minutes, seconds = divmod(max(0, int(seconds)), 60)
    duration = f"{minutes}m {seconds:02d}s" if minutes else f"{seconds}s"
    completed = len({(row[0], row[2]) for row in records if row[1] == "✓"})
    summaries = issue_details(records)
    details = [detail for _, detail in summaries]
    failure_count = sum(is_failure for is_failure, _ in summaries)
    warning_count = len(summaries) - failure_count
    if interrupted:
        status = "INTERRUPTED"
    elif details:
        status = "\n".join(details[:3])
        if len(details) > 3:
            status += f"\n… and {len(details) - 3} more; see terminal"
    elif failed:
        status = f"FAILED --- exit {result}; see terminal"
    elif unavailable:
        status = "Report unavailable; check the terminal"
    else:
        status = "ALL SYSTEMS GO"
    if failed and details and not interrupted:
        status = "FAILED\n" + status
    body = [status, f"{duration} · Stages completed: {completed} · Issues: {len(details)}"]
    if dry_run:
        body.append("Preview only; no changes applied.")
    if unavailable:
        body.append("Stage report unavailable; check the terminal.")
    if failed or issues:
        body.append("See terminal output for details.")
    data = {"state": status, "tone": "critical" if failed or interrupted else
            "warning" if issues or unavailable else "success"}
    # Status.qml is a compact row; stage/tool names belong in the wrapping body.
    native_body = details[:3]
    if len(details) > 3:
        native_body.append(f"+{len(details) - 3} more in terminal")
    if unavailable:
        native_body.append("Report unavailable; check terminal.")
    elif failed and not details and not interrupted:
        native_body.append(f"Exit {result}; check terminal.")
    if dry_run:
        native_body.append("Preview · no changes applied")
    counts = []
    if failure_count:
        counts.append(f"{failure_count} failed")
    if warning_count:
        counts.append(f"{warning_count} {'warning' if warning_count == 1 else 'warnings'}")
    data["state"] = ("INTERRUPTED" if interrupted else
                     "FAILED" if failed and not failure_count else
                     " · ".join(counts) if counts else
                     "REPORT UNAVAILABLE" if unavailable else "ALL SYSTEMS GO")
    data["detail"] = f"{duration} · {completed} {'stage' if completed == 1 else 'stages'}"
    if data["tone"] == "success":
        data["rainbow-text"] = True
    if not failed and not interrupted and len(details) == 1 and details[0].startswith("Dirty "):
        # Give a single dirty repository the status row; timing becomes the body.
        dirty = re.fullmatch(r"(Dirty .+?)(?: — (\d+ files?))?", details[0])
        native_body[0] = data["detail"]
        data["state"] = dirty.group(1)
        data["detail"] = dirty.group(2) or ""
    return (title, escape("\n".join(body)), "critical" if failed or interrupted else "normal",
            data, "\n".join(native_body))


def completion_message(*args, **kwargs):
    return completion_payload(*args, **kwargs)[:3]


def ancestor_pids():
    """Find the launching terminal without guessing from current keyboard focus."""
    pids = []
    pid = os.getpid()
    while pid > 1 and pid not in pids:
        pids.append(pid)
        try:
            # comm may contain spaces and parentheses; ppid follows the last ).
            stat = Path(f"/proc/{pid}/stat").read_text().rsplit(")", 1)[1].split()
            pid = int(stat[1])
        except (OSError, ValueError, IndexError):
            break
    return pids


def capture_focus_command():
    if (os.environ.get("TJIKUP_NOTIFY") == "0" or
            os.environ.get("SSH_CONNECTION") or os.environ.get("SSH_TTY") or
            not os.environ.get("HYPRLAND_INSTANCE_SIGNATURE")):
        return None
    hyprctl = shutil.which("hyprctl")
    if not hyprctl:
        return None
    try:
        clients = json.loads(subprocess.run(
            [hyprctl, "-j", "clients"], capture_output=True, text=True,
            check=True, timeout=1,
        ).stdout)
        for pid in ancestor_pids():
            matches = [client for client in clients if client.get("pid") == pid]
            # A process can own several windows. Do not focus an arbitrary one.
            if len(matches) == 1:
                address = matches[0].get("address", "")
                if re.fullmatch(r"0x[0-9a-fA-F]+", address):
                    # Lua-configured Hyprland rejects the legacy dispatcher.
                    # Probe without moving focus; older releases keep their API.
                    probe = subprocess.run(
                        [hyprctl, "eval", "assert(hl.dsp.focus)"],
                        capture_output=True, text=True, check=False, timeout=1,
                    )
                    if probe.returncode == 0 and probe.stdout.strip() == "ok":
                        return [hyprctl, "dispatch",
                                f'hl.dsp.focus({{ window = "address:{address}" }})']
                    return [hyprctl, "dispatch", "focuswindow", f"address:{address}"]
    except (OSError, subprocess.SubprocessError, ValueError, TypeError, AttributeError):
        pass
    return None


def ssh_notification_filter(command):
    """Kitty desktop hook: render remote status metadata through local Notjik.

    Only display fields cross SSH. Actions are derived on this desktop, never
    accepted from the remote notification. False retains Kitty's normal delivery.
    """
    if command.application_name != "Tjikup":
        return False
    prefix = "tjikup-status-v1:"
    try:
        metadata = next(value for value in command.notification_types
                        if value.startswith(prefix))
        if len(metadata) > 16384:
            return False
        payload = json.loads(metadata[len(prefix):])
        data, body = payload["data"], payload["body"]
        if (set(payload) != {"data", "body"} or
                set(data) not in ({"state", "detail", "tone"},
                                  {"state", "detail", "tone", "rainbow-text"}) or
                data["tone"] not in {"success", "warning", "critical"} or
                ("rainbow-text" in data and not isinstance(data["rainbow-text"], bool)) or
                not all(isinstance(value, str) and len(value) <= 8192
                        for value in (body, data["state"], data["detail"]))):
            return False
        return send_native_notification(
            command.title, body, "critical" if data["tone"] == "critical" else "normal",
            data, capture_focus_command(),
        )
    except (StopIteration, KeyError, TypeError, ValueError, AttributeError):
        return False


def send_native_notification(title, body, urgency, data, focus_command=None):
    native = shutil.which("notjikctl")
    if not native:
        return False
    icon = "tjikup-notification"
    actions = [{"label": "Show window", "command": focus_command}] if focus_command else []
    try:
        sent = subprocess.run(
            [native, "send", title, body, "--app", "Tjikup",
             "--icon", str(icon), "--type", "status", "--data", json.dumps(data),
             "--actions", json.dumps(actions), "--urgency", urgency,
             "--timeout", "10s" if data["tone"] == "success" else "until-dismissed"],
            stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL, check=False, timeout=3,
        )
        return sent.returncode == 0
    except (OSError, subprocess.TimeoutExpired):
        return False


def notify_ssh_terminal(title, body, urgency, data=None, native_body=""):
    """Return one OSC 99 notification through a direct Kitty SSH PTY."""
    if os.environ.get("TERM") != "xterm-kitty" or not sys.stdout.isatty():
        return

    def encoded(value):
        return b64encode(value.encode("utf-8")).decode("ascii")

    host = socket.gethostname().split(".")[0]
    title = title.replace("Tjikup", f"Tjikup · {host}", 1)
    identifier = f"tjikup-{uuid.uuid4().hex}"
    # The icon name resolves on the computer hosting Kitty, not the SSH host.
    # Disable protocol replies: they must never appear at the shell prompt.
    metadata = (
        f"i={identifier}:e=1:a=-report:f={encoded('Tjikup')}:n={encoded('tjikup-notification')}"
        f":u={2 if urgency == 'critical' else 1}:w=10000:o=always"
    )
    if data is not None:
        payload = "tjikup-status-v1:" + json.dumps({"data": data, "body": native_body})
        metadata += f":t={encoded(payload)}"
    # Kitty treats bodies as plain text; notify-send expects escaped markup.
    message = (
        f"\x1b]99;{metadata}:d=0:p=title;{encoded(title)}\x1b\\"
        f"\x1b]99;i={identifier}:e=1:d=0:p=body;{encoded(unescape(body))}\x1b\\"
        f"\x1b]99;i={identifier}:e=1:d=1:p=buttons;{encoded('Focus')}\x1b\\"
    )
    sys.stdout.write(message)
    sys.stdout.flush()


def notify_completion(
    report_file, result, seconds, *, dry_run=False, interrupted=False, focus_command=None
):
    if os.environ.get("TJIKUP_NOTIFY") == "0":
        return
    if os.environ.get("SSH_CONNECTION") or os.environ.get("SSH_TTY"):
        try:
            notify_ssh_terminal(
                *completion_payload(
                    report_file,
                    result,
                    seconds,
                    dry_run=dry_run,
                    interrupted=interrupted,
                )
            )
        except (OSError, ValueError):
            pass
        return
    if not (os.environ.get("WAYLAND_DISPLAY") or os.environ.get("DISPLAY")):
        return
    try:
        title, body, urgency, data, native_body = completion_payload(
            report_file,
            result,
            seconds,
            dry_run=dry_run,
            interrupted=interrupted,
        )
        icon = "tjikup-notification"
        if send_native_notification(title, native_body, urgency, data, focus_command):
            return
        sender = shutil.which("notify-send")
        if sender is None:
            return
        subprocess.run(
            [
                sender,
                "--app-name=Tjikup",
                f"--icon={icon}",
                f"--urgency={urgency}",
                "--expire-time=10000",
                title,
                body,
            ],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            check=False,
            timeout=3,
        )
    except (OSError, subprocess.TimeoutExpired):
        pass
