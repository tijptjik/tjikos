"""Linux adapter protocol tests with private executable fixtures; no host network calls."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

APP = Path(__file__).resolve().parents[1]
BINARY = APP / 'target/release/bartjik-core'
with tempfile.TemporaryDirectory(prefix='bartjik-network-adapter-') as directory:
    root = Path(directory)
    nm = root / 'nmcli'
    nm.write_text('#!' + sys.executable + '\n' + r'''
import json, os, sys
from pathlib import Path
a = sys.argv[1:]
with open(os.environ['NETWORK_COMMANDS'], 'a') as f:
    f.write(json.dumps(a) + '\n')
if os.environ.get('NETWORK_FAILURE'):
    print('Not authorized: synthetic-sensitive-diagnostic', file=sys.stderr)
    sys.exit(4)
if 'DEVICE,TYPE,STATE,CONNECTION' in a:
    print('wlan-demo:wifi:connected:Demo Wi-Fi\neth-demo:ethernet:disconnected:')
elif a[-2:] == ['radio', 'wifi']:
    print('enabled')
elif 'IN-USE,SSID,BSSID,SIGNAL,SECURITY,DEVICE' in a:
    print(r'*:Demo Wi-Fi:02\:00\:00\:00\:00\:01:94:WPA2:wlan-demo')
    print(r':Guest\:Wi-Fi:02\:00\:00\:00\:00\:02:78:WPA2:wlan-demo')
elif a[-2:] == ['device', 'show']:
    print('GENERAL.DEVICE:wlan-demo\nIP4.ADDRESS[1]:192.0.2.20/24\nIP4.GATEWAY:192.0.2.1\nIP4.DNS[1]:192.0.2.1')
elif '--ask' in a:
    assert 'synthetic-password' not in ' '.join(a)
    assert sys.stdin.read() == 'synthetic-password\n'
    Path(os.environ['NETWORK_SECRET_RECEIPT']).write_text('stdin only')
''')
    nm.chmod(0o700)
    resolver = root / 'resolvectl'
    resolver.write_text('#!' + sys.executable + '\nprint(\'[]\')\n')
    resolver.chmod(0o700)
    env = {**os.environ, 'PATH': str(root), 'NETWORK_COMMANDS': str(root / 'commands'), 'NETWORK_SECRET_RECEIPT': str(root / 'secret')}

    def request(value, **extra):
        result = subprocess.run([str(BINARY), 'helper', 'network'], input=json.dumps(value), text=True, capture_output=True, env={**env, **extra}, timeout=10, check=True)
        assert not result.stderr, result.stderr
        return json.loads(result.stdout)

    assert request({})['ok'], 'legacy empty request must remain a status query'
    status = request({'action': 'status'})
    assert status['ok'] and status['adapter'] == 'network-manager-cli'
    assert status['devices'][0]['addresses']['ip'] == ['192.0.2.20/24']
    assert status['networks'][1]['ssid'] == 'Guest:Wi-Fi'
    assert status['networks'][1]['bssid'] == '02:00:00:00:00:02'
    for action in [{'action': 'scan'}, {'action': 'radio', 'enabled': False}, {'action': 'ethernet', 'interface': 'eth-demo'}, {'action': 'disconnect', 'interface': 'wlan-demo'}, {'action': 'connect', 'interface': 'wlan-demo', 'bssid': '02:00:00:00:00:02', 'password': 'synthetic-password'}]:
        assert request(action)['ok'], action['action']
    assert (root / 'secret').read_text() == 'stdin only'
    before = (root / 'commands').read_text()
    invalid = request({'action': 'connect', 'interface': 'wlan-demo', 'bssid': '--invalid', 'password': 'synthetic-password'})
    assert not invalid['ok']
    assert (root / 'commands').read_text() == before
    failure = request({'action': 'scan'}, NETWORK_FAILURE='1')
    assert not failure['ok'] and 'permission denied' in failure['error']
    assert 'sensitive-diagnostic' not in json.dumps(failure)
    assert 'synthetic-password' not in (root / 'commands').read_text()
print('PASS: private nmcli/resolvectl fixtures, normalized state, Wi-Fi/Ethernet actions, stdin secrets, denied authorization')
