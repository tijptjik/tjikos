"""Visible, cancellable pre-recording countdown on the selected Wayland monitor."""
import json
import signal
import sys


def countdown(seconds, area=None, output=None):
    import gi
    gi.require_version('Gtk', '3.0')
    gi.require_version('Gdk', '3.0')
    gi.require_version('GtkLayerShell', '0.1')
    from gi.repository import Gtk, Gdk, GLib, GtkLayerShell

    cancelled = False
    window = Gtk.Window()
    window.set_title('Runtjik recording countdown')
    window.set_default_size(220, 200)
    GtkLayerShell.init_for_window(window)
    GtkLayerShell.set_namespace(window, 'runtjik-countdown')
    GtkLayerShell.set_layer(window, GtkLayerShell.Layer.OVERLAY)
    GtkLayerShell.set_keyboard_mode(window, GtkLayerShell.KeyboardMode.EXCLUSIVE)
    display = Gdk.Display.get_default()
    monitor = display.get_primary_monitor() or display.get_monitor(0)
    if area:
        import re
        x, y, width, height = map(int, re.fullmatch(r'(-?\d+),(-?\d+) (\d+)x(\d+)', area).groups())
        monitor = display.get_monitor_at_point(x + width // 2, y + height // 2)
    elif output:
        import subprocess
        monitors = json.loads(subprocess.check_output(['hyprctl', '-j', 'monitors']))
        target = next((item for item in monitors if item['name'] == output), None)
        if target:
            monitor = display.get_monitor_at_point(target['x'] + 1, target['y'] + 1)
    GtkLayerShell.set_monitor(window, monitor)
    box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=4)
    box.set_border_width(20)
    box.pack_start(Gtk.Label(label='Recording starts in'), False, False, 0)
    label = Gtk.Label(label=str(seconds))
    label.set_name('countdown-number')
    box.pack_start(label, True, True, 0)
    box.pack_start(Gtk.Label(label='Esc to cancel'), False, False, 0)
    window.add(box)
    css = Gtk.CssProvider()
    css.load_from_data(b'window {background:#26233a;color:#e0def4;border:2px solid #ff53a6;border-radius:20px;} #countdown-number {font-size:80px;color:#ff53a6;}')
    Gtk.StyleContext.add_provider_for_screen(window.get_screen(), css, Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION)

    def cancel(*_):
        nonlocal cancelled
        cancelled = True
        window.destroy()
        return False

    def key(_window, event):
        if event.keyval == Gdk.KEY_Escape:
            cancel()
            return True
        return False

    def tick():
        nonlocal seconds
        seconds -= 1
        if seconds <= 0:
            window.destroy()
            return False
        label.set_text(str(seconds))
        return True

    window.connect('key-press-event', key)
    window.connect('destroy', Gtk.main_quit)
    GLib.unix_signal_add(GLib.PRIORITY_DEFAULT, signal.SIGTERM, cancel)
    GLib.unix_signal_add(GLib.PRIORITY_DEFAULT, signal.SIGINT, cancel)
    GLib.timeout_add_seconds(1, tick)
    window.show_all()
    Gtk.main()
    return 2 if cancelled else 0


if __name__ == '__main__':
    spec = json.loads(sys.argv[1])
    sys.exit(countdown(spec['countdown_seconds'], spec.get('area'), spec.get('output')))
