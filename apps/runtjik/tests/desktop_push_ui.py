"""A synthetic native window updates launcher counts without a timer or query."""
import json
import os
from pathlib import Path
import subprocess
from gtk_ui import APP, silent_compositor
from private_compositor import wait_for

with silent_compositor():
    root = Path(os.environ["XDG_RUNTIME_DIR"]).parent
    data = root / "data"
    (data / "applications").mkdir(parents=True, exist_ok=True)
    (root / "system").mkdir()
    script = root / "must-not-launch.sh"
    script.write_text(f'touch "{root / "unexpected-launch"}"\n')
    (data / "applications/event.desktop").write_text(f'[Desktop Entry]\nType=Application\nName=Event Fixture\nExec=/bin/sh {script}\nStartupWMClass=org.tjikos.AdapterFixture\n')
    config = root / "runtjik.conf"
    config.write_text('[index]\npath_commands=false\nrefresh_seconds=300\n[plugins]\nenabled=[]\n[launch]\nisolate=false\n[ui]\nauto_launch_applications=false\nauto_launch_commands=false\n[theme]\nicon_theme="Adwaita"\n')
    env = {**os.environ, "XDG_DATA_HOME": str(data), "XDG_DATA_DIRS": str(root / "system"),
           "XDG_STATE_HOME": str(root / "state"), "RUNTJIK_HOME": str(APP / "dist"),
           "RUNTJIK_CONFIG": str(config), "GTK_A11Y": "none", "GSK_RENDERER": "cairo"}
    source = root / "window.c"
    source.write_text('''#include <gtk/gtk.h>
static void activate(GtkApplication *app,gpointer data) {
    GtkWidget *w=gtk_application_window_new(app);gtk_window_set_title(GTK_WINDOW(w),"Synthetic push fixture");
    gtk_window_set_default_size(GTK_WINDOW(w),300,180);gtk_window_present(GTK_WINDOW(w));
}
int main(int argc,char **argv) {GtkApplication *a=gtk_application_new("org.tjikos.AdapterFixture",G_APPLICATION_NON_UNIQUE);
    g_signal_connect(a,"activate",G_CALLBACK(activate),NULL);return g_application_run(G_APPLICATION(a),argc,argv);}
''')
    window = root / "window"
    flags = subprocess.check_output(["pkg-config", "--cflags", "--libs", "gtk4"], text=True).split()
    subprocess.run(["cc", str(source), "-o", str(window), *flags], check=True)
    binary = APP / "dist/runtjik-gtk"
    def run(action):
        return json.loads(subprocess.check_output([str(binary), action], env=env, text=True) or '{}')
    def count():
        state = run("status")
        assert state.get("query") == "event", state
        rows = [r for r in state["rows"] if r["id"] == "apps:event.desktop"]
        return rows[0]["windows"] if rows else -1
    fixture = None
    try:
        run("show")
        subprocess.run(["wtype", "event"], env=env, check=True)
        wait_for(lambda: run("status").get("resultQuery") == "event")
        wait_for(lambda: count() == 0)
        before = run("status")["desktop_sequence"]
        fixture = subprocess.Popen([str(window)], env=env)
        wait_for(lambda: count() == 1, timeout=4)
        assert run("status")["desktop_sequence"] > before
        fixture.terminate()
        fixture.wait(timeout=5)
        wait_for(lambda: count() == 0, timeout=4)
        assert not (root / "unexpected-launch").exists()
        run("quit")
        run("quit")
        wait_for(lambda: not run("status").get("running"))
        print("PASS: pushed window open/close updates GTK counts, preserves query, performs no launch, and accepts repeated quit")
    finally:
        run("quit")
        if fixture and fixture.poll() is None:
            fixture.terminate()
            fixture.wait(timeout=5)
