"""Provider registry. Async I/O never blocks QML. Native MPRIS/tray live in QML."""

import asyncio
import configparser
import datetime
import os
from pathlib import Path
from urllib.parse import urlencode
from .io import command, json_command, http_json, Unavailable, RateLimit
from .model import item, untrusted_items
from . import quota, credits, details, connectivity
from .icons import icon
from .audio_output import is_headphones

COUNTRY_FLAGS = {
    "Albania": "al",
    "Australia": "au",
    "Austria": "at",
    "Belgium": "be",
    "Brazil": "br",
    "Bulgaria": "bg",
    "Canada": "ca",
    "Colombia": "co",
    "Croatia": "hr",
    "Czech Republic": "cz",
    "Denmark": "dk",
    "Estonia": "ee",
    "Finland": "fi",
    "France": "fr",
    "Germany": "de",
    "Greece": "gr",
    "Hong Kong": "hk",
    "Hungary": "hu",
    "Ireland": "ie",
    "Israel": "il",
    "Italy": "it",
    "Japan": "jp",
    "Latvia": "lv",
    "Luxembourg": "lu",
    "Moldova": "md",
    "Netherlands": "nl",
    "New Zealand": "nz",
    "North Macedonia": "mk",
    "Norway": "no",
    "Poland": "pl",
    "Portugal": "pt",
    "Romania": "ro",
    "Serbia": "rs",
    "Singapore": "sg",
    "Slovakia": "sk",
    "South Africa": "za",
    "Spain": "es",
    "Sweden": "se",
    "Switzerland": "ch",
    "UK": "gb",
    "United Kingdom": "gb",
    "United States": "us",
    "Taiwan": "tw",
    "USA": "us",
    "United Arab Emirates": "ae"
}

INTERVALS = dict(
    crypto=600,
    network=3,
    mullvad=10,
    tailscale=5,
    audio=1,
    datetime=1,
    locale=1,
    codex=30,
    openrouter=600,
    DND=1,
    caffeine=2,
    nightlight=2,
    **{"monitor-layout": 2},
)
METHODS = {
    "keyboard-us-altgr-intl": ("EN", "English — US International (AltGr accents)"),
    "keyboard-us-intl": ("EN", "English — US International (dead keys)"),
    "keyboard-us": ("EN", "English — US"),
    "keyboard-gb": ("EN", "English — UK"),
    "pinyin": ("拼", "Pinyin — Traditional Chinese"),
    "quick-classic": ("速", "Quick (速成) — Traditional Chinese"),
    "quick3": ("速", "Quick 3 (速成) — Traditional Chinese"),
    "cantonese": ("粵", "Cantonese"),
}
ASSETS = [
    dict(symbol="BTC", price_precision=0),
    dict(symbol="SOL", price_precision=0),
    dict(symbol="AR", price_precision=1),
]


def cmc_config(m):
    # Temporary compatibility: only read this optional external credential/config file.
    # No credential is copied to effective config or emitted to QML.
    c = configparser.ConfigParser(interpolation=None)
    if m.get("legacyConfig"):
        c.read(Path(m["legacyConfig"]).expanduser())
    general = c["general"] if c.has_section("general") else {}
    assets = m.get("assets")
    if assets is None and c.sections():
        assets = [
            dict(
                symbol=s.upper(),
                icon=c[s].get("icon", s.upper()),
                in_tooltip=c[s].getboolean("in_tooltip", False),
                **{
                    k: c[s].getint(k, 2)
                    for k in ("price_precision", "change_precision", "volume_precision")
                },
            )
            for s in c.sections()
            if s != "general"
        ]
    return dict(
        assets=assets or ASSETS,
        currency=m.get("currency", general.get("currency", "USD")).upper(),
        currencySymbol=m.get("currencySymbol", general.get("currency_symbol", "")),
        display=m.get("display", general.get("display", "price").split(",")),
        key=os.environ.get(m.get("keyEnv", "COINMARKETCAP_API_KEY"))
        or general.get("api_key"),
    )


