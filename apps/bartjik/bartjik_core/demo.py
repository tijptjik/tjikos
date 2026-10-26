"""Synthetic data only, no account identifiers, device names or real balances."""

from .model import item
from . import details, connectivity
import datetime
import time
from .icons import icon as artwork


def items(provider, options=None):
    options = options or {}
    if provider == "workspaces":
        return [
            item(
                str(n),
                None if options.get("label") == "circles" else options.get("labels", {}).get(str(n), str(n)),
                artwork("circle-filled" if n == 3 else "circle-open")
                if options.get("label") in ("circles", "circles_name")
                else None,
                kind="image",
                state={"active": n == 3, "focused": n == 3, "urgent": n == 6},
                tooltip="Synthetic workspace",
            )
            for n in ((1, 2, 3, 6, 10) if options.get("demoPresentation") else (1, 2, 3, 6, 10, 14))
        ]
    if provider == "crypto":
        rows = [
            item(
                symbol + "/USD",
                price,
                artwork(name),
                kind="image",
                actions={"leftClick": "crypto.panel"},
            )
            for symbol, price, name in [
                ("BTC", "67432", "bitcoin"),
                ("SOL", "148", "solana"),
                ("AR", "18.4", "arweave"),
            ]
        ]
        for row in rows:
            row["changes"] = {"1h": 0.43, "24h": -1.72, "7d": 4.81}
        return rows

    if provider == "icontray":
        if options.get("demoTrayFixtures"):
            return [item("tray-" + str(n), None,
                         artwork("tray/" + override.removeprefix("builtin:")) if override.startswith("builtin:") else path,
                         kind="image", tooltip=title)
                    for n, (app_id, title, path) in enumerate(options["demoTrayFixtures"])
                    for override in [options.get("iconOverrides", {}).get(app_id, "")]]
        return [
            item(
                "tray-" + str(n),
                None,
                artwork("tray/" + (icon + "-outline" if options.get("iconOverrides", {}).get({"chatgpt":"ChatGPT", "telegram":"TelegramDesktop", "whatsapp":"ZapZap"}[icon]) == "builtin:" + icon + "-outline" else icon)),
                kind="image",
                tooltip={"chatgpt":"ChatGPT", "telegram":"Telegram", "whatsapp":"WhatsApp"}[icon] if options.get("demoPresentation") else "Synthetic tray item",
                state={"urgent": icon=="telegram"},
            )
            for n, icon in enumerate(["chatgpt", "telegram", "whatsapp"])
        ]
    if provider == "playerctl":
        row = item("demo-player", "Evening light across the quiet harbour", "pause", kind="symbol", actions={"middleClick":"player.toggle","scrollUp":"player.previous","scrollDown":"player.next"})
        row["icon"]["actions"] = {"leftClick":"player.toggle"}
        row["artist"] = {"value":"Synthetic Artist", "style":{}, "actions":{"leftClick":"player.artist"}}
        row["track"] = {"value":row["text"]["value"], "style":{}, "actions":{"leftClick":"player.playlist"}}
        row["media"] = dict(artist=row["artist"]["value"], title=row["track"]["value"], album="Evening sessions", artwork=artwork("demo-album").replace(".svg", ".png"), playing=True, canToggle=True, canPrevious=True, canNext=True, canShuffle=True, shuffle=False, playlistAvailable=False, player="Synthetic player")
        if options.get("demoPresentation"):
            row["artist"]["value"]="Karla Blum"
            row["track"]["value"]=row["text"]["value"]="Acid Rain"
            row["media"].update(artist="Karla Blum",title="Acid Rain",album="Acid Rain")
        return [row]
    data = {
        "playerctl": ("Synthetic Artist — Evening light", "play_arrow"),
        "network": ("Demo Wi-Fi", "wifi"),
        "mullvad": (None, artwork("flags/jp")),
        "tailscale": (None, "vpn_lock"),
        "audio": ("42%", "headphones"),
        "datetime": ("21:34", None),
        "locale": ("EN", None),
        "codex": ("83%", artwork("codex")),
        "openrouter": ("24.80", artwork("openrouter")),
        "DND": (None, "notifications"),
        "caffeine": (None, "coffee"),
        "nightlight": (None, "nightlight"),
        "monitor-layout": (None, "desktop_windows"),
        "process": ("Demo", None),
    }
    text, icon = data[provider]
    if provider == "nightlight" and options.get("demoPresentation"):
        icon = "light_mode"
    rows = [
        item(
            "demo",
            text,
            icon,
            tooltip=({"DND":"Notifications : ON","caffeine":"Idle Inhibit : ON","nightlight":"Nightshift : OFF","monitor-layout":"Desktop Mode","audio":"Volume: 42%","locale":"English"}.get(provider,"Synthetic "+provider) if options.get("demoPresentation") else "Synthetic "+provider),
            kind="image"
            if provider in ("codex", "openrouter", "mullvad")
            else "symbol",
        )
    ]

    if provider == "mullvad":
        rows[0]["icon"]["style"].update(greyscaleIcons=False, monochromeIcon=False)
    panels = {
        "openrouter": details.openrouter_panel(dict(
            text="$24.80", source=None, activityStatus="available",
            dailyUsage={"2026-06-14": 128400, "2026-06-13": 86400, "2026-06-12": 45200},
        )),
        "datetime": details.world_clock(
            options.get("worldClocks"),
            datetime.datetime(2026, 6, 15, 12, 0, tzinfo=datetime.timezone.utc),
        ),
        "mullvad": connectivity.mullvad_panel("connected", "Japan, Tokyo", {
            "jp": dict(id="jp", name="Japan", cities=[dict(id="osa", name="Osaka"), dict(id="tyo", name="Tokyo")]),
            "nl": dict(id="nl", name="Netherlands", cities=[dict(id="ams", name="Amsterdam")]),
            "sg": dict(id="sg", name="Singapore", cities=[dict(id="sin", name="Singapore")]),
        }, ["nl", "jp", "sg"]),
        "tailscale": connectivity.tailscale_panel({
            "BackendState": "Running", "Self": {"HostName": "demo-desktop"}, "TailscaleIPs": ["100.64.0.1"],
            "Peer": {
                "laptop": {"HostName": "demo-laptop", "Online": True, "TailscaleIPs": ["100.64.0.2"]},
                "phone": {"HostName": "demo-phone", "Online": False, "TailscaleIPs": ["100.64.0.3"]},
            },
        }),
        "locale": details.panel(
            "Input methods",
            [
                details.field("EN", "Active", "English", id="keyboard-us", active=True),
                details.field("拼", "", "Pinyin", id="pinyin"),
                details.field("速", "", "Quick", id="quick-classic"),
            ],
            "Fcitx group · Demo",
        ),
        "codex": details.panel(
            "Codex usage",
            [
                details.field("Weekly remaining", "83%", "Capacity-weighted", id="weekly"),
                details.field("Requests · 24h", "1,234", "3 failed", id="requests", amount=1234, failures=3),
                details.field("Input / output tokens", "423,100 / 81,200", id="tokens", input=423100, output=81200),
                details.field("Cached input", "212,000", id="cached", amount=212000),
                details.field("Recorded cost · 24h", "$12.34", "Synthetic usage", id="cost", amount=12.34),
                details.field("Mean request latency", "2.4s", id="latency", seconds=2.4),
                details.field("A01", "83%", "pro · resets in 3d12h", progress=.83, group="account", plan="pro", resetAt=time.time()+84*3600),
                details.field("B02", "72%", "pro · resets in 2d8h", progress=.72, group="account", plan="pro", resetAt=time.time()+56*3600),
                details.field("C03", "94%", "pro · resets in 5d1h", progress=.94, group="account", plan="pro", resetAt=time.time()+121*3600),
                details.field("D04", "81%", "pro · resets in 4d6h", progress=.81, group="account", plan="pro", resetAt=time.time()+102*3600),
            ],
            "codex-lb · read-only · weekly quota",
        ),
    }
    if provider in panels:
        rows[0]["panel"] = panels[provider]
    return rows
