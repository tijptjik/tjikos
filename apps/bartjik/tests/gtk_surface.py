"""GTK shell on a private compositor; parent is placed silently on workspace 7."""
import json
import os
from pathlib import Path
import subprocess
import sys
import time

APP = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(APP.parent / 'runtjik/tests'))
from gtk_ui import silent_compositor
from private_compositor import wait_for


def main():
    with silent_compositor():
        root = Path(os.environ['XDG_RUNTIME_DIR']).parent
        env = {**os.environ, 'BARTJIK_GTK_SOCKET': str(root / 'bar.sock'),
               'BARTJIK_HOME': str(APP), 'BARTJIK_CONFIG': str(root / 'bar.json'), 'GTK_A11Y': 'none', 'GSK_RENDERER': 'cairo', 'TJIK_COMPOSITOR': 'hyprland'}
        configuration = {"theme": "rose-punk", "layout": {"centre_width": "100%"},
                         "modules": {"spaces": {"provider": "workspaces", "persistent": {"BAR": [8]}},
                                     "clock": {"provider": "datetime", "format": "%H:%M", "alternate": "ALT %H:%M", "timezone": "UTC", "locale": "C", "worldClocks": [{"label": "UTC", "timezone": "UTC"}, {"label": "Hong Kong", "timezone": "Asia/Hong_Kong"}]}},
                         "bars": [{"id": "fixture", "side": "t", "position": "c", "reserve": True,
                                   "offsetX": 0, "offsetY": 0, "islandGap": 80, "islands": [{"id": "spaces", "modules": ["spaces"]}, {"id": "clock", "modules": ["clock"]}]}]}
        (root / 'bar.json').write_text(json.dumps(configuration))
        binary = APP / 'dist/bin/bartjik-gtk'
        def hypr(*args):
            return subprocess.check_output(['hyprctl', *args], env=env, text=True)
        hypr('output', 'create', 'headless', 'BAR')
        hypr('eval', 'hl.monitor({output="WAYLAND-1",disabled=true}); hl.monitor({output="BAR",mode="1000x680@60",position="0x0",scale=1}); hl.dispatch(hl.dsp.focus({monitor="BAR"}))')
        time.sleep(.5)
        def run(action):
            return json.loads(subprocess.check_output([str(binary), action], env=env, text=True))
        fixture = root / 'fixture.c'
        fixture.write_text('#include <gtk/gtk.h>\n#include <stdio.h>\nstatic void hit(GtkGestureClick*g,int n,double x,double y,gpointer p){FILE*f=fopen(p,"w");if(f){fputs("clicked",f);fclose(f);}}\nint main(int n,char **v){gtk_init();GtkWidget*w=gtk_window_new();gtk_window_set_decorated(GTK_WINDOW(w),FALSE);gtk_window_set_title(GTK_WINDOW(w),v[1]);gtk_window_set_default_size(GTK_WINDOW(w),400,220);GtkGesture*g=gtk_gesture_click_new();g_signal_connect(g,"pressed",G_CALLBACK(hit),v[2]);gtk_widget_add_controller(w,GTK_EVENT_CONTROLLER(g));gtk_window_present(GTK_WINDOW(w));g_main_loop_run(g_main_loop_new(NULL,FALSE));}')
        flags = subprocess.check_output(['pkg-config', '--cflags', '--libs', 'gtk4'], text=True).split()
        subprocess.run(['cc', str(fixture), '-o', str(root / 'fixture'), *flags], check=True)
        sources = APP.parent / 'runtjik/tests'
        subprocess.run(['wayland-scanner', 'client-header', str(sources / 'virtual_pointer.xml'), str(root / 'pointer.h')], check=True)
        subprocess.run(['wayland-scanner', 'private-code', str(sources / 'virtual_pointer.xml'), str(root / 'protocol.c')], check=True)
        flags = subprocess.check_output(['pkg-config', '--cflags', '--libs', 'wayland-client'], text=True).split()
        pointer_source = (sources / 'pointer.c').read_text().replace("else if(op=='s'", "else if(op=='c' && scanf(\"%u %u\",&x,&state)==2)zwlr_virtual_pointer_v1_button(p,ms,x,state);\n  else if(op=='s'")
        (root / 'pointer.c').write_text(pointer_source)
        subprocess.run(['cc', str(root / 'pointer.c'), str(root / 'protocol.c'), '-I'+str(root), '-o', str(root / 'pointer'), *flags], check=True)
        pointer = subprocess.Popen([str(root / 'pointer'), 'BAR'], env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
        def input_event(command):
            pointer.stdin.write(command+'\n')
            pointer.stdin.flush()
            assert pointer.stdout.readline().strip() == 'ok'
        clients = [subprocess.Popen([str(root / 'fixture'), 'Bar fixture '+name, str(root / 'fixture-click')], env=env) for name in ['one', 'two']]
        log = (root / 'bar.log').open('w')
        bar = subprocess.Popen([str(binary), 'serve', '--reserve'], env=env, stdout=log, stderr=log)
        try:
            wait_for(lambda: (root / 'bar.sock').exists())
            wait_for(lambda: len(run('status')['bars']) == 1)
            status = run('status')
            assert status['reserve'] and status['bars'][0]['output'] == 'BAR', status
            wait_for(lambda: run('status')['bars'][0]['width'] > 100)
            monitors = json.loads(hypr('-j', 'monitors'))
            assert monitors[0]['reserved'][1] > 0, monitors
            # Select an unoccupied configured workspace through actual pointer input.
            def click(x, y):
                input_event(f'm {int(x*10)} {int(y*10000/680)}')
                time.sleep(.15)
                input_event('b 1')
                time.sleep(.1)
                input_event('b 0')
            def bar_layer():
                return next(c for m in json.loads(hypr('-j', 'layers')).values() for rows in m['levels'].values() for c in rows if c['namespace'] == 'bartjik-gtk-bar')
            wait_for(lambda: any(i['label'] == '8' for i in run('status')['bars'][0]['workspaces']))
            time.sleep(.3)
            state = run('status')['bars'][0]
            item = next(i for i in state['workspaces'] if i['label'] == '8')
            layer = bar_layer()
            click(layer['x'] + item['x'] + item['width']/2, layer['y'] + item['y'] + item['height']/2)
            try:
                wait_for(lambda: json.loads(hypr('-j', 'activeworkspace'))['id'] == 8)
            except ValueError:
                print('WORKSPACE CLICK', layer, item, run('status'), hypr('-j', 'cursorpos'), hypr('-j', 'layers'), flush=True)
                raise
            # Return to the fixture windows for panel activation coverage.
            hypr('eval', 'hl.dispatch(hl.dsp.focus({workspace="2"}))')
            time.sleep(.3)
            run('panel')
            wait_for(lambda: run('status')['bars'][0]['panel'])
            time.sleep(.6)
            subprocess.run(['grim', '-o', 'BAR', str(root / 'gtk-bar-panel.png')], env=env, check=True)
            wait_for(lambda: len(run('status')['bars'][0]['items']) == 2)
            item = next(i for i in run('status')['bars'][0]['items'] if i['label'] == 'Bar fixture one')
            def panel_layer():
                return next(c for m in json.loads(hypr('-j', 'layers')).values() for rows in m['levels'].values() for c in rows if c['namespace'] == 'bartjik-gtk-panel')
            panel = panel_layer()
            x, y = panel['x'] + item['x'] + item['width']/2, panel['y'] + item['y'] + item['height']/2
            input_event(f'm {int(x*10)} {int(y*10000/680)}')
            input_event('b 1')
            input_event('b 0')
            wait_for(lambda: json.loads(hypr('-j', 'activewindow')).get('title') == 'Bar fixture one')
            run('hide-panel')
            wait_for(lambda: not run('status')['bars'][0]['panel'])
            layers = json.loads(hypr('-j', 'layers'))
            assert not any(c['namespace'] == 'bartjik-gtk-panel' for m in layers.values() for rows in m['levels'].values() for c in rows), layers
            # Shared datetime provider: real button input toggles the alternate format.
            wait_for(lambda: run('status')['bars'][0]['clocks'][0]['state'] == 'ready')
            state = run('status')['bars'][0]
            clock = state['clocks'][0]
            assert run('status')['clockProviders'] == 1
            bounds = clock['bounds']
            layer = bar_layer()
            click(layer['x'] + bounds['x'] + bounds['width']/2, layer['y'] + bounds['y'] + bounds['height']/2)
            wait_for(lambda: run('status')['bars'][0]['clocks'][0]['alternate'])
            assert run('status')['bars'][0]['clocks'][0]['text'].startswith('ALT ')
            # Right-click opens the world-clock card without changing the text mode.
            time.sleep(1.2)
            state = run('status')['bars'][0]
            bounds = state['clocks'][0]['bounds']
            layer = bar_layer()
            input_event(f"m {int((layer['x'] + bounds['x'] + bounds['width']/2)*10)} {int((layer['y']+22)*10000/680)}")
            input_event('c 273 1')
            input_event('c 273 0')
            wait_for(lambda: run('status')['bars'][0]['clocks'][0]['panel'])
            rows = run('status')['bars'][0]['clocks'][0]['rows']
            assert [r['label'] for r in rows] == ['UTC', 'Hong Kong'], rows
            assert (int(rows[1]['value'][:2]) - int(rows[0]['value'][:2])) % 24 == 8, rows
            time.sleep(.4)
            subprocess.run(['grim', '-o', 'BAR', str(root / 'gtk-world-clock.png')], env=env, check=True)
            run('hide-panel')
            wait_for(lambda: not run('status')['bars'][0]['clocks'][0]['panel'])
            configuration['modules']['clock']['format'] = 'bad\nformat'
            (root / 'bar.json').write_text(json.dumps(configuration))
            run('reload')
            wait_for(lambda: run('status')['bars'][0]['clocks'][0]['state'] == 'error')
            configuration['modules']['clock']['format'] = '%H:%M'
            (root / 'bar.json').write_text(json.dumps(configuration))
            run('reload')
            wait_for(lambda: run('status')['bars'][0]['clocks'][0]['state'] == 'ready')
            # Reload the same configured island on every edge and verify reservation.
            for side, reserved_index in [('b', 3), ('l', 0), ('r', 2), ('t', 1)]:
                configuration['bars'][0]['side'] = side
                (root / 'bar.json').write_text(json.dumps(configuration))
                run('reload')
                wait_for(lambda: run('status')['bars'][0]['side'] == side)
                wait_for(lambda: json.loads(hypr('-j', 'monitors'))[0]['reserved'][reserved_index] > 0)
                time.sleep(.2)
                layer = bar_layer()
                assert layer['w'] > 0 and layer['h'] > 0, layer
                if side == 'b':
                    assert layer['y'] + layer['h'] >= 678, layer
                if side == 'r':
                    assert layer['x'] + layer['w'] >= 998, layer
            # Remove reservation so a tiled fixture lies under the island gap.
            configuration['bars'][0]['reserve'] = False
            (root / 'bar.json').write_text(json.dumps(configuration))
            run('reload')
            hypr('eval', 'hl.config({general={gaps_in=0,gaps_out=0}})')
            wait_for(lambda: json.loads(hypr('-j', 'monitors'))[0]['reserved'][1] == 0)
            time.sleep(.5)
            state = run('status')['bars'][0]
            islands = state['islands']
            layer = bar_layer()
            gap_x = (islands[0]['x'] + islands[0]['width'] + islands[1]['x']) / 2
            marker = root / 'fixture-click'
            marker.unlink(missing_ok=True)
            click(layer['x'] + gap_x, layer['y'] + state['height']/2)
            wait_for(marker.exists)
            # A new output creates its own bar without restarting the provider.
            hypr('output', 'create', 'headless', 'SECOND')
            hypr('eval', 'hl.monitor({output="SECOND",mode="800x600@60",position="1000x0",scale=1})')
            wait_for(lambda: len(run('status')['bars']) == 2)
            assert run('status')['clockProviders'] == 1
            hypr('output', 'remove', 'SECOND')
            wait_for(lambda: len(run('status')['bars']) == 1)
            # Auto-hide observes the adapter pointer without an input-catching edge strip.
            configuration['bars'][0].update(autoHide=True, activationDelay=250,
                hideDelay=180, fadeDuration=100, keepVisibleDistance='64px', reserve=True)
            (root / 'bar.json').write_text(json.dumps(configuration))
            run('reload')
            input_event('m 1000 5000')
            wait_for(lambda: run('status')['bars'][0]['opacity'] == 0)
            assert not run('status')['bars'][0]['inputActive']
            wait_for(lambda: json.loads(hypr('-j', 'monitors'))[0]['reserved'][1] == 0)
            # Clicking where an island was painted reaches the window below it.
            state = run('status')['bars'][0]
            layer = bar_layer()
            marker.unlink(missing_ok=True)
            click(layer['x'] + 20, layer['y'] + 22)
            try:
                wait_for(marker.exists)
            except ValueError:
                print('HIDDEN INPUT', run('status'), hypr('-j', 'clients'), hypr('-j', 'monitors'), flush=True)
                raise
            assert run('status')['bars'][0]['concealed']
            # A brief edge visit cancels; a subsequent continuous dwell reveals.
            input_event('m 1000 15')
            time.sleep(.1)
            input_event('m 1000 5000')
            time.sleep(.3)
            assert run('status')['bars'][0]['concealed']
            input_event('m 1000 15')
            wait_for(lambda: run('status')['bars'][0]['opacity'] == 1)
            wait_for(lambda: json.loads(hypr('-j', 'monitors'))[0]['reserved'][1] > 0)
            # A visible panel holds its bar while the pointer crosses to it.
            run('panel')
            input_event('m 1000 5000')
            time.sleep(.5)
            assert not run('status')['bars'][0]['concealed']
            run('hide-panel')
            wait_for(lambda: run('status')['bars'][0]['opacity'] == 0)
            hypr('output', 'create', 'headless', 'SECOND')
            hypr('eval', 'hl.monitor({output="SECOND",mode="800x600@60",position="1000x0",scale=1})')
            wait_for(lambda: len(run('status')['bars']) == 2)
            assert run('status')['clockProviders'] == 1
            wait_for(lambda: all(b['concealed'] for b in run('status')['bars']))
            input_event('m 1000 15')
            wait_for(lambda: all(b['opacity'] == 1 for b in run('status')['bars']))
            configuration['bars'][0]['syncMonitors'] = False
            (root / 'bar.json').write_text(json.dumps(configuration))
            run('reload')
            wait_for(lambda: next(b for b in run('status')['bars'] if b['output'] == 'SECOND')['opacity'] == 0)
            assert not next(b for b in run('status')['bars'] if b['output'] == 'BAR')['concealed']
            hypr('output', 'remove', 'SECOND')
            wait_for(lambda: len(run('status')['bars']) == 1)
            configuration['bars'][0]['autoHide'] = False
            (root / 'bar.json').write_text(json.dumps(configuration))
            run('reload')
            assert not run('status')['pointerSampling']
            assert run('status')['bars'][0]['opacity'] == 1
            subprocess.run(['grim', '-o', 'BAR', str(root / 'gtk-autohide.png')], env=env, check=True)
            run('quit')
            bar.wait(timeout=5)
            assert bar.returncode == 0
            wait_for(lambda: not (root / 'bar.sock').exists())
            # The fake adapter deliberately has no global-pointer capability.
            configuration['bars'][0]['autoHide'] = True
            (root / 'bar.json').write_text(json.dumps(configuration))
            bar = subprocess.Popen([str(binary), 'serve', '--demo'], env=env, stdout=log, stderr=log)
            wait_for(lambda: (root / 'bar.sock').exists())
            wait_for(lambda: len(run('status')['bars']) == 1)
            wait_for(lambda: 'Unsupported' in run('status')['autoHideError'] or 'unsupported' in run('status')['autoHideError'])
            assert run('status')['bars'][0]['opacity'] == 1
            wait_for(lambda: run('status')['bars'][0]['clocks'][0]['state'] == 'ready')
            assert run('status')['bars'][0]['clocks'][0]['text'] == '12:00'
            run('quit')
            bar.wait(timeout=5)
            assert bar.returncode == 0
            print('PASS: empty workspace selection, four-edge reservation/reload, gap click-through, panel activation, hotplug, auto-hide dwell/fades/input release/sync and quit')
            print('Artifacts:', root)
        finally:
            if bar.poll() is None:
                bar.terminate()
                bar.wait(timeout=5)
            pointer.terminate()
            pointer.wait(timeout=5)
            for client in clients:
                client.terminate()
                client.wait(timeout=5)
            log.close()

if __name__ == '__main__':
    main()
