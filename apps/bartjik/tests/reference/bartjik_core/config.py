"""Validated configuration. Objects merge recursively; every array replaces."""

import copy
import hashlib
import json
import math
import os
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[3]
PROVIDERS = (
    "battery",
    "workspaces",
    "playerctl",
    "crypto",
    "icontray",
    "network",
    "mullvad",
    "tailscale",
    "audio",
    "datetime",
    "locale",
    "codex",
    "openrouter",
    "DND",
    "caffeine",
    "nightlight",
    "monitor-layout",
    "process",
)
NATIVE = {"playerctl", "icontray"}
EVENTS = (
    "click",
    "leftClick",
    "rightClick",
    "middleClick",
    "doubleClick",
    "hoverEnter",
    "hoverLeave",
    "scrollUp",
    "scrollDown",
)
INTERNAL = {
    "crypto.panel",
    "workspace.activate",
    "workspace.next",
    "workspace.previous",
    "player.toggle",
    "player.next",
    "player.previous",
    "player.shuffle",
    "player.quit",
    "player.artist",
    "player.playlist",
    "tray.activate",
    "tray.secondary",
    "tray.menu",
    "tray.up",
    "tray.down",
    "network.panel",
    "audio.panel",
    "battery.panel",
    "notifications.panel",
    "audio.route",
    "audio.up",
    "audio.down",
    "audio.mute",
    "datetime.toggle",
    "locale.cycle",
    "DND.toggle",
    "tailscale.toggle",
    "caffeine.toggle",
    "nightlight.toggle",
    "monitor.toggle",
}
BAR = dict(
    monitors="all",
    side="b",
    position="c",
    alignment="c",
    offsetX=0,
    offsetY=-16,
    padding=0,
    screenGutter="edgeOffset",
    overflow="clip",
    islandGap=10,
    stackGap=8,
    thickness=44,
    verticalWidth=160,
    enabled=True,
    visible=True,
    layer="top",
    reserve=False,
    autoHide=False,
    syncMonitors=True,
    activationEdge="side",
    activationDelay=0,
    keepVisibleDistance="64px",
    hideDelay=0,
    fadeDuration=180,
    revealSize=3,
    islands=[],
    perMonitor={},
)
DEFAULTS = dict(
    theme="monochrome",
    style={},
    layout={},
    perMonitor={},
    modules={"clock": {"provider": "datetime"}},
    bars=[dict(id="clock", islands=[dict(id="clock", modules=["clock"])])],
)


def merge(base, patch):
    result = copy.deepcopy(base)
    for k, v in patch.items():
        result[k] = (
            merge(result[k], v)
            if isinstance(v, dict) and isinstance(result.get(k), dict)
            else copy.deepcopy(v)
        )
    return result


def path():
    return Path(
        os.environ.get(
            "BARTJIK_CONFIG",
            str(
                Path(os.environ.get("XDG_CONFIG_HOME", str(Path.home() / ".config")))
                / "bartjik/config.json"
            ),
        )
    )


def obj(value, name):
    if not isinstance(value, dict):
        raise ValueError(f"{name} must be an object")


def number(value, name, low=0, high=10000):
    if (
        isinstance(value, bool)
        or not isinstance(value, (int, float))
        or not math.isfinite(value)
        or not low <= value <= high
    ):
        raise ValueError(f"{name} must be a finite number in [{low}, {high}]")


def dimension(value, name, signed=False):
    if isinstance(value, str):
        if not re.fullmatch(r"-?\d+(?:\.\d+)?(?:px|%)", value):
            raise ValueError(f"{name}: use pixels or a percentage")
        amount = float(value.removesuffix("px").removesuffix("%"))
        number(
            amount,
            name,
            -100000 if signed else 0,
            100 if value.endswith("%") else 100000,
        )
    else:
        number(value, name, -100000 if signed else 0, 100000)


