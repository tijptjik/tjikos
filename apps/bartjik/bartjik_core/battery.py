"""System batteries only; USB peripherals must not create a laptop indicator."""

import asyncio
import math
from pathlib import Path

from .model import item
from .io import command, json_command, Unavailable

POWER_SUPPLY = Path("/sys/class/power_supply")
PROFILE_SERVICE = "org.freedesktop.UPower.PowerProfiles"
PROFILE_PATH = "/org/freedesktop/UPower/PowerProfiles"
PROFILES = (("performance", "Performance"), ("balanced", "Balanced"), ("power-saver", "Power Saver"))


def read(path, name):
    try:
        return (path / name).read_text().strip()
    except OSError:
        return ""


def number(path, name):
    try:
        value = float(read(path, name))
        return value if math.isfinite(value) and value >= 0 else None
    except ValueError:
        return None


def battery_item(percent, external):
    row = item(
        "battery", icon="bolt" if external else "battery_grid", kind="symbol",
        tooltip=f"{'AC power' if external else 'Battery'} · {percent:.0f}%",
    )
    row["battery"] = dict(percent=percent, external=external)
    row["actions"] = {"leftClick": "battery.panel"}
    return row


def snapshot(root=POWER_SUPPLY):
    try:
        supplies = list(root.iterdir())
    except OSError:
        return []
    batteries = [p for p in supplies if read(p, "type") == "Battery"
                 and read(p, "scope") != "Device" and read(p, "present") != "0"]
    if not batteries:
        return []
    capacities, energies = [], []
    for path in batteries:
        now, full = number(path, "energy_now"), number(path, "energy_full")
        if now is None or not full:
            now, full = number(path, "charge_now"), number(path, "charge_full")
            voltage = number(path, "voltage_min_design")
            if voltage and now is not None and full:
                now, full = now * voltage / 1e6, full * voltage / 1e6
            else:
                # Charge ratios are useful even without a voltage, but cannot
                # be summed with energy readings from another battery.
                capacity = number(path, "capacity")
                if capacity is None and now is not None and full:
                    capacity = 100 * now / full
                if capacity is not None:
                    capacities.append(capacity)
                continue
        capacities.append(100 * now / full)
        energies.append((now, full))
    if not capacities:
        return []
    percent = (100 * sum(n for n, _ in energies) / sum(f for _, f in energies)
               if len(energies) == len(batteries) else sum(capacities) / len(capacities))
    adapters = [read(p, "online") for p in supplies
                if read(p, "type") != "Battery" and read(p, "scope") != "Device"]
    online = [value for value in adapters if value in ("0", "1")]
    external = ("1" in online if online else
                any(read(p, "status") == "Charging" for p in batteries))
    return [battery_item(max(0, min(100, percent)), external)]


async def properties(service, path, interface):
    result = await json_command([
        "busctl", "--system", "--json=short", "call", service, path,
        "org.freedesktop.DBus.Properties", "GetAll", "s", interface,
    ], timeout=3)
    return {key: value["data"] for key, value in result["data"][0].items()}


async def profile_status():
    try:
        return await properties(PROFILE_SERVICE, PROFILE_PATH, PROFILE_SERVICE)
    except (Unavailable, KeyError, TypeError, ValueError):
        return {}


async def estimate_status():
    try:
        return await properties("org.freedesktop.UPower", "/org/freedesktop/UPower/devices/DisplayDevice", "org.freedesktop.UPower.Device")
    except (Unavailable, KeyError, TypeError, ValueError):
        return {}


def panel_data(battery, estimate, profiles):
    available = {profile.get("Profile", {}).get("data") for profile in profiles.get("Profiles", [])}
    external = battery["external"]
    state = estimate.get("State")
    seconds = estimate.get("TimeToFull" if external else "TimeToEmpty", 0)
    # Do not reuse an estimate from before an AC transition.
    valid = state == (1 if external else 2) and isinstance(seconds, (int, float)) and math.isfinite(seconds) and seconds > 0
    status = ("Fully charged" if state == 4 or battery["percent"] >= 100 else
              "Charging" if state == 1 else "Charging paused" if state in (3, 5) else "AC power") if external else "On battery"
    return dict(
        title="Battery", percent=battery["percent"], external=external,
        status=status, seconds=seconds if valid else None,
        estimateLabel="Until fully charged" if external else "Battery life remaining",
        estimateUnavailable="Fully charged" if status == "Fully charged" else
                            "Charging paused" if status == "Charging paused" and estimate else "Estimating…",
        profiles=[dict(id=key, label=label, available=key in available,
                       active=profiles.get("ActiveProfile") == key) for key, label in PROFILES],
        profileMessage=("Power modes unavailable" if not available else
                        "Performance limited: " + profiles["PerformanceDegraded"] if profiles.get("PerformanceDegraded") else ""),
    )


async def provider(options):
    rows = await asyncio.to_thread(snapshot)
    if rows:
        estimate, profiles = await asyncio.gather(estimate_status(), profile_status())
        rows[0]["panel"] = panel_data(rows[0]["battery"], estimate, profiles)
    return rows


async def select_profile(action, selection, executor):
    if action != "profile" or selection not in dict(PROFILES):
        raise Unavailable("Unknown power mode")
    async with executor.lock("battery-profile"):
        status = await profile_status()
        available = {profile.get("Profile", {}).get("data") for profile in status.get("Profiles", [])}
        if selection not in available:
            raise Unavailable("This power mode is unavailable")
        await command(["busctl", "--system", "set-property", PROFILE_SERVICE, PROFILE_PATH,
                       PROFILE_SERVICE, "ActiveProfile", "s", selection])
        if (await profile_status()).get("ActiveProfile") != selection:
            raise Unavailable("The power mode did not change")
    return dict(PROFILES)[selection] + " selected"
