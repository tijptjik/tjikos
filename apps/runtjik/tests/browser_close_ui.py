"""Verify browser close routing against a synthetic native window."""
import sys
import subprocess
from pathlib import Path
from unittest.mock import Mock, patch
from private_compositor import private_compositor, wait_for
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import browser_host
import runtjik

with private_compositor():
    process = subprocess.Popen(['kitty', '--class', 'runtjik-close-fixture', '--title',
                                '[runtjik-fixture] Synthetic browser window', 'sleep', '60'],
                               stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        client = wait_for(lambda: next((w for w in runtjik.windows()
                                       if w.get('class') == 'runtjik-close-fixture'), None))
        with patch.object(browser_host.uuid, 'uuid4', return_value=Mock(hex='fixture')):
            browser_host.browser_action({'op': 'close', 'window': 1, 'tab': None}, lambda message: {})
        wait_for(lambda: not any(w['address'] == client['address'] for w in runtjik.windows()))
        print('PASS: exact marked native window closed through Hyprland')
    finally:
        if process.poll() is None:
            process.terminate()
        process.wait(timeout=5)
