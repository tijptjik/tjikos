#!/usr/bin/env python3

"""OpenRouter credit and usage data, with a legacy plain-text presentation."""

import json
import math
import os
from collections import defaultdict
from typing import Any, Callable
from urllib.error import HTTPError, URLError
from urllib.parse import urlencode
from urllib.request import Request, urlopen


API_BASE = "https://openrouter.ai/api/v1"
TIMEOUT_SECONDS = 10
TOKEN_FIELDS = ("prompt_tokens", "completion_tokens", "reasoning_tokens")
FetchJSON = Callable[[str, dict[str, str] | None], dict[str, Any]]


class OpenRouterError(RuntimeError):
    """An expected error while querying OpenRouter."""


def fetch_json(path: str, params: dict[str, str] | None = None) -> dict[str, Any]:
    api_key = os.environ.get("OPENROUTER_API_KEY")
    if not api_key:
        raise OpenRouterError("OPENROUTER_API_KEY is not set")

    url = f"{API_BASE}{path}"
    if params:
        url = f"{url}?{urlencode(params)}"

    request = Request(
        url,
        headers={
            "Accept": "application/json",
            "Authorization": f"Bearer {api_key}",
        },
    )

    try:
        with urlopen(request, timeout=TIMEOUT_SECONDS) as response:
            payload = json.load(response)
    except HTTPError as error:
        raise OpenRouterError(f"HTTP {error.code}") from error
    except (URLError, TimeoutError, OSError, ValueError) as error:
        raise OpenRouterError(str(error)) from error

    if not isinstance(payload, dict):
        raise OpenRouterError("invalid response")
    if "error" in payload:
        error = payload["error"]
        message = (
            error.get("message", "API error")
            if isinstance(error, dict)
            else "API error"
        )
        raise OpenRouterError(str(message))
    return payload


def as_number(value: Any) -> float | None:
    if isinstance(value, (int, float)) and not isinstance(value, bool) and math.isfinite(value):
        return float(value)
    return None


def format_usd(value: float | None) -> str:
    if value is None:
        return "-"
    return f"${value:,.2f}"


def format_tokens(value: int) -> str:
    return f"{value:,}"


def response_data(payload: dict[str, Any]) -> dict[str, Any]:
    data = payload.get("data") if isinstance(payload, dict) else None
    if not isinstance(data, dict):
        raise OpenRouterError("invalid API response")
    return data


def get_remaining_credits(fetch: FetchJSON) -> tuple[float | None, str | None]:
    """Return account credits, falling back to a key-specific spending limit."""
    try:
        data = response_data(fetch("/credits", None))
        total_credits = as_number(data.get("total_credits"))
        total_usage = as_number(data.get("total_usage"))
        if total_credits is None or total_usage is None:
            raise OpenRouterError("invalid credits response")
        return max(0, total_credits - total_usage), None
    except OpenRouterError as credits_error:
        # /credits is restricted to management keys. A regular API key can
        # still expose its own configured spending limit through /key.
        try:
            data = response_data(fetch("/key", None))
            remaining = as_number(data.get("limit_remaining"))
            if remaining is not None:
                return max(0, remaining), "key limit"
        except OpenRouterError:
            pass
        raise OpenRouterError(
            f"credits unavailable ({credits_error})"
        ) from credits_error


def daily_token_usage(fetch: FetchJSON) -> dict[str, int]:
    payload = fetch("/activity", None)
    activity = payload.get("data") if isinstance(payload, dict) else None
    if not isinstance(activity, list):
        raise OpenRouterError("invalid activity response")

    totals: defaultdict[str, int] = defaultdict(int)
    for entry in activity:
        if not isinstance(entry, dict) or not isinstance(entry.get("date"), str):
            raise OpenRouterError("invalid activity response")
        if any(field in entry and entry[field] is not None and (as_number(entry[field]) is None or entry[field] < 0) for field in TOKEN_FIELDS):
            raise OpenRouterError("invalid activity response")
        tokens = sum(
            int(entry.get(field) or 0)
            for field in TOKEN_FIELDS
            if isinstance(entry.get(field), (int, float))
        )
        totals[entry["date"]] += tokens
    return dict(totals)


def build_payload(fetch: FetchJSON = fetch_json) -> dict[str, Any]:
    remaining, source = get_remaining_credits(fetch)
    daily = None
    activity_status = "available"
    try:
        daily = daily_token_usage(fetch)
        activity_lines = [
            "Token usage per UTC day (last 30 completed days):",
        ]
        if daily:
            activity_lines.extend(
                f"{date}  {format_tokens(tokens)} tokens"
                for date, tokens in sorted(daily.items(), reverse=True)
            )
        else:
            activity_lines.append("No token activity")
    except OpenRouterError as error:
        activity_status = "restricted" if str(error) in ("HTTP 401", "HTTP 403") else "unavailable"
        activity_lines = [f"Daily token usage unavailable ({error})"]

    credit_label = "OpenRouter credits remaining"
    if source:
        credit_label += f" ({source})"
    tooltip = "\n".join([f"{credit_label}: {format_usd(remaining)}", *activity_lines])
    return {
        "remaining": remaining,
        "source": source,
        "dailyUsage": daily,
        "activityStatus": activity_status,
        "text": format_usd(remaining),
        "tooltip": tooltip,
        "alt": "openrouter",
        "class": "custom-openrouter",
    }
