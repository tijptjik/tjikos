"""Event-driven window pickers cannot turn an observation into an action."""
import tempfile
from pathlib import Path
from rust_parity import Backend, fixture

with tempfile.TemporaryDirectory(prefix="runtjik-observe-") as root:
    env = fixture(root)
    backend = Backend("rust", env, demo=True)
    try:
        backend.request(op="query", query="")
        for entry, count in [("apps:firefox.desktop", 2), ("apps:org.gnome.Calculator.desktop", 0)]:
            response = backend.request(op="activate", id=entry, mode="switch", prepare=True, observe=True)
            assert response["kind"] == "windows" and len(response["results"]) == count, response
            assert not response["auto_select"], response
    finally:
        backend.close()
with tempfile.TemporaryDirectory(prefix="runtjik-observe-fake-") as root:
    env = fixture(root)
    env["TJIK_COMPOSITOR"] = "fake"
    with Path(env["RUNTJIK_CONFIG"]).open("a") as config:
        config.write('\n[overrides."apps:fixture-0000.desktop"]\nwindow_classes=["editor"]\n')
    backend = Backend("rust", env)
    try:
        backend.request(op="query", query="fixture")
        response = backend.request(op="activate", id="apps:fixture-0000.desktop", mode="switch", prepare=True, observe=True)
        assert response["kind"] == "windows" and len(response["results"]) == 1, response
        assert not response["auto_select"] and response["results"][0]["id"] == "fixture-one/window-fern", response
    finally:
        backend.close()
print("PASS: zero/one/multiple window observations never auto-select or launch")
