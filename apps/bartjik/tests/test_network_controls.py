"""Network parsing and credential transport, without changing real connections."""

import importlib.util
from pathlib import Path
import subprocess
import sys
import unittest
from unittest.mock import patch

sys.dont_write_bytecode = True
SOURCE = Path(__file__).resolve().parents[1] / "qml/network_controls.py"
SPEC = importlib.util.spec_from_file_location("network", SOURCE)
network = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(network)


class NetworkingTests(unittest.TestCase):
    def setUp(self):
        resolver = patch.object(network, "default_dns", return_value=[])
        resolver.start()
        self.addCleanup(resolver.stop)

    def test_vpn_catch_all_dns_takes_precedence_over_wifi(self):
        wifi = dict(ifname="wlan0", defaultRoute=True, servers=[dict(addressString="192.168.1.1")])
        vpn = dict(ifname="wg0-mullvad", defaultRoute=True,
                   searchDomains=[dict(name=".", routeOnly=True)], servers=[dict(addressString="10.64.0.1")])
        self.assertEqual(network.default_dns_routes([wifi, vpn]), ["10.64.0.1"])
        self.assertEqual(network.default_dns_routes([wifi]), ["192.168.1.1"])

    def test_specific_dns_domains_do_not_replace_default_dns(self):
        wifi = dict(ifname="wlan0", defaultRoute=True, servers=[dict(addressString="192.168.1.1")])
        vpn = dict(ifname="tailscale0", defaultRoute=False,
                   searchDomains=[dict(name="internal.example", routeOnly=True)],
                   servers=[dict(addressString="100.100.100.100")])
        self.assertEqual(network.default_dns_routes([wifi, vpn]), ["192.168.1.1"])

    def test_kernel_counters_keep_receive_and_transmit_separate(self):
        counters = network.parse_counters(
            "Inter-| Receive | Transmit\n"
            " wlan0: 4096 10 0 0 0 0 0 0 1024 5 0 0 0 0 0 0\n"
            " eno1: 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0\n"
        )
        self.assertEqual(counters["wlan0"], dict(rx=4096, tx=1024))
        self.assertEqual(counters["eno1"], dict(rx=0, tx=0))

    def test_rates_use_measured_elapsed_time(self):
        sample = network.traffic_sample({"wlan0": dict(rx=5000, tx=2000)},
                                        {"wlan0": dict(rx=1000, tx=1000)}, 2)
        self.assertEqual(sample["wlan0"], dict(rx=5000, tx=2000, rxRate=2000, txRate=500))

    def test_first_sample_does_not_present_lifetime_total_as_speed(self):
        sample = network.traffic_sample({"wlan0": dict(rx=5000, tx=2000)}, {}, 1)
        self.assertIsNone(sample["wlan0"]["rxRate"])
        self.assertEqual(sample["wlan0"]["rx"], 5000)

    def test_counter_reset_and_zero_interval_do_not_generate_negative_rates(self):
        for elapsed in (0, 1):
            sample = network.traffic_sample({"wlan0": dict(rx=10, tx=20)},
                                            {"wlan0": dict(rx=5000, tx=2000)}, elapsed)
            self.assertIsNone(sample["wlan0"]["rxRate"])
            self.assertIsNone(sample["wlan0"]["txRate"])

    def test_escaped_ssid_and_bssid(self):
        self.assertEqual(
            network.fields(r"*:Cafe\: A\\B:AA\:BB\:CC\:DD\:EE\:FF:80:WPA2:wlan0"),
            ["*", "Cafe: A\\B", "AA:BB:CC:DD:EE:FF", "80", "WPA2", "wlan0"],
        )

    def test_connected_ap_wins_over_stronger_duplicate(self):
        def fake_nmcli(*args, **kwargs):
            if args == ("radio", "wifi"):
                return "enabled"
            if args[-2:] == ("device", "status"):
                return "wlan0:wifi:connected:Cafe\neno1:ethernet:unavailable:\nlo:loopback:connected:lo"
            return (
                r"*:Cafe:AA\:BB\:CC\:DD\:EE\:01:30:WPA2:wlan0" + "\n" +
                r":Cafe:AA\:BB\:CC\:DD\:EE\:02:90:WPA2:wlan0" + "\n" +
                r":Public:AA\:BB\:CC\:DD\:EE\:03:95:--:wlan0" + "\n" +
                r"::AA\:BB\:CC\:DD\:EE\:04:99:WPA2:wlan0"
            )

        with patch.object(network, "nmcli", side_effect=fake_nmcli):
            state = network.snapshot()
        self.assertEqual(len(state["devices"]), 2)
        self.assertEqual([item["ssid"] for item in state["networks"]], ["Cafe", "Public"])
        self.assertEqual(state["networks"][0]["signal"], 30)
        self.assertEqual(state["networks"][1]["security"], "")

    def test_disabled_radio_does_not_scan(self):
        with patch.object(network, "nmcli", side_effect=["wlan0:wifi:unavailable:", "disabled", "GENERAL.DEVICE:wlan0"]) as command:
            state = network.snapshot()
        self.assertFalse(state["enabled"])
        self.assertEqual(state["networks"], [])
        self.assertEqual(command.call_count, 3)

    def test_dhcp_server_is_distinct_from_gateway_and_dns(self):
        details = network.address_details(
            "GENERAL.DEVICE:wlan0\nIP4.ADDRESS[1]:192.168.1.100/24\n"
            "IP4.GATEWAY:192.168.1.1\nIP4.DNS[1]:192.168.1.2\nIP4.DNS[2]:1.1.1.1\n"
            "DHCP4.OPTION[1]:dhcp_server_identifier = 192.168.1.3\n"
            "IP6.ADDRESS[1]:fe80::1234/64\nGENERAL.DEVICE:eno1\nIP4.GATEWAY:\n"
        )
        self.assertEqual(details["wlan0"]["dhcp"], "192.168.1.3")
        self.assertEqual(details["wlan0"]["gateway"], "192.168.1.1")
        self.assertEqual(details["wlan0"]["dns"], ["192.168.1.2", "1.1.1.1"])
        self.assertEqual(details["wlan0"]["ipv6"], ["fe80::1234/64"])
        self.assertEqual(details["eno1"]["dhcp"], "")

    def test_password_and_shell_characters_only_travel_over_stdin(self):
        secret = "a secret $(touch /tmp/not-a-command) `id` : \\"
        result = subprocess.CompletedProcess([], 0, "connected\n", "")
        with patch.object(network.subprocess, "run", return_value=result) as command:
            network.execute(dict(action="connect", interface="wlan0", bssid="AA:BB:CC:DD:EE:FF", password=secret))
        args, kwargs = command.call_args
        self.assertNotIn(secret, args[0])
        self.assertIn("--ask", args[0])
        self.assertEqual(kwargs["input"], secret + "\n")
        self.assertFalse(kwargs.get("shell", False))

    def test_existing_connection_does_not_prompt_for_a_new_password(self):
        with patch.object(network.subprocess, "run", return_value=subprocess.CompletedProcess([], 0, "", "")) as command:
            network.execute(dict(action="connect", interface="wlan0", bssid="AA:BB:CC:DD:EE:FF", password=""))
        self.assertNotIn("--ask", command.call_args.args[0])
        self.assertEqual(command.call_args.kwargs["input"], "")

    def test_credentials_are_redacted_from_errors(self):
        result = subprocess.CompletedProcess([], 4, "", "Failed: secret-value")
        with patch.object(network.subprocess, "run", return_value=result):
            with self.assertRaisesRegex(RuntimeError, r"Failed: \[redacted\]"):
                network.nmcli("--ask", password="secret-value")

    def test_invalid_targets_and_multiline_passwords_never_run(self):
        base = dict(action="connect", interface="wlan0", bssid="AA:BB:CC:DD:EE:FF")
        for override in ({"bssid": "--help"}, {"interface": "--help"}, {"password": "first\nsecond"}, {"action": "unknown"}):
            with self.subTest(override=override), patch.object(network, "nmcli") as command:
                with self.assertRaises(ValueError):
                    network.execute({**base, **override})
                command.assert_not_called()


class PanelConfigTests(unittest.TestCase):
    def test_native_panel_options_are_validated(self):
        from bartjik_core.config import module
        for provider in ('audio', 'network'):
            module({'provider': provider, 'panel': {'hover': True, 'delay': 300, 'closeDelay': 160}})
            module({'provider': provider, 'panel': {'hover': False}})
            for options in ({'hover': 'true'}, {'delay': -1}, {'closeDelay': 10001}):
                with self.assertRaises(ValueError):
                    module({'provider': provider, 'panel': options})


if __name__ == "__main__":
    unittest.main()