def crypto_items(data, c):
    if data.get("status", {}).get("error_code"):
        raise Unavailable("CoinMarketCap rejected request")
    result = []
    extra = []
    for a in c["assets"]:
        symbol = a["symbol"].upper()
        q = data["data"][symbol]["quote"][c["currency"]]
        fields = []
        for field in c["display"]:
            precision = a.get(
                "volume_precision"
                if "volume" in field
                else "change_precision"
                if "change" in field
                else "price_precision",
                2,
            )
            val = f"{float(q[field]):.{precision}f}"
            fields.append(
                c["currencySymbol"] + val
                if field == "price"
                else field.replace("percent_change_", "")
                + ":"
                + val
                + ("%" if "change" in field else "")
            )
        label = " ".join(fields)
        tooltip = (
            f"{symbol}/{c['currency']}\n24h: {q.get('percent_change_24h', 0):.2f}%"
        )
        if a.get("in_tooltip"):
            extra.append(symbol + " " + label)
            continue
        result.append(
            item(
                symbol + "/" + c["currency"],
                label,
                a.get(
                    "icon",
                    icon(
                        {"BTC": "bitcoin", "SOL": "solana", "AR": "arweave"}.get(
                            symbol, "coin"
                        )
                    ),
                ),
                kind="glyph" if "icon" in a else "image",
                tooltip=tooltip,
                actions={"leftClick": "crypto.panel"},
            )
        )
        result[-1]["changes"] = {
            k: q.get("percent_change_" + k) for k in ("1h", "24h", "7d")
        }
    if extra:
        for i in result:
            i["tooltip"] += "\n" + "\n".join(extra)
    return result


async def crypto(m):
    c = cmc_config(m)
    if not c["key"]:
        raise Unavailable("COINMARKETCAP_API_KEY is not set")
    url = (
        "https://pro-api.coinmarketcap.com/v1/cryptocurrency/quotes/latest?"
        + urlencode(
            dict(
                symbol=",".join(a["symbol"] for a in c["assets"]), convert=c["currency"]
            )
        )
    )
    data = await asyncio.to_thread(
        http_json, url, {"X-CMC_PRO_API_KEY": c["key"], "Accept": "application/json"}
    )
    return crypto_items(data, c)


def nm_fields(line):
    fields = []
    current = ""
    escaped = False
    for ch in line:
        if escaped:
            current += ch
            escaped = False
        elif ch == "\\":
            escaped = True
        elif ch == ":":
            fields.append(current)
            current = ""
        else:
            current += ch
    fields.append(current)
    return fields


async def network(m):
    rows = [
        nm_fields(l)
        for l in (
            await command(
                [
                    "nmcli",
                    "-t",
                    "-f",
                    "DEVICE,TYPE,STATE,CONNECTION",
                    "device",
                    "status",
                ]
            )
        ).splitlines()
    ]
    rows = [
        r
        for r in rows
        if len(r) == 4
        and (not m.get("interface") or r[0] == m["interface"])
        and r[1] in ("wifi", "ethernet")
    ]
    if not rows:
        raise Unavailable("No matching network interface")
    selected = next((r for r in rows if r[2].startswith("connected")), rows[0])
    interface, kind, state, connection = selected
    connected = state.startswith("connected")
    signal = None
    ssid = connection
    if connected and kind == "wifi":
        try:
            wifi = [
                nm_fields(l)
                for l in (
                    await command(
                        [
                            "nmcli",
                            "-t",
                            "-f",
                            "ACTIVE,SSID,SIGNAL",
                            "device",
                            "wifi",
                            "list",
                            "ifname",
                            interface,
                            "--rescan",
                            "no",
                        ]
                    )
                ).splitlines()
            ]
            active = next((r for r in wifi if len(r) == 3 and r[0] == "yes"), None)
            if active:
                ssid = active[1]
                signal = active[2]
        except Unavailable:
            pass  # A telemetry failure is not a disconnect.
    label = (
        ssid if kind == "wifi" and connected else interface if connected else "Offline"
    )
    return [
        item(
            "connection",
            label,
            "wifi" if kind == "wifi" else "lan",
            dict(disconnected=not connected),
            f"{interface} · {state}\n{ssid}"
            + (f" · {signal}%" if signal is not None else ""),
            kind="symbol",
            actions={"leftClick": "network.panel"},
        )
    ]


