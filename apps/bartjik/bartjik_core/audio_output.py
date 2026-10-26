"""Audio route identity shared by the bar provider and its actions."""


def is_headphones(sink):
    return (
        sink.get("description") == "USB Audio Speakers"
        or "headphone" in str(sink.get("active_port", "")).lower()
        or "headphone" in sink.get("description", "").lower()
    )


def panel_routes(sinks):
    """Prefer the same USB/DP pair exposed by the audio panel when available."""
    usb = next((s for s in sinks if s.get("description") == "USB Audio Speakers"), None)
    screen = next((s for s in sinks if s.get("description", "").startswith("Navi 21/23 HDMI/DP")), None)
    return [usb, screen] if usb is not None and screen is not None else sinks
