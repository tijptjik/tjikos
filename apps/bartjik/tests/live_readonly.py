"""Opt-in live probes: print availability/count only, never payloads or credentials."""

# Test-only comparison oracle; production launches the Rust binary.
import sys as _sys
from pathlib import Path as _Path
_sys.path.insert(0, str(_Path(__file__).resolve().parent / "reference"))
_sys.path.insert(0, str(_Path(__file__).resolve().parent / "reference/qml"))

import asyncio, json, os, sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from bartjik_core import providers, workspaces
from bartjik_core.io import Unavailable


async def main():
    report = {}

    async def probe(name, fn, spec):
        try:
            rows = await asyncio.wait_for(fn(spec), 20)
            report[name] = {"state": "verified", "items": len(rows)}
        except Exception as e:
            report[name] = {
                "state": "unavailable",
                "reason": str(e) if isinstance(e, Unavailable) else type(e).__name__,
            }

    await asyncio.gather(
        *(
            probe(name, fn, {})
            for name, fn in providers.REGISTRY.items()
            if name not in ("process", "crypto", "openrouter")
        )
    )
    for name, key in [
        ("crypto", "COINMARKETCAP_API_KEY"),
        ("openrouter", "OPENROUTER_API_KEY"),
    ]:
        if os.environ.get(key):
            await probe(name, providers.REGISTRY[name], {})
        else:
            report[name] = {
                "state": "unavailable",
                "reason": key + " is not exported to this process",
            }
    try:
        snapshot = await workspaces.snapshot()
        report["workspaces"] = {
            "state": "verified",
            "screens": [
                {
                    "connector": m["name"],
                    "items": len(workspaces.view(snapshot, m["name"], {})),
                    "active": [
                        i["id"]
                        for i in workspaces.view(snapshot, m["name"], {})
                        if i["state"]["active"]
                    ],
                    "focused": m["focused"],
                }
                for m in snapshot["monitors"]
            ],
        }
    except Exception as e:
        report["workspaces"] = {"state": "unavailable", "reason": type(e).__name__}
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    asyncio.run(main())