def argv(value):
    if (
        not isinstance(value, list)
        or not 1 <= len(value) <= 64
        or any(not isinstance(v, str) or "\0" in v or len(v) > 8192 for v in value)
        or not value[0]
    ):
        raise ValueError(
            "command must be a nonempty argument array (no shell expansion)"
        )


def actions(value):
    obj(value, "actions")
    for event, action in value.items():
        if event not in EVENTS:
            raise ValueError(f"unknown event: {event}")
        if action is None:
            continue
        if isinstance(action, list):
            argv(action)
        elif (
            isinstance(action, dict)
            and set(action) <= {"command", "detach"}
            and isinstance(action.get("detach", False), bool)
        ):
            argv(action.get("command"))
        elif isinstance(action, str) and action in INTERNAL:
            pass
        else:
            raise ValueError(f"invalid action for {event}")


def style(value):
    obj(value, "style")
    for key in (
        "fontSize",
        "iconSize",
        "padding",
        "paddingX",
        "paddingY",
        "radius",
        "borderWidth",
        "iconTextGap",
        "itemGap",
        "moduleGap",
        "minItemSize",
        "maxTextWidth",
        "previewHeight",
    ):
        if key in value:
            number(value[key], key, 0, 1000)
    for key in (
        "font",
        "iconFont",
        "background",
        "surface",
        "border",
        "panelBorder",
        "text",
        "muted",
        "accent",
        "color",
    ):
        if key in value and not isinstance(value[key], str):
            raise ValueError(f"{key} must be text")
    for key in ("greyscaleIcons", "monochromeIcon", "animateIcons", "marquee"):
        if key in value and not isinstance(value[key], bool):
            raise ValueError(f"{key} must be boolean")
    if "iconSet" in value and value["iconSet"] not in ("proicons", "material"):
        raise ValueError("iconSet must be proicons or material")
    for state in value.get("states", {}).values():
        style(state)


def layout(value):
    obj(value, "layout")
    if "centre_width" in value:
        dimension(value["centre_width"], "centre_width")
    for side, patch in value.get("sides", {}).items():
        if side not in "trbl" or len(side) != 1:
            raise ValueError("layout.sides keys must be t/r/b/l")
        layout(patch)


