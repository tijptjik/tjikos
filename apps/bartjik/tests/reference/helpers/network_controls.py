"""Small JSON/stdin bridge to NetworkManager. No shell or secrets in argv."""

import json
import os
import re
import subprocess
import sys
import time


def nmcli(*args, password=None):
    result = subprocess.run(
        ["nmcli", "--colors", "no", "--wait", "25", *args],
        input=(password + "\n") if password is not None else "",
        capture_output=True,
        text=True,
        timeout=30,
        env={**os.environ, "LC_ALL": "C"},
    )
    if result.returncode:
        error = result.stderr.strip() or "NetworkManager could not complete the request."
        if password:
            error = error.replace(password, "[redacted]")
        raise RuntimeError(error)
    return result.stdout.rstrip("\n")


def fields(line):
    """Decode nmcli's escaped colons and backslashes, including SSIDs."""
    result, value, escaped = [], "", False
    for char in line:
        if escaped:
            value += char
            escaped = False
        elif char == "\\":
            escaped = True
        elif char == ":":
            result.append(value)
            value = ""
        else:
            value += char
    result.append(value)
    return result


def rows(columns, *args):
    return [fields(line) for line in nmcli("-t", "-f", columns, *args).splitlines() if line]


def parse_counters(output):
    counters = {}
    for line in output.splitlines():
        interface, separator, values = line.rpartition(":")
        columns = values.split()
        if separator and len(columns) >= 16:
            counters[interface.strip()] = dict(rx=int(columns[0]), tx=int(columns[8]))
    return counters


def traffic_sample(current, previous, elapsed):
    result = {}
    for interface, counters in current.items():
        old = previous.get(interface)
        valid = old is not None and elapsed > 0 and all(counters[key] >= old[key] for key in ("rx", "tx"))
        result[interface] = dict(**counters,
            rxRate=(counters["rx"] - old["rx"]) / elapsed if valid else None,
            txRate=(counters["tx"] - old["tx"]) / elapsed if valid else None)
    return result


def watch_traffic():
    previous, timestamp = {}, time.monotonic()
    while True:
        with open("/proc/net/dev", encoding="utf-8") as source:
            current = parse_counters(source.read())
        now = time.monotonic()
        print(json.dumps(traffic_sample(current, previous, now - timestamp)), flush=True)
        previous, timestamp = current, now
        time.sleep(1)


def address_details(output):
    devices, current = {}, None
    for line in output.splitlines():
        key, separator, value = line.partition(":")
        if not separator:
            continue
        if key == "GENERAL.DEVICE":
            current = dict(ip=[], ipv6=[], gateway="", dns=[], dhcp="")
            devices[value] = current
        elif current is not None:
            if key.startswith("IP4.ADDRESS["):
                current["ip"].append(value)
            elif key.startswith("IP6.ADDRESS["):
                current["ipv6"].append(value)
            elif key == "IP4.GATEWAY":
                current["gateway"] = value
            elif key.startswith("IP4.DNS["):
                current["dns"].append(value)
            elif key.startswith("DHCP4.OPTION["):
                option, _, address = value.partition(" = ")
                if option == "dhcp_server_identifier":
                    current["dhcp"] = address
    return devices


def default_dns_routes(links):
    """Resolvers for names without a more-specific split-DNS route."""
    routes = [link for link in links if link.get("servers")]
    catch_all = [link for link in routes
                 if any(domain.get("name") == "." for domain in link.get("searchDomains", []))]
    selected = catch_all or [link for link in routes if link.get("defaultRoute") or not link.get("ifname")]
    return list(dict.fromkeys(server["addressString"] for link in selected
                             for server in link["servers"] if server.get("addressString")))


def default_dns():
    try:
        result = subprocess.run(["resolvectl", "--json=short", "status"],
                                capture_output=True, text=True, timeout=3)
        if result.returncode == 0:
            links = json.loads(result.stdout)
            if isinstance(links, list):
                return default_dns_routes(links)
    except (OSError, ValueError, subprocess.TimeoutExpired):
        pass
    return []


MODULE_KIND = "network"


def snapshot():
    devices = []
    for interface, kind, state, connection in rows("DEVICE,TYPE,STATE,CONNECTION", "device", "status"):
        if kind in ("wifi", "ethernet"):
            devices.append(dict(interface=interface, kind=kind, state=state, connection=connection))
    enabled = nmcli("radio", "wifi") == "enabled"
    networks = {}
    if enabled and any(device["kind"] == "wifi" for device in devices):
        for active, ssid, bssid, signal, security, interface in rows(
            "IN-USE,SSID,BSSID,SIGNAL,SECURITY,DEVICE", "device", "wifi", "list", "--rescan", "no"
        ):
            if not ssid:
                continue
            item = dict(ssid=ssid, bssid=bssid, signal=int(signal or 0), security="" if security == "--" else security,
                        interface=interface, active=active == "*")
            key = (interface, ssid, security)
            old = networks.get(key)
            if old is None or (item["active"], item["signal"]) > (old["active"], old["signal"]):
                networks[key] = item
    details = address_details(nmcli("-t", "-f", "GENERAL.DEVICE,IP4.ADDRESS,IP4.GATEWAY,IP4.DNS,DHCP4.OPTION,IP6.ADDRESS", "device", "show"))
    for device in devices:
        device["addresses"] = details.get(device["interface"], {})
    return dict(enabled=enabled, devices=devices, defaultDns=default_dns(),
                networks=sorted(networks.values(), key=lambda item: (not item["active"], -item["signal"], item["ssid"])))


def execute(request):
    action = request.get("action", "status")
    if action == "scan":
        nmcli("device", "wifi", "rescan")
    elif action == "radio":
        if not isinstance(request.get("enabled"), bool):
            raise ValueError("A Wi-Fi radio state is required.")
        nmcli("radio", "wifi", "on" if request["enabled"] else "off")
    elif action in ("connect", "disconnect", "ethernet"):
        interface = request.get("interface", "")
        if not interface or interface.startswith("-"):
            raise ValueError("Select a network interface.")
        if action == "connect":
            bssid = request.get("bssid", "")
            if not re.fullmatch(r"(?:[0-9a-fA-F]{2}:){5}[0-9a-fA-F]{2}", bssid):
                raise ValueError("Select a visible Wi-Fi network and try again.")
            password = request.get("password", "")
            if any(char in password for char in "\r\n\x00"):
                raise ValueError("The password must be a single line.")
            # --ask reads a missing secret from stdin; it never enters the command line.
            # An empty password lets NetworkManager reuse the saved connection secrets.
            args = (["--ask"] if password else []) + ["device", "wifi", "connect", bssid, "ifname", interface]
            nmcli(*args, password=password if password else None)
        else:
            nmcli("device", "disconnect" if action == "disconnect" else "connect", interface)
    elif action != "status":
        raise ValueError("Unknown networking action.")


def main():
    try:
        request = json.loads(sys.stdin.readline())
        execute(request)
        print(json.dumps(dict(ok=True, **snapshot())))
    except (ValueError, RuntimeError, OSError, subprocess.TimeoutExpired) as error:
        message = "NetworkManager timed out. Check the connection and retry." if isinstance(error, subprocess.TimeoutExpired) else str(error)
        print(json.dumps(dict(ok=False, error=message)))


if __name__ == "__main__":
    if sys.argv[1:] == ["traffic"]:
        watch_traffic()
    else:
        main()
