"""Read-only panel models and explicit, validated connectivity actions."""

import asyncio
import ipaddress
import re
import time
from . import details
from .io import command, json_command, Unavailable

_catalog = {}
_catalog_until = 0
_catalog_lock = None


def parse_relays(raw):
    countries = {}
    current = None
    for line in raw.splitlines():
        country = re.fullmatch(r"([^\t].+?) \(([a-z]{2})\)", line)
        city = re.match(r"\t([^\t].+?) \(([a-z]{3})\) @", line)
        if country:
            name, code = country.groups()
            current = countries.setdefault(code, dict(id=code, name=name, cities=[]))
        elif city and current is not None:
            name, code = city.groups()
            if not any(c["id"] == code for c in current["cities"]):
                current["cities"].append(dict(id=code, name=name))
    if not countries:
        raise Unavailable("Relay locations unavailable")
    return countries


async def relay_catalog():
    global _catalog, _catalog_until, _catalog_lock
    if _catalog_lock is None:
        _catalog_lock = asyncio.Lock()
    async with _catalog_lock:
        if time.monotonic() < _catalog_until:
            return _catalog
        try:
            _catalog = parse_relays(await command(["mullvad", "relay", "list"], timeout=5))
            _catalog_until = time.monotonic() + 3600
        except Unavailable:
            _catalog_until = time.monotonic() + 60
        return _catalog


def mullvad_panel(state, location, catalog, quick_countries):
    parts = [s.strip() for s in location.split(",")]
    country_name, city_name = (parts + ["", ""])[:2]
    country = next((c for c in catalog.values() if c["name"] == country_name), {})
    cities = country.get("cities", [])
    city = next((c for c in cities if c["name"] == city_name), {})
    return dict(
        details.panel("Mullvad", [details.field("Connection", state.title()), details.field("Location", ", ".join(p for p in (country_name, city_name) if p) or "—")]),
        connection=state, country=country_name or "Choose a country", city=city_name,
        countryCode=country.get("id", ""), cityCode=city.get("id", ""),
        cities=[dict(id=c["id"], name=c["name"], active=c["id"] == city.get("id")) for c in cities],
        countries=[dict(id=code, name=catalog[code]["name"], active=code == country.get("id")) for code in quick_countries if code in catalog],
        locationsAvailable=bool(catalog),
    )


def tailscale_panel(data):
    state = data.get("BackendState")
    own = data.get("Self") or {}
    peers = [dict(id=str(key), name=p.get("HostName") or "Peer", online=bool(p.get("Online")), address=next(iter(p.get("TailscaleIPs") or []), ""), os=p.get("OS") or "") for key,p in (data.get("Peer") or {}).items() if isinstance(p, dict)]
    peers.sort(key=lambda p: (not p["online"], p["name"]))
    ips = data.get("TailscaleIPs") or own.get("TailscaleIPs") or []
    device = own.get("HostName") or "This device"
    rows = [details.field("Backend", state), details.field("This device", device), details.field("Addresses", ", ".join(ips) or "—"), details.field("Peers online", f"{sum(p['online'] for p in peers)} / {len(peers)}")]
    return dict(details.panel("Tailscale", rows), connection=state, device=device, address=next(iter(ips), ""), peers=peers[:32], online=sum(p["online"] for p in peers), total=len(peers))


async def act(provider, action, selection, module, executor):
    async with executor.lock(provider):
        if provider == "mullvad":
            if action in ("connect", "disconnect"):
                await command(["mullvad", action], timeout=10)
                return "Connection requested" if action == "connect" else "Disconnect requested"
            catalog = await relay_catalog()
            if action == "country":
                if selection not in module.get("quickCountries", ["nl", "jp", "sg"]) or selection not in catalog:
                    raise Unavailable("This country is no longer available")
                await command(["mullvad", "relay", "set", "location", selection], timeout=10)
                return "Selected " + catalog[selection]["name"]
            if action == "city":
                if not isinstance(selection, dict) or not isinstance(selection.get("country"), str) or not isinstance(selection.get("city"), str):
                    raise Unavailable("This city is no longer available")
                country = catalog.get(selection["country"])
                city = next((c for c in country["cities"] if c["id"] == selection["city"]), None) if country else None
                if not city:
                    raise Unavailable("This city is no longer available")
                await command(["mullvad", "relay", "set", "location", country["id"], city["id"]], timeout=10)
                return "Selected " + city["name"]
        elif provider == "tailscale":
            if action == "admin":
                await executor.execute({"command": ["xdg-open", "https://login.tailscale.com/admin/machines"], "detach": True}, {}, {}, {}, [])
                return "Opened devices in your browser"
            data = await json_command(["tailscale", "status", "--json"])
            if not data.get("BackendState"):
                raise Unavailable("Tailscale is unavailable")
            if action == "toggle":
                await command(["tailscale", "down" if data["BackendState"] == "Running" else "up"], timeout=15)
                return "Connection change requested"
            if action in ("copy", "ping"):
                target = (data.get("Self") or {}) if selection == "self" else (data.get("Peer") or {}).get(selection)
                if not target:
                    raise Unavailable("This device is no longer available")
                addresses = (data.get("TailscaleIPs") or target.get("TailscaleIPs")) if selection == "self" else target.get("TailscaleIPs")
                try:
                    address = str(ipaddress.ip_address(addresses[0]))
                except (ValueError, IndexError, TypeError):
                    raise Unavailable("No address is available for this device") from None
                if action == "copy":
                    await command(["wl-copy"], input_text=address)
                    return "Address copied"
                if data["BackendState"] != "Running" or not target.get("Online"):
                    raise Unavailable("Connect both devices before checking reachability")
                reply = await command(["tailscale", "ping", "--c=1", "--timeout=3s", address], timeout=5)
                latency = re.search(r"\bin ([\d.]+\s?ms)\b", reply)
                return "Reachable" + (" · " + latency[1] if latency else "")
        raise Unavailable("This control is unavailable")
