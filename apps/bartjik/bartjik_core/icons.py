from pathlib import Path


def icon(name):
    return str(
        Path(__file__).resolve().parents[1]
        / "qml"
        / "icons"
        / (name + ".svg")
    )
