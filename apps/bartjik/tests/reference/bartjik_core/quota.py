#!/usr/bin/env python

import json
import sqlite3
import time
from datetime import datetime
from pathlib import Path
from urllib.request import urlopen

DB_PATH = Path.home() / ".codex-lb" / "store.db"
WINDOWS = ("weekly",)
SUMMARY_LABELS = {
    "weekly": "󰃭",
}
# These are codex-lb's weekly (secondary-window) capacities.  Weighting the
# remaining percentages by capacity gives the combined remaining quota rather
# than treating an account with a larger plan as one ordinary account.
WEEKLY_CAPACITY_BY_PLAN = {
    "free": 1134.0,
    "plus": 7560.0,
    "business": 7560.0,
    "team": 7560.0,
    "edu": 7560.0,
    "pro": 50400.0,
    "prolite": 37800.0,
    "enterprise": 50400.0,
}
TOOLTIP_HEADERS = {
    "weekly_percent": "1w%",
    "weekly_time": "1w↻",
}
QUERY = """
WITH classified_usage AS (
  SELECT
    a.id AS account_id,
    COALESCE(NULLIF(a.alias, ''), a.email) AS label,
    LOWER(a.plan_type) AS plan_type,
    CASE
      WHEN uh.window_minutes = 300 THEN 'five_hour'
      WHEN uh.window_minutes = 10080 THEN 'weekly'
      -- Keep compatibility with older store rows without window_minutes.
      WHEN uh.window_minutes IS NULL AND uh.window = 'primary' THEN 'five_hour'
      WHEN uh.window_minutes IS NULL AND uh.window = 'secondary' THEN 'weekly'
    END AS window_name,
    100.0 - uh.used_percent AS remaining_percent,
    uh.reset_at,
    uh.recorded_at,
    uh.id
  FROM accounts a
  JOIN usage_history uh ON uh.account_id = a.id
  WHERE a.status = 'active'
), latest_usage AS (
  SELECT
    account_id,
    label,
    plan_type,
    remaining_percent,
    window_name,
    reset_at,
    ROW_NUMBER() OVER (
      PARTITION BY account_id, window_name
      ORDER BY recorded_at DESC, id DESC
    ) AS rn
  FROM classified_usage
  WHERE window_name IS NOT NULL
)
SELECT
  account_id,
  label,
  plan_type,
  window_name,
  remaining_percent,
  reset_at
FROM latest_usage
WHERE rn = 1 AND window_name = 'weekly'
ORDER BY label, window_name;
"""


def format_percent(value: float | None, digits: int = 0) -> str:
    if value is None:
        return "-"
    if digits == 0:
        return f"{round(value):.0f}%"
    return f"{value:.{digits}f}".rstrip("0").rstrip(".") + "%"


def format_time_until_reset(reset_at: int | None, now: int) -> str:
    if reset_at is None:
        return "-"

    seconds = max(0, int(reset_at) - now)
    minutes, _ = divmod(seconds, 60)
    days, minutes = divmod(minutes, 60 * 24)
    hours, minutes = divmod(minutes, 60)

    if days > 0:
        return f"{days}d{hours:02d}h"
    if hours > 0:
        return f"{hours}h{minutes}m"
    return f"{minutes}m"


def load_rows() -> list[sqlite3.Row]:
    connection = sqlite3.connect(
        "file:" + str(DB_PATH) + "?mode=ro", uri=True, timeout=2
    )
    connection.row_factory = sqlite3.Row
    try:
        return connection.execute(QUERY).fetchall()
    finally:
        connection.close()


def load_api_rows() -> list[dict]:
    # Laptop requests reach the desktop through the localhost SSH relay.
    with urlopen("http://127.0.0.1:2455/api/accounts", timeout=5) as response:
        accounts = json.load(response)["accounts"]
    rows = []
    for account in accounts:
        if account["status"] != "active":
            continue
        if account.get("windowMinutesSecondary") not in (None, 10080):
            continue
        reset = account.get("resetAtSecondary")
        rows.append(
            {
                "label": account.get("alias") or account["email"],
                "plan_type": account["planType"].lower(),
                "window_name": "weekly",
                "remaining_percent": (account.get("usage") or {}).get(
                    "secondaryRemainingPercent"
                ),
                "reset_at": int(
                    datetime.fromisoformat(reset.replace("Z", "+00:00")).timestamp()
                )
                if reset
                else None,
            }
        )
    return rows


def build_payload(rows: list[sqlite3.Row]) -> dict[str, str]:
    accounts: dict[str, dict[str, object]] = {}
    for row in rows:
        account = accounts.setdefault(
            row["label"],
            {
                "weekly": None,
                "weekly_reset_at": None,
                "plan_type": row["plan_type"],
            },
        )
        window_name = row["window_name"]
        account[window_name] = row["remaining_percent"]
        account[f"{window_name}_reset_at"] = row["reset_at"]

    averages = {}
    for window_name in WINDOWS:
        weighted_values = []
        for account in accounts.values():
            value = account[window_name]
            if value is None:
                continue
            plan_type = str(account["plan_type"] or "").casefold()
            weight = WEEKLY_CAPACITY_BY_PLAN.get(plan_type, 1.0)
            weighted_values.append((value, weight))
        total_weight = sum(weight for _, weight in weighted_values)
        averages[window_name] = (
            sum(value * weight for value, weight in weighted_values) / total_weight
            if total_weight
            else None
        )

    text = " ".join(
        f"{SUMMARY_LABELS[window_name]} {format_percent(averages[window_name])}"
        for window_name in WINDOWS
    )

    now = int(time.time())
    label_width = max((len(label) for label in accounts), default=5)
    header = "  ".join(
        [
            "acct".ljust(label_width),
            TOOLTIP_HEADERS["weekly_percent"].rjust(5),
            TOOLTIP_HEADERS["weekly_time"].rjust(4),
        ]
    )
    lines = [header]

    for label, values in sorted(accounts.items()):
        lines.append(
            "  ".join(
                [
                    label.ljust(label_width),
                    format_percent(values["weekly"], 1).rjust(5),
                    format_time_until_reset(values["weekly_reset_at"], now).rjust(4),
                ]
            )
        )

    return {
        "text": text,
        "tooltip": "\n".join(lines) if len(lines) > 1 else "No active Codex accounts",
        "alt": "codex",
        "class": "custom-codex",
    }
