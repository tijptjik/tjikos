"""Synthesize QML mouse events into a private test surface, never desktop commands."""

import json, os, shutil, subprocess, sys, tempfile, time
from pathlib import Path
from render import wait_for

ROOT = Path(__file__).resolve().parents[1]


def main():
    if not os.environ.get("DBUS_SESSION_BUS_ADDRESS", "").startswith("unix:path=/tmp/"):
        raise RuntimeError("private bus required")
    path = None

    def event(kind):
        r = subprocess.run(
            [
                "quickshell",
                "ipc",
                "-p",
                str(path),
                "call",
                "interaction",
                "event",
                kind,
            ],
            text=True,
            capture_output=True,
        )
        if r.returncode:
            raise ValueError("not ready")
        return json.loads(r.stdout)

    with tempfile.TemporaryDirectory() as td:
        shutil.copytree(ROOT / "qml", Path(td) / "qml")
        path = Path(td) / "qml/Interaction.qml"
        source = (ROOT / "tests/Interaction.qml").read_text().replace('"../qml"', '"."')
        source = (
            source.replace("PanelWindow {", "FloatingWindow {")
            .replace("anchors{top:true;left:true}", "")
            .replace(";exclusionMode:ExclusionMode.Ignore", "")
            .replace(
                "WlrLayershell.namespace:'bartjik-interaction-fixture'", "visible:true"
            )
        )
        if "--wayland" in sys.argv:
            source = (
                (ROOT / "tests/Interaction.qml").read_text().replace('"../qml"', '"."')
            )
        path.write_text(source)
        with (Path(td) / "log").open("w") as log:
            p = subprocess.Popen(
                ["quickshell", "-p", str(path)],
                stdout=log,
                stderr=log,
                env={
                    **os.environ,
                    "QT_QPA_PLATFORM": "wayland"
                    if "--wayland" in sys.argv
                    else "offscreen",
                },
            )
            try:
                wait_for(lambda: event("status") == [])
                time.sleep(0.2)
                for click, label in [
                    ("left", "icon"),
                    ("right", "right"),
                    ("middle", "middle"),
                    ("text", "text"),
                    ("double", "double"),
                    ("up", "up"),
                    ("down", "down"),
                ]:
                    event("reset")
                    event(click)
                    time.sleep(0.2)
                    rows = event("status")
                    actions = [
                        r["action"][0]
                        for r in rows
                        if r["action"][0] not in ("enter", "leave")
                    ]
                    assert actions == [label], (click, rows)
                    ctx = next(r["context"] for r in rows if r["action"][0] == label)
                    assert (
                        ctx["monitor"] == "DP-2"
                        and ctx["item"] == "coin"
                        and ctx["part"] == ("text" if click == "text" else "icon")
                    ), ctx
                    assert ctx["anchor"]["x"] >= 20 and ctx["anchor"]["width"] >= 24, (
                        ctx
                    )

                # Qt can defer a hover update until the next input delivery.
                def moved(kind, inside):
                    event(kind)
                    return event("debug")[0]["hovered"] == inside

                wait_for(lambda: moved("leave", False))
                event("reset")
                event("enter")
                wait_for(lambda: any(r["action"] == ["enter"] for r in event("status")))
                event("move")
                time.sleep(0.1)
                wait_for(lambda: moved("leave", False))
                wait_for(lambda: any(r["action"] == ["leave"] for r in event("status")))
                assert [r["action"][0] for r in event("status")] == [
                    "enter",
                    "leave",
                ], event("status")

                # The island-wide preview handler must allow ordinary item tooltips.
                event("tooltip-enter")
                wait_for(lambda: event("tooltip-debug")[0]["visible"])
                tooltip = event("tooltip-debug")[0]
                assert tooltip["hovered"] and tooltip["text"] == "Notifications : ON", tooltip
            finally:
                p.terminate()
                p.wait(timeout=5)
        errors = [
            l
            for l in (Path(td) / "log").read_text().splitlines()
            if any(s in l for s in ("WARN scene", "ERROR", "Traceback"))
        ]
        if errors:
            raise AssertionError("\n".join(errors))
    print(
        "Real QML icon/text, all mouse buttons, double-click disambiguation, wheel, hover transitions, island tooltips and anchor context passed"
    )


if __name__ == "__main__":
    main()
