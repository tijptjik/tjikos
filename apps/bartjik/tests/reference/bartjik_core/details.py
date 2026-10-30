"""Typed panel data shared by live providers and synthetic fixtures."""

import configparser
import datetime as dt
import os
import time
from pathlib import Path
from zoneinfo import ZoneInfo
from . import quota

CITIES = [
    {"label": "New York", "timezone": "America/New_York"},
    {"label": "London", "timezone": "Europe/London"},
    {"label": "Amsterdam", "timezone": "Europe/Amsterdam"},
    {"label": "Tokyo", "timezone": "Asia/Tokyo"},
]


def panel(title, rows, subtitle=""):
    return {"title": title, "subtitle": subtitle, "rows": rows}


def field(label, value, detail="", **extra):
    return dict(label=str(label), value=str(value), detail=str(detail), **extra)


def openrouter_panel(payload):
    daily = payload.get("dailyUsage")
    status = payload.get("activityStatus", "unavailable")
    rows = [
        field(date, f"{tokens:,}", id=date, tokens=tokens)
        for date, tokens in sorted((daily or {}).items(), reverse=True)[:30]
    ]
    note = (
        "Daily usage isn't available for this key."
        if status == "restricted"
        else "Daily usage is temporarily unavailable."
        if status != "available"
        else "No token activity in this reporting period."
        if not rows
        else "Last 30 completed days · UTC"
    )
    return dict(
        panel("OpenRouter", rows),
        balance=payload["text"].removeprefix("$"),
        balanceLabel="Key spending limit remaining" if payload.get("source") else "Credits remaining",
        activityStatus=status,
        activityNote=note,
    )


def world_clock(cities=None, now=None):
    now = now or dt.datetime.now(dt.timezone.utc)
    rows = []
    for city in CITIES if cities is None else cities:
        local = now.astimezone(ZoneInfo(city["timezone"]))
        rows.append(
            field(
                city["label"], local.strftime("%H:%M"), local.strftime("%a %d %b · %Z")
            )
        )
    return panel("World clock", rows, "Local times · daylight saving aware")


def input_methods(group, path=None):
    p = configparser.ConfigParser(interpolation=None)
    p.read(
        path
        or Path(os.environ.get("XDG_CONFIG_HOME", str(Path.home() / ".config")))
        / "fcitx5/profile"
    )
    prefix = next(
        (
            s
            for s in p
            if s.startswith("Groups/")
            and s.count("/") == 1
            and p[s].get("Name", "").strip('"') == group
        ),
        None,
    )
    if prefix is None:
        return []
    sections = sorted(
        (s for s in p if s.startswith(prefix + "/Items/")),
        key=lambda s: int(s.rsplit("/", 1)[1]),
    )
    return [p[s]["Name"].strip('"') for s in sections if p[s].get("Name")]


def request_summary(con):
    # Aggregate only: never fetch request IDs, prompts, account credentials or error messages.
    deadline = time.monotonic() + 1.5
    con.set_progress_handler(lambda: int(time.monotonic() > deadline), 10000)
    try:
        row = con.execute("""SELECT COUNT(*), SUM(COALESCE(input_tokens,0)),
            SUM(COALESCE(output_tokens,0)), SUM(COALESCE(cached_input_tokens,0)),
            SUM(COALESCE(cost_usd,0)), AVG(latency_ms),
            SUM(CASE WHEN status IN ('error','failed','failure') THEN 1 ELSE 0 END)
            FROM request_logs WHERE requested_at >= datetime('now','-24 hours')
            AND deleted_at IS NULL""").fetchone()
        n, inputs, outputs, cached, cost, latency, errors = [v or 0 for v in row]
        return [
            field("Requests · 24h", f"{n:,}", f"{errors:,} failed", id="requests", amount=n, failures=errors),
            field("Input / output tokens", f"{inputs:,} / {outputs:,}", id="tokens", input=inputs, output=outputs),
            field("Cached input", f"{cached:,}", id="cached", amount=cached),
            field(
                "Recorded cost · 24h",
                f"${cost:,.2f}",
                "Reported by codex-lb; not subscription billing",
                id="cost", amount=cost,
            ),
            field("Mean request latency", f"{latency / 1000:.1f}s", id="latency", seconds=latency / 1000),
        ]
    except Exception:
        return [
            field("Request totals", "Unavailable", "Database schema or query limit")
        ]
    finally:
        con.set_progress_handler(None, 0)


def codex_panel(rows, summary, remaining):
    result = [
        field("Weekly remaining", remaining, "Capacity-weighted across active accounts", id="weekly")
    ] + summary
    for row in rows:
        value = row["remaining_percent"]
        result.append(
            field(
                row["label"],
                quota.format_percent(value, 1),
                str(row["plan_type"])
                + " · resets in "
                + quota.format_time_until_reset(row["reset_at"], int(time.time())),
                progress=max(0, min(1, value / 100)) if value is not None else -1,
                group="account",
                plan=str(row["plan_type"] or ""),
                resetAt=row["reset_at"],
            )
        )
    return panel("Codex usage", result, "codex-lb · read-only · weekly quota")
