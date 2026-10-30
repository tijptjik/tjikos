"""Plain text typed items; no renderer-specific combined strings."""

import html
import re


def item(id, text=None, icon=None, state=None, tooltip="", kind="glyph", actions=None):
    result = dict(
        id=str(id),
        type="icon_text"
        if text is not None and icon is not None
        else "icon"
        if icon is not None
        else "text",
        state=state or {},
        tooltip=tooltip,
        visible=True,
        enabled=True,
    )
    if icon is not None:
        result["icon"] = dict(kind=kind, value=str(icon), style={}, actions={})
    if text is not None:
        result["text"] = dict(value=str(text), style={}, actions={})
    result["actions"] = actions or {}
    return result


def legacy(payload, id="value", icon=None):
    plain = lambda s: html.unescape(re.sub("<[^>]*>", "", str(s)))
    state = payload.get("class", [])
    state = [state] if isinstance(state, str) else state
    return [
        item(
            id,
            plain(payload.get("text", "")),
            icon,
            {str(s): True for s in state},
            plain(payload.get("tooltip", "")),
        )
    ]


def validate_items(items):
    if not isinstance(items, list) or len(items) > 256:
        raise ValueError("provider items must be an array of at most 256")
    seen = set()
    for i in items:
        if (
            not isinstance(i, dict)
            or not isinstance(i.get("id"), str)
            or not 0 < len(i["id"]) <= 256
            or i["id"] in seen
        ):
            raise ValueError("items need unique stable string IDs")
        seen.add(i["id"])
        if i.get("type") not in ("icon", "text", "icon_text"):
            raise ValueError("unknown item type")
        for part in ("icon", "text"):
            if i["type"] == part or i["type"] == "icon_text":
                p = i.get(part)
                if (
                    not isinstance(p, dict)
                    or not isinstance(p.get("value"), str)
                    or len(p["value"]) > 16384
                ):
                    raise ValueError("invalid item part")
                if part == "icon" and p.get("kind") not in (
                    "glyph",
                    "symbol",
                    "theme",
                    "image",
                    "native",
                ):
                    raise ValueError("invalid icon kind")
        if not isinstance(i.get("state", {}), dict):
            raise ValueError("state must be an object")
    return items


def untrusted_items(payload):
    items = (
        legacy(payload)
        if isinstance(payload, dict)
        and "text" in payload
        and not isinstance(payload["text"], dict)
        else payload
    )
    validate_items(items)
    # Process output never installs executable actions, even if supplied by an API.
    for i in items:
        i.pop("actions", None)
        for part in ("icon", "text"):
            if part in i:
                i[part].pop("actions", None)
    return items
