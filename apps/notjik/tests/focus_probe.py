"""Private GTK typing target for notification focus acceptance."""
import json
from pathlib import Path
import sys
import gi

gi.require_version("Gtk", "4.0")
from gi.repository import Gtk

output = Path(sys.argv[1])
app = Gtk.Application(application_id="org.tjikos.notjik.focusprobe")


def activate(app):
    window = Gtk.ApplicationWindow(application=app, title="Synthetic typing target")
    window.set_default_size(360, 160)
    entry = Gtk.Entry()
    entry.set_placeholder_text("Synthetic keyboard focus test")
    window.set_child(entry)
    def save(*_):
        output.write_text(json.dumps({"text": entry.get_text(), "active": window.is_active()}))
    entry.connect("changed", save)
    window.connect("notify::is-active", save)
    window.present()
    entry.grab_focus()
    save()


app.connect("activate", activate)
app.run([])
