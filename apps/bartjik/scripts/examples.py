"""Regenerate the public presets. No live machine configuration is read."""

import copy, json
from pathlib import Path

root = Path(__file__).resolve().parents[1]
modules = {
    "spaces": {
        "provider": "workspaces",
        "scope": "screen",
        "special": False,
        "occupiedOnly": True,
        "label": "name",
        "appearance": "separators",
        "style": {"iconTextGap": 0, "itemGap": 0},
        "icon": {"style": {"minItemSize": 12, "iconSize": 12}},
        "text": {"style": {"minItemSize": 16, "fontSize": 14}},
        "preview": {"enabled": True, "delay": 300, "width": 640, "slideDuration": 120},
        "persistent": {
            "DP-1": list(range(1, 11)),
            "DP-2": list(range(11, 21)),
            "HDMI-A-1": [21, 22],
            "eDP-1": list(range(1, 11)),
        },
    },
    "music": {
        "provider": "playerctl",
        "players": ["spotify"],
        "panel": {"hover": True, "delay": 300, "closeDelay": 200},
        "artist": {"style": {"maxTextWidth": 180, "marquee": True}},
        "track": {"style": {"maxTextWidth": 300, "marquee": True}},
        "actions": {"rightClick": "player.quit"},
    },
    "prices": {
        "provider": "crypto",
        "interval": 600,
        "currency": "USD",
        "currencySymbol": "",
        "assets": [
            {"symbol": "BTC", "price_precision": 0},
            {"symbol": "SOL", "price_precision": 0},
            {"symbol": "AR", "price_precision": 1},
        ],
        "items": {
            "BTC/USD": {
                "icon": {
                    "actions": {
                        "leftClick": [
                            "xdg-open",
                            "https://coinmarketcap.com/currencies/bitcoin/",
                        ]
                    }
                },
                "text": {
                    "actions": {
                        "leftClick": [
                            "xdg-open",
                            "https://www.tradingview.com/symbols/BTCUSD/",
                        ]
                    }
                },
            }
        },
    },
    "tray": {"provider": "icontray", "exclude": ["Input Method"]},
    "net": {"provider": "network", "actions": {"leftClick": "network.panel"}},
    "vpn": {
        "provider": "mullvad",
        "actions": {
            "leftClick": ["mullvad", "connect"],
            "rightClick": "tailscale.toggle",
            "middleClick": ["mullvad", "disconnect"],
        },
    },
    "tailnet": {
        "provider": "tailscale",
        "symbol": "vpn_lock",
        "actions": {"leftClick": "tailscale.toggle"},
    },
    "sound": {
        "provider": "audio",
        "icon": {"actions": {"leftClick": "audio.route"}},
        "text": {"actions": {"leftClick": "audio.panel"}},
        "actions": {
            "scrollUp": "audio.up",
            "scrollDown": "audio.down",
            "middleClick": "audio.mute",
        },
    },
    "time": {
        "provider": "datetime",
        "format": "%H:%M",
        "alternate": "%a %d %b",
        "tooltipFormat": "%A, %d %B %Y",
        "actions": {"leftClick": "datetime.toggle"},
    },
    "input": {
        "provider": "locale",
        "actions": {
            "leftClick": "locale.cycle",
            "rightClick": {"command": ["fcitx5-configtool"], "detach": True},
        },
    },
    "quota": {"provider": "codex", "source": "db"},
    "credit": {"provider": "openrouter", "interval": 600},
    "notifications": {"provider": "DND", "actions": {"leftClick": "DND.toggle"}},
    "idle": {
        "provider": "caffeine",
        "actions": {
            "leftClick": "caffeine.toggle",
            "middleClick": ["systemctl", "suspend"],
        },
    },
    "filter": {
        "provider": "nightlight",
        "actions": {"leftClick": "nightlight.toggle", "middleClick": None},
    },
    "display": {
        "provider": "monitor-layout",
        "actions": {"leftClick": "monitor.toggle"},
    },
}

modules["net"]["actions"]["hoverEnter"] = "network.panel"
modules["vpn"]["quickCountries"] = ["nl", "jp", "sg"]

for mid in ("time", "vpn", "tailnet", "input", "quota", "credit"):
    modules[mid]["panel"] = {"hover": True, "delay": 300, "closeDelay": 160}
modules["time"]["worldClocks"] = [
    {"label": "New York", "timezone": "America/New_York"},
    {"label": "London", "timezone": "Europe/London"},
    {"label": "Amsterdam", "timezone": "Europe/Amsterdam"},
    {"label": "Tokyo", "timezone": "Asia/Tokyo"},
]
modules["tray"]["exclude"] = ["Input Method", "solaar"]
modules["tray"]["iconOverrides"] = {'chrome_status_icon_*': 'builtin:googlechrome', 'Mullvad VPN_status_icon_*': 'builtin:mullvad', 'TelegramDesktop': 'builtin:telegram', 'ZapZap': 'builtin:whatsapp', 'spotify*': 'builtin:spotify', 'com.spotify.Client': 'builtin:spotify'}
modules["tray"]["style"] = {
    "iconSize": 21,
    "greyscaleIcons": True,
    "monochromeIcon": False,
}


def island(id, *modules):
    return dict(id=id, modules=list(modules))


def bar(id, pos, align, islands, monitors="all", **rest):
    return dict(
        id=id,
        monitors=monitors,
        side="b",
        position=pos,
        alignment=align,
        offsetY=-20,
        islandGap=10,
        islands=islands,
        **rest,
    )


