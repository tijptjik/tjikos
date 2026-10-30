from pathlib import Path


def icon(name):
    return str(
        Path(__file__).resolve().parents[3]
        / "qml"
        / "icons"
        / (name + ".svg")
    )
