"""Actual adapter actions against owned windows in a private compositor only."""
import json
import os
from pathlib import Path
import subprocess
from gtk_ui import silent_compositor, APP
from private_compositor import wait_for

PROBE = APP.parents[1] / "packages/tjik-desktop/target/release/examples/probe"

with silent_compositor():
    root = Path(os.environ["XDG_RUNTIME_DIR"]).parent
    source = root / "window.c"
    source.write_text('''#include <gtk/gtk.h>
static gboolean closed(GtkWindow *w, gpointer data) {g_main_loop_quit(data);return FALSE;}
int main(int argc,char **argv) {
    gtk_init(); GMainLoop *loop=g_main_loop_new(NULL,FALSE);
    GtkWidget *w=gtk_window_new();gtk_window_set_title(GTK_WINDOW(w),argv[1]);
    gtk_window_set_default_size(GTK_WINDOW(w),300,180);
    gtk_window_set_child(GTK_WINDOW(w),gtk_label_new("Synthetic adapter fixture"));
    g_signal_connect(w,"close-request",G_CALLBACK(closed),loop);
    gtk_window_present(GTK_WINDOW(w));g_main_loop_run(loop);return 0;
}
''')
    binary = root / "window"
    flags = subprocess.check_output(["pkg-config", "--cflags", "--libs", "gtk4"], text=True).split()
    subprocess.run(["cc", str(source), "-o", str(binary), *flags], check=True)
    env = {**os.environ, "TJIK_COMPOSITOR": "hyprland", "GTK_A11Y": "none"}
    def probe(*args, check=True):
        result = subprocess.run([str(PROBE), *args], env=env, text=True, capture_output=True)
        if check:
            assert result.returncode == 0, (args, result.stderr)
        return result
    def windows():
        return json.loads(probe().stdout)["windows"]
    wait_for(lambda: json.loads(probe().stdout)["outputs"])
    first = subprocess.Popen([str(binary), "Adapter fixture one"], env=env)
    second = None
    try:
        target = wait_for(lambda: next((w for w in windows() if w["pid"] == first.pid), None))
        assert target["geometry"]["width"] > 0 and target["output"]
        second = subprocess.Popen([str(binary), "Adapter fixture two"], env=env)
        wait_for(lambda: any(w["pid"] == second.pid for w in windows()))
        probe("activate", target["id"])
        def active():
            return json.loads(subprocess.check_output(["hyprctl", "-j", "activewindow"], env=env, text=True))
        wait_for(lambda: active().get("pid") == first.pid)
        probe("close", target["id"])
        first.wait(timeout=5)
        stale = probe("close", target["id"], check=False)
        assert stale.returncode and "StaleTarget" in stale.stderr, stale
        assert second.poll() is None
        mismatch = {**env, "WAYLAND_DISPLAY": "wrong-display"}
        denied = subprocess.run([str(PROBE)], env=mismatch, text=True, capture_output=True)
        assert denied.returncode and "selected Wayland display" in denied.stderr, denied
        print("PASS: private adapter discovery, activation, close, stale target and mismatched display rejection")
    finally:
        for process in [first, second]:
            if process and process.poll() is None:
                process.terminate()
                process.wait(timeout=5)