async def mullvad(m):
    # Structured status keeps city names separate from IP/address annotations.
    data = await json_command(["mullvad", "status", "--json"])
    state = data.get("state") if isinstance(data, dict) else None
    if not isinstance(state, str) or not state:
        raise Unavailable("Mullvad status unavailable")
    connected = state == "connected"
    place = (data.get("details") or {}).get("location") or {}
    location = ", ".join(place[k] for k in ("country", "city") if isinstance(place.get(k), str))
    rows = [
        item(
            "vpn",
            None,
            icon("flags/" + COUNTRY_FLAGS.get(location.split(",")[0].strip(), "un"))
            if connected and m.get("regionFlag", True)
            else "shield_lock"
            if connected
            else "shield",
            dict(active=connected, disconnected=state == "disconnected"),
            f"Mullvad {state}\n{location}",
            kind="image" if connected and m.get("regionFlag", True) else "symbol",
            actions={
                "leftClick": {
                    "command": m.get("gui", ["/opt/Mullvad VPN/mullvad-gui"]),
                    "detach": True,
                },
                "rightClick": ["mullvad", "connect"],
                "middleClick": ["mullvad", "disconnect"],
            },
        )
    ]

    if connected and m.get("regionFlag", True):
        rows[0]["icon"]["style"].update(greyscaleIcons=False, monochromeIcon=False)
    catalog = await connectivity.relay_catalog() if m.get("panel", {}).get("hover") else {}
    rows[0]["panel"] = connectivity.mullvad_panel(state, location, catalog, m.get("quickCountries", ["nl", "jp", "sg"]))
    return rows


async def tailscale(m):
    data = await json_command(["tailscale", "status", "--json"])
    state = data.get("BackendState")
    if not state:
        raise Unavailable("Tailscale backend unavailable")
    rows = [
        item(
            "tailnet",
            None,
            m.get("symbol", "vpn_lock"),
            dict(active=state == "Running", disconnected=state != "Running"),
            "Tailscale "
            + (
                {
                    "Running": "connected",
                    "NeedsLogin": "needs login",
                    "Stopped": "disconnected",
                }.get(state, state)
            ),
            kind="symbol",
            actions={"leftClick": "tailscale.toggle"},
        )
    ]

    rows[0]["panel"] = connectivity.tailscale_panel(data)
    return rows


async def audio_state():
    sinks = await json_command(["pactl", "-f", "json", "list", "sinks"])
    default = await command(["pactl", "get-default-sink"])
    current = next((s for s in sinks if s["name"] == default), None)
    if current is None:
        raise Unavailable("No default audio sink")
    return sinks, current


async def audio(m):
    sinks, s = await audio_state()
    channels = list(s["volume"].values())
    volume = round(sum(v["value"] for v in channels) / len(channels) / 65536 * 100)
    muted = s["mute"]
    headphones = is_headphones(s)
    row = item(
        "master",
        "-%" if muted else f"{volume}%",
        "headphones" if headphones else "volume_up",
        dict(muted=muted),
        s.get("description", s["name"]),
        kind="symbol",
        actions={
            "scrollUp": "audio.up",
            "scrollDown": "audio.down",
            "middleClick": "audio.mute",
            "hoverEnter": "audio.panel",
        },
    )
    row["icon"]["actions"] = {"leftClick": "audio.route"}
    row["text"]["actions"] = {"leftClick": "audio.mute"}
    return [row]