def module(value):
    obj(value, "module")
    if value.get("provider") not in PROVIDERS:
        raise ValueError(
            "unknown provider; register a process adapter with provider: process"
        )
    for key, lo, hi in [
        ("interval", 0.1, 86400),
        ("timeout", 0.1, 60),
        ("maxOutput", 1024, 1048576),
        ("hoverDelay", 0, 10000),
        ("doubleClickInterval", 100, 1000),
    ]:
        if key in value:
            number(value[key], key, lo, hi)
    if value["provider"] == "icontray":
        overrides = value.get("iconOverrides", {})
        obj(overrides, "iconOverrides")
        for key, source in overrides.items():
            if not key or not isinstance(source, str) or not source:
                raise ValueError("iconOverrides requires nonempty app patterns and image paths")
            if source.startswith("builtin:"):
                if source[8:] not in ("googlechrome", "mullvad", "telegram", "whatsapp", "spotify", "chatgpt", "chatgpt-outline", "telegram-outline", "whatsapp-outline"):
                    raise ValueError("unknown builtin tray artwork")
            elif not source.startswith(("/", "file:/")):
                raise ValueError("tray artwork must be a builtin or absolute local image path")
    if value["provider"] == "process":
        argv(value.get("command"))
        if value.get("mode", "poll") not in ("poll", "stream"):
            raise ValueError("process mode must be poll or stream")
    if "actions" in value:
        actions(value["actions"])
    if "style" in value:
        style(value["style"])
    for part in ("icon", "text", "artist", "track"):
        obj(value.get(part, {}), part)
        if "actions" in value.get(part, {}):
            actions(value[part]["actions"])
        if "style" in value.get(part, {}):
            style(value[part]["style"])
    obj(value.get("items", {}), "items")
    for patch in value.get("items", {}).values():
        module(merge({"provider": value["provider"]}, patch))
    if value["provider"] == "workspaces":
        for key in ("special", "occupiedOnly", "offscreen"):
            if key in value and not isinstance(value[key], bool):
                raise ValueError(f"{key} must be boolean")
        labels = value.get("labels", {})
        obj(labels, "workspace labels")
        if len(labels) > 128 or any(not isinstance(k, str) or not k or not isinstance(v, str) or not v or len(v) > 64 for k, v in labels.items()):
            raise ValueError("workspace labels must map workspace names to nonempty text (at most 64 characters)")
        preview = value.get("preview", {})
        obj(preview, "workspace preview")
        if "enabled" in preview and not isinstance(preview["enabled"], bool):
            raise ValueError("preview.enabled must be boolean")
        for key, low, high in (
            ("delay", 0, 10000),
            ("closeDelay", 0, 5000),
            ("slideDuration", 0, 1000),
            ("width", 160, 800),
            ("refreshInterval", 33, 5000),
            ("maxWindows", 1, 32),
        ):
            if key in preview:
                number(preview[key], "preview." + key, low, high)
        if value.get("label", "name") not in ("name", "circles", "circles_name"):
            raise ValueError("workspace label must be name, circles or circles_name")
    if value["provider"] == "workspaces" and value.get("appearance", "plain") not in ("plain", "chips", "separators", "ring"):
        raise ValueError("workspace appearance must be plain, chips, separators or ring")
    if value["provider"] == "datetime":
        from zoneinfo import ZoneInfo

        if value.get("timezone"):
            ZoneInfo(value["timezone"])
        cities = value.get("worldClocks", [])
        if not isinstance(cities, list) or len(cities) > 24:
            raise ValueError("worldClocks must be an array of at most 24 cities")
        for city in cities:
            if (
                not isinstance(city, dict)
                or not isinstance(city.get("label"), str)
                or not isinstance(city.get("timezone"), str)
            ):
                raise ValueError("worldClocks requires label and timezone")
            ZoneInfo(city["timezone"])
    if (
        value.get("provider") == "mullvad"
        and "regionFlag" in value
        and not isinstance(value["regionFlag"], bool)
    ):
        raise ValueError("regionFlag must be boolean")
    if (
        value.get("provider") == "openrouter"
        and "currencySymbol" in value
        and not isinstance(value["currencySymbol"], str)
    ):
        raise ValueError("currencySymbol must be text")
    if value.get("provider") == "mullvad" and "quickCountries" in value:
        countries = value["quickCountries"]
        if not isinstance(countries, list) or len(countries) > 8 or any(not isinstance(c, str) or not re.fullmatch("[a-z]{2}", c) for c in countries) or len(countries) != len(set(countries)):
            raise ValueError("quickCountries must contain at most 8 unique two-letter country codes")
    if value.get("provider") in (
        "crypto",
        "playerctl",
        "datetime",
        "mullvad",
        "tailscale",
        "locale",
        "codex",
        "openrouter",
        "audio",
        "network",
    ):
        panel = value.get("panel", {})
        obj(panel, "module panel")
        if "hover" in panel and not isinstance(panel["hover"], bool):
            raise ValueError("panel.hover must be boolean")
        for key in ("delay", "closeDelay"):
            if key in panel:
                number(panel[key], "panel." + key, 0, 10000)

    if value.get("provider") == "crypto":
        for asset in value.get("assets", []):
            if not isinstance(asset, dict) or not re.fullmatch(
                "[A-Za-z0-9]+", asset.get("symbol", "")
            ):
                raise ValueError("crypto assets need an alphanumeric symbol")
            for key in ("price_precision", "change_precision", "volume_precision"):
                number(asset.get(key, 2), key, 0, 12)


