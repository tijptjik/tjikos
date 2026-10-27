"""A real GTK window that records a graceful-close request and finishes cleanup."""
import json
from pathlib import Path
import sys
import gi
gi.require_version('Gtk','3.0')
from gi.repository import Gtk,Gio,GLib

identity,title,marker=sys.argv[1:]
GLib.set_prgname(identity)
GLib.set_application_name(title)
application=Gtk.Application(application_id=identity,flags=Gio.ApplicationFlags.NON_UNIQUE)

def activate(app):
    window=Gtk.ApplicationWindow(application=app,title=title)
    window.set_default_size(640,360)
    window.add(Gtk.Label(label=title+'\n\nSynthetic showcase application'))
    def close(*_):
        with Path(marker).open('a') as output:
            output.write(json.dumps({'event':'graceful-close','application':identity,'title':title})+'\n')
        # Model cooperative app cleanup; no signals are used to close this window.
        GLib.timeout_add(650,lambda:(app.quit(),False)[1])
        return True
    window.connect('delete-event',close)
    window.show_all()
application.connect('activate',activate)
raise SystemExit(application.run([]))