async def clock(m):
    env = os.environ.copy()
    if m.get("timezone"):
        env["TZ"] = m["timezone"]
    if m.get("locale"):
        env["LC_TIME"] = m["locale"]
    raw = await command(
        [
            "date",
            "+"
            + m.get("format", "%H:%M")
            + "\n"
            + m.get("alternate", "%a %d %b")
            + "\n"
            + m.get("tooltipFormat", "%A, %d %B %Y"),
        ],
        env=env,
    )
    values = raw.splitlines()
    if len(values) != 3:
        raise Unavailable("date formats must produce one line each")
    row = item(
        "time", values[0], tooltip=values[2], actions={"leftClick": "datetime.toggle"}
    )
    row["alternate"] = values[1]
    row["panel"] = details.world_clock(m.get("worldClocks"))
    return [row]


async def locale(m):
    current = await command(["fcitx5-remote", "--check", "-n"], timeout=2)
    if not current:
        raise Unavailable("Fcitx 5 unavailable")
    label, tooltip = METHODS.get(current, (current, current))
    rows = [
        item(
            "method",
            label,
            tooltip=tooltip,
            actions={
                "leftClick": "locale.cycle",
                "rightClick": {"command": ["fcitx5-configtool"], "detach": True},
            },
        )
    ]

    try:
        group = await command(["fcitx5-remote", "--check", "-q"], timeout=2)
        methods = await asyncio.to_thread(details.input_methods, group)
        rows[0]["panel"] = details.panel(
            "Input methods",
            [
                details.field(
                    METHODS.get(method, (method, method))[0],
                    "Active" if method == current else "",
                    METHODS.get(method, (method, method))[1],
                    id=method,
                    active=method == current,
                )
                for method in methods
            ],
            "Fcitx group · " + group,
        )
        if not methods:
            rows[0]["panel"]["rows"] = [
                details.field(
                    "Available methods",
                    "Unavailable",
                    "Active group is missing from the Fcitx profile",
                )
            ]
    except Unavailable:
        rows[0]["panel"] = details.panel(
            "Input methods",
            [details.field(label, "Active", tooltip)],
            "Group list unavailable",
        )
    return rows


def quota_api_rows(data):
    rows = []
    for a in data["accounts"]:
        if a["status"] != "active":
            continue
        # Duration is authoritative; secondary is only the historical fallback.
        window = next(
            (
                w
                for w in ("Primary", "Secondary")
                if a.get("windowMinutes" + w) == 10080
            ),
            None,
        )
        if window is None and a.get("windowMinutesSecondary") is None:
            window = "Secondary"
        if window is None:
            continue
        reset = a.get("resetAt" + window)
        rows.append(
            dict(
                label=a.get("alias") or a.get("email", "account"),
                plan_type=a["planType"].lower(),
                window_name="weekly",
                remaining_percent=(a.get("usage") or {}).get(
                    window.lower() + "RemainingPercent"
                ),
                reset_at=int(
                    datetime.datetime.fromisoformat(
                        reset.replace("Z", "+00:00")
                    ).timestamp()
                )
                if reset
                else None,
            )
        )
    return rows


async def codex(m):
    summary = [details.field("Request totals", "Local database only")]
    if m.get("source", "db") == "api":
        rows = quota_api_rows(
            await asyncio.to_thread(
                http_json, m.get("url", "http://127.0.0.1:2455/api/accounts")
            )
        )
    else:
        import sqlite3

        p = Path(
            m.get("database", str(Path.home() / ".codex-lb/store.db"))
        ).expanduser()
        if not p.exists():
            raise Unavailable("Codex quota database unavailable")

        def query():
            from contextlib import closing

            with closing(
                sqlite3.connect("file:" + str(p) + "?mode=ro", uri=True, timeout=2)
            ) as con:
                con.row_factory = sqlite3.Row
                return con.execute(quota.QUERY).fetchall(), details.request_summary(con)

        rows, summary = await asyncio.to_thread(query)
    payload = quota.build_payload(rows)
    result = [
        item(
            "weekly",
            payload["text"].split(" ", 1)[-1],
            icon("codex"),
            tooltip=payload["tooltip"],
            kind="image",
        )
    ]

    result[0]["panel"] = details.codex_panel(rows, summary, result[0]["text"]["value"])
    return result