def bar(value, modules):
    obj(value, "bar")
    if "layout" in value or "centre_width" in value:
        raise ValueError(
            "column geometry belongs to global/perMonitor layout, never an individual bar"
        )
    if (
        value.get("side") not in ("t", "r", "b", "l")
        or value.get("position") not in ("l", "c", "r")
        or value.get("alignment") not in ("l", "c", "r")
    ):
        raise ValueError("invalid side/position/alignment")
    selectors = value.get("monitors")
    if selectors != "all" and (
        not isinstance(selectors, list)
        or not selectors
        or any(
            isinstance(s, bool)
            or not isinstance(s, (str, int))
            or (isinstance(s, int) and s < 1)
            for s in selectors
        )
    ):
        raise ValueError("monitors must be all or connector names / 1-based indexes")
    if value["overflow"] not in ("clip", "centre"):
        raise ValueError("overflow must be clip or centre")
    if value["activationEdge"] not in ("side", "t", "r", "b", "l"):
        raise ValueError("invalid activationEdge")
    if value["screenGutter"] != "edgeOffset":
        dimension(value["screenGutter"], "screenGutter")
    dimension(value["keepVisibleDistance"], "keepVisibleDistance")
    for key in ("offsetX", "offsetY"):
        dimension(value[key], key, True)
    for key in (
        "padding",
        "islandGap",
        "stackGap",
        "thickness",
        "verticalWidth",
        "hideDelay",
        "activationDelay",
        "fadeDuration",
        "revealSize",
    ):
        number(value[key], key)
    for key in ("enabled", "visible", "reserve", "autoHide", "syncMonitors"):
        if not isinstance(value[key], bool):
            raise ValueError(f"{key} must be boolean")
    if value["layer"] not in ("background", "bottom", "top", "overlay"):
        raise ValueError("invalid layer")
    if "style" in value:
        style(value["style"])
    if not isinstance(value["islands"], list):
        raise ValueError("islands must be an array")
    ids = set()
    for island in value["islands"]:
        obj(island, "island")
        identifier(island.get("id"))
        if island["id"] in ids:
            raise ValueError("duplicate island ID")
        ids.add(island["id"])
        if not isinstance(island.get("modules"), list) or any(
            m not in modules for m in island["modules"]
        ):
            raise ValueError("island references unknown module")
        if len(set(island["modules"])) != len(island["modules"]):
            raise ValueError("duplicate module in island")
        if "style" in island:
            style(island["style"])


def identifier(value):
    if not isinstance(value, str) or not re.fullmatch("[A-Za-z0-9_.:-]{1,100}", value):
        raise ValueError(
            "IDs must contain 1–100 letters, digits, dot, colon, dash or underscore"
        )


def load(config_path=None):
    p = config_path or path()
    patch = json.loads(p.read_text()) if p.exists() else {}
    obj(patch, "config")
    cfg = merge(DEFAULTS, patch)
    # Supplying modules intentionally replaces the default module registry.
    if "modules" in patch:
        cfg["modules"] = patch["modules"]
    for key in ("style", "layout", "perMonitor", "modules"):
        obj(cfg[key], key)
    theme = Path(cfg["theme"])
    theme = (
        (ROOT / "themes" / f"{cfg['theme']}.json")
        if "/" not in str(theme) and not str(theme).endswith(".json")
        else (p.parent / theme).resolve()
    )
    cfg["_styleOverrides"] = copy.deepcopy(cfg["style"])
    cfg["style"] = merge(json.loads(theme.read_text()), cfg["style"])
    style(cfg["style"])
    layout(cfg["layout"])
    for mid, m in cfg["modules"].items():
        identifier(mid)
        module(m)
    if not isinstance(cfg["bars"], list) or len(cfg["bars"]) > 64:
        raise ValueError("bars must be an array of at most 64 bars")
    ids = set()
    for i, b in enumerate(cfg["bars"]):
        identifier(b.get("id"))
        if b["id"] in ids:
            raise ValueError("duplicate bar ID")
        ids.add(b["id"])
        cfg["bars"][i] = merge(BAR, b)
        bar(cfg["bars"][i], cfg["modules"])
        obj(b.get("perMonitor", {}), "bar.perMonitor")
        for override in b.get("perMonitor", {}).values():
            obj(override, "bar override")
            if "id" in override or "perMonitor" in override:
                raise ValueError("bar override cannot change identity")
            bar(
                merge(cfg["bars"][i], override),
                merge(cfg["modules"], override.get("modules", {})),
            )
    for override in cfg["perMonitor"].values():
        obj(override, "perMonitor")
        layout(override.get("layout", {}))
        style(override.get("style", {}))
        for mid, m in override.get("modules", {}).items():
            module(merge(cfg["modules"].get(mid, {}), m))
    # Validate every effective override, including numeric + connector combinations.
    for index, key in enumerate(cfg["perMonitor"]):
        resolve(
            cfg,
            {"name": key, "width": 100000, "height": 100000, "scale": 1},
            index,
            validate_only=True,
        )
    return cfg