base = dict(
    theme="rose-punk",
    style={},
    layout={"centre_width": "26%"},
    modules=modules,
    perMonitor={"DP-2": {"modules": {"music": {"players": ["spotify"]}}}},
    bars=[
        bar(
            "workspace-bar",
            "l",
            "r",
            [island("workspaces", "spaces"), island("media", "music")],
            ["DP-1", "DP-2"],
            perMonitor={"DP-2": {"islands": [island("workspaces", "spaces")]}},
        ),
        bar(
            "status-bar",
            "r",
            "l",
            [
                island("markets", "prices"),
                island("applications", "tray"),
                island("connectivity", "net", "vpn", "tailnet"),
                island("audio-clock-input", "sound", "time", "input"),
            ],
            ["DP-1"],
        ),
        bar(
            "controls-bar",
            "r",
            "l",
            [
                island("ai", "quota", "credit"),
                island("controls", "notifications", "idle", "filter", "display"),
            ],
            ["DP-1"],
        ),
    ],
)
(root / "examples/desktop.json").write_text(
    json.dumps(base, ensure_ascii=False, indent=2) + "\n"
)
laptop = copy.deepcopy(base)
laptop["modules"]["music"]["players"] = ["Feishin"]
laptop["layout"] = {"centre_width": "20%"}
laptop["perMonitor"] = {}
laptop["bars"] = [
    bar(
        "workspace-bar",
        "l",
        "r",
        [island("workspaces", "spaces"), island("media", "music")],
        ["eDP-1"],
    ),
    bar(
        "status-bar",
        "r",
        "l",
        [
            island("connectivity", "net", "vpn", "tailnet"),
            island("audio-clock-input", "sound", "time", "input"),
        ],
        ["eDP-1"],
    ),
    bar(
        "services-bar",
        "r",
        "l",
        [
            island("markets", "prices"),
            island("applications", "tray"),
            island("ai", "quota", "credit"),
        ],
        ["eDP-1"],
    ),
    bar(
        "controls-bar",
        "l",
        "r",
        [island("controls", "notifications", "idle", "filter", "display")],
        ["eDP-1"],
    ),
]
(root / "examples/laptop.json").write_text(
    json.dumps(laptop, ensure_ascii=False, indent=2) + "\n"
)
demo = copy.deepcopy(base)
demo["layout"] = {"centre_width": "20%"}
demo["perMonitor"] = {}
for b in demo["bars"]:
    b["monitors"] = [1]
    b["offsetY"] = -100
(root / "examples/demo.json").write_text(
    json.dumps(demo, ensure_ascii=False, indent=2) + "\n"
)
(root / "examples/minimal.json").write_text(
    json.dumps(
        {
            "theme": "monochrome",
            "modules": {"time": modules["time"]},
            "bars": [bar("clock", "c", "c", [island("clock", "time")])],
        },
        indent=2,
    )
    + "\n"
)
(root / "examples/optional-poweroff.json").write_text(
    json.dumps(
        {
            "modules": {
                "filter": {
                    "provider": "nightlight",
                    "actions": {"middleClick": ["systemctl", "poweroff"]},
                }
            },
            "bars": [
                {
                    "id": "nightlight",
                    "islands": [{"id": "filter", "modules": ["filter"]}],
                }
            ],
        },
        indent=2,
    )
    + "\n"
)

comparison = copy.deepcopy(base)
comparison["layout"]["centre_width"] = "50%"
comparison["style"].update(paddingX=12, paddingY=10)
comparison["modules"]["tray"]["style"] = {
    "iconSize": 21,
    "greyscaleIcons": True,
    "monochromeIcon": False,
}
comparison["modules"]["vpn"]["regionFlag"] = True
comparison["modules"]["credit"]["currencySymbol"] = ""
comparison["modules"]["prices"]["panel"] = {
    "hover": True,
    "delay": 300,
    "closeDelay": 160,
}
comparison["modules"]["spaces"].pop("persistent", None)
comparison["modules"]["prices"]["items"] = {}
comparison["modules"]["prices"]["actions"] = {"leftClick": "crypto.panel"}
for asset in comparison["modules"]["prices"]["assets"]:
    asset.pop("icon", None)
for mid in ("notifications", "idle", "filter", "vpn"):
    comparison["modules"][mid]["style"] = {"states": {"active": {"color": "#e5e5e5"}}}
comparison["bars"][1]["islands"].extend(comparison["bars"][2]["islands"])
comparison["bars"] = comparison["bars"][:2]
markets = next(i for i in comparison["bars"][1]["islands"] if i["id"] == "markets")
comparison["bars"][1]["islands"] = [
    i for i in comparison["bars"][1]["islands"] if i["id"] != "markets"
]
comparison["bars"][0]["islands"].insert(0, markets)
order = ["applications", "ai", "connectivity", "audio-clock-input", "controls"]
comparison["bars"][1]["islands"].sort(key=lambda i: order.index(i["id"]))
for b in comparison["bars"]:
    b.update(
        offsetY=-60,
        thickness=44,
        overflow="centre",
        autoHide=True,
        activationEdge="b",
        activationDelay=0,
        hideDelay=1000,
        keepVisibleDistance="140px",
        fadeDuration=180,
    )
(root / "examples/comparison.json").write_text(
    json.dumps(comparison, ensure_ascii=False, indent=2) + "\n"
)