async def openrouter(m):
    key = os.environ.get(m.get("keyEnv", "OPENROUTER_API_KEY"))
    if not key:
        raise Unavailable("OPENROUTER_API_KEY is not set")

    def fetch(path, params=None):
        try:
            return http_json(
                "https://openrouter.ai/api/v1" + path,
                {"Authorization": "Bearer " + key, "Accept": "application/json"},
            )
        except RateLimit:
            raise
        except Unavailable as e:
            raise credits.OpenRouterError(str(e)) from None

    payload = await asyncio.to_thread(credits.build_payload, fetch)
    result = [
        item(
            "credit",
            m.get("currencySymbol", "") + payload["text"].removeprefix("$"),
            icon("openrouter"),
            kind="image",
            tooltip=payload["tooltip"],
        )
    ]
    result[0]["panel"] = details.openrouter_panel(payload)
    return result


async def dnd(m):
    data = await json_command([m.get("ctl", "notjikctl"), "status"])
    if not data.get("ok") or not isinstance(data.get("paused"), bool):
        raise Unavailable("notjik unavailable")
    paused = data["paused"]
    return [
        item(
            "dnd",
            None,
            "notifications_off" if paused else "notifications",
            dict(active=paused),
            "Notifications : OFF" if paused else "Notifications : ON",
            kind="symbol",
            actions={"leftClick": "DND.toggle"},
        )
    ]


async def running(name):
    try:
        await command(["pgrep", "-x", name])
        return True
    except Unavailable as e:
        if "exit 1" in str(e):
            return False
        raise


async def caffeine(m):
    enabled = not await running("hypridle")
    return [
        item(
            "idle",
            None,
            "coffee" if enabled else "hotel",
            dict(active=enabled),
            "Idle Inhibit : ON" if enabled else "Idle Inhibit : OFF",
            kind="symbol",
            actions={"leftClick": "caffeine.toggle"},
        )
    ]


async def nightlight(m):
    enabled = await running("hyprsunset")
    return [
        item(
            "filter",
            None,
            "nightlight" if enabled else "light_mode",
            dict(active=enabled),
            "Nightshift : ON" if enabled else "Nightshift : OFF",
            kind="symbol",
            actions={"leftClick": "nightlight.toggle"},
        )
    ]


async def monitor(m):
    rows = await json_command(["hyprctl", "-j", "monitors", "all"])
    enabled = any(
        r["name"] == m.get("tv", "HDMI-A-1") and not r.get("disabled") for r in rows
    )
    return [
        item(
            "layout",
            None,
            "sports_esports" if enabled else "desktop_windows",
            dict(active=enabled),
            "Game Mode" if enabled else "Desktop Mode",
            kind="symbol",
            actions={"leftClick": "monitor.toggle"},
        )
    ]


async def process(m):
    raw = await command(
        m["command"], timeout=m.get("timeout", 5), limit=m.get("maxOutput", 262144)
    )
    import json

    return untrusted_items(json.loads(raw))


REGISTRY = {
    "crypto": crypto,
    "network": network,
    "mullvad": mullvad,
    "tailscale": tailscale,
    "audio": audio,
    "datetime": clock,
    "locale": locale,
    "codex": codex,
    "openrouter": openrouter,
    "DND": dnd,
    "caffeine": caffeine,
    "nightlight": nightlight,
    "monitor-layout": monitor,
    "process": process,
}