def monitor_patch(patches, screen, index):
    return merge(patches.get(str(index + 1), {}), patches.get(screen["name"], {}))


def resolve(cfg, screen, index, validate_only=False):
    patch = monitor_patch(cfg["perMonitor"], screen, index)
    merged = merge(cfg, patch)
    if "theme" in patch:
        theme = Path(patch["theme"])
        theme = (
            ROOT / "themes" / f"{theme}.json"
            if "/" not in str(theme) and not str(theme).endswith(".json")
            else path().parent / theme
        )
        merged["style"] = merge(
            merge(json.loads(theme.read_text()), cfg.get("_styleOverrides", {})),
            patch.get("style", {}),
        )
    style(merged["style"])
    result = []
    stacks = {}
    for original in cfg["bars"]:
        selectors = original["monitors"]
        if (
            selectors != "all"
            and screen["name"] not in selectors
            and index + 1 not in selectors
        ):
            continue
        b = merge(original, monitor_patch(original["perMonitor"], screen, index))
        modules = merge(merged["modules"], b.get("modules", {}))
        for m in modules.values():
            module(m)
        bar(b, modules)
        if not b["enabled"] or patch.get("enabled") is False:
            continue
        lay = merge(
            merged["layout"], merged["layout"].get("sides", {}).get(b["side"], {})
        )
        span = screen["width"] if b["side"] in "tb" else screen["height"]
        if (
            not validate_only
            and "centre_width" in lay
            and pixels(lay["centre_width"], span, screen["scale"]) > span
        ):
            raise ValueError(f"centre_width exceeds {screen['name']} {b['side']} span")
        group = (b["side"], b["position"])
        depth = stacks.get(group, 0)
        b.update(
            key=screen["name"] + "/" + b["id"],
            screen=screen["name"],
            layout=lay,
            style=merge(merged["style"], b.get("style", {})),
            stackOffset=depth,
            moduleConfigs=modules,
        )
        stacks[group] = (
            depth
            + (b["thickness"] if b["side"] in "tb" else b["verticalWidth"])
            + b["stackGap"]
        )
        result.append(b)
    return result


def pixels(value, extent, scale):
    if isinstance(value, str) and value.endswith("%"):
        return float(value[:-1]) * extent / 100
    return float(str(value).removesuffix("px")) / scale


def provider_config(m):
    # Rendering/actions cannot create duplicate polling loops.
    return {
        k: v
        for k, v in m.items()
        if k
        not in (
            "style",
            "actions",
            "icon",
            "text",
            "artist",
            "track",
            "items",
            "hoverDelay",
            "doubleClickInterval",
        )
    }


def provider_key(m):
    return hashlib.sha256(
        json.dumps(provider_config(m), sort_keys=True, separators=(",", ":")).encode()
    ).hexdigest()[:20]
