"""Exercise graceful close, persisted escalation and Ctrl+Enter with synthetic apps."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time

app = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(app))
sys.path.insert(0, os.environ.get('RUNTJIK_TEST_BARTJIK', str(app.parent/'bartjik')))
from demo_cycle import private_compositor
import runtjik as r


def wait(check):
    for _ in range(160):
        try:
            value = check()
            if value:
                return value
        except (ValueError, subprocess.CalledProcessError):
            pass
        time.sleep(.05)
    raise AssertionError('Synthetic close test timed out')


with private_compositor():
    directory = Path(os.environ['XDG_CONFIG_HOME']).parent
    os.environ['XDG_STATE_HOME'] = str(directory/'state')
    os.environ['RUNTJIK_CONFIG'] = str(directory/'absent.conf')
    wait(lambda: json.loads(subprocess.check_output(['hyprctl', '-j', 'monitors'], text=True)))
    time.sleep(.5)
    fixture = directory/'fixture.py'
    fixture.write_text('''import gi,signal,sys
from pathlib import Path
gi.require_version('Gtk','3.0')
from gi.repository import Gtk,Gio,GLib
mode,marker=sys.argv[1:]
app=Gtk.Application(application_id='hk.type.RuntjikCloseQA',flags=Gio.ApplicationFlags.NON_UNIQUE)
def record(message):
    with Path(marker).open('a') as output:output.write(message+'\\n')
def close(*args):
    record('graceful')
    if mode=='cooperative':app.quit()
    return True
def activate(app):
    window=Gtk.ApplicationWindow(application=app,title='Synthetic shutdown fixture')
    window.set_default_size(320,180)
    window.connect('delete-event',close)
    window.show_all()
app.connect('activate',activate)
signal.signal(signal.SIGTERM,lambda *args:record('SIGTERM'))
GLib.timeout_add(100,lambda:True)
app.run([])
''')
    processes = []
    def clients():
        return json.loads(subprocess.check_output(['hyprctl', '-j', 'clients'], text=True))
    def start(mode):
        marker = directory/(mode+'-'+str(len(processes))+'.txt')
        process = subprocess.Popen(['/usr/bin/python3', '-B', str(fixture), mode, str(marker)],
                                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        processes.append(process)
        client = wait(lambda: next((c for c in clients() if c.get('pid') == process.pid), None))
        return process, marker, client
    def request(client, mode='close'):
        entry = r.Entry('apps:close-qa.desktop', 'Close QA', data={'classes': [client['class']]})
        launcher = r.Launcher(demo=True)
        launcher.demo = False
        launcher.entries = [entry]
        launcher.settings['history']['enabled'] = False
        launcher.store = r.Store(directory/'close.sqlite3')
        try:
            return launcher.activate(entry.id, mode)
        finally:
            launcher.store.db.close()
    try:
        cooperative, marker, client = start('cooperative')
        result = request(client)
        wait(lambda: cooperative.poll() is not None)
        assert marker.read_text().splitlines() == ['graceful']
        assert 'graceful close' in result['message']
        print('PASS: graceful compositor close reaches the app', flush=True)
        stubborn, marker, client = start('stubborn')
        unrelated = subprocess.Popen(['sleep', '60'])
        processes.append(unrelated)
        request(client)
        wait(lambda: marker.exists() and 'graceful' in marker.read_text())
        assert stubborn.poll() is None
        result = request(client, 'launch')
        wait(lambda: 'SIGTERM' in marker.read_text())
        assert stubborn.poll() is None and 'SIGTERM' in result['message']
        result = request(client, 'launch')
        wait(lambda: stubborn.poll() is not None)
        assert stubborn.returncode == -9 and 'SIGKILL' in result['message']
        assert unrelated.poll() is None
        print('PASS: reopening and launching retries SIGTERM, then SIGKILL; unrelated process survives', flush=True)
        restarted, marker, client = start('stubborn')
        result = request(client)
        wait(lambda: marker.exists() and 'graceful' in marker.read_text())
        assert marker.read_text().splitlines() == ['graceful']
        assert 'graceful close' in result['message']
        print('PASS: restarted app begins with graceful close again', flush=True)
        home = directory/'demo'
        home.mkdir()
        shutil.copytree(app/'qml', home/'qml')
        shutil.copy2(app/'runtjik.py', home/'runtjik.py')
        os.environ.update(RUNTJIK_HOME=str(home), RUNTJIK_DEMO='1')
        ipc = ['quickshell', 'ipc', '--path', str(home/'qml'), 'call', 'runtjik']
        def call(*args):
            return subprocess.check_output(ipc+list(args), text=True, stderr=subprocess.DEVNULL)
        def status():
            return json.loads(call('status'))
        with (directory/'qml.log').open('w') as log:
            shell = subprocess.Popen(['quickshell', '--path', str(home/'qml'), '--no-duplicate'], stdout=log, stderr=log)
            processes.append(shell)
            wait(lambda: shell.poll() is not None or status()['ready'])
            assert shell.poll() is None, (directory/'qml.log').read_text()
            call('test', 'Firefox')
            subprocess.run(['wtype', '-M', 'ctrl', '-k', 'Return', '-m', 'ctrl'], check=True)
            wait(lambda: 'Demo: graceful close request for Firefox' in status()['message'])
            subprocess.run(['wtype', '-k', 'Return'], check=True)
            wait(lambda: status()['message'] == 'Demo action completed')
            call('close')
            shell.wait(timeout=5)
        print('PASS: Ctrl+Enter survives pending search; ordinary Enter remains a launch in demo mode', flush=True)
        # Real UI action against the stubborn synthetic fixture: no demo shortcuts.
        data = directory/'applications-data'
        desktop = data/'applications/close-ui.desktop'
        desktop.parent.mkdir(parents=True)
        desktop.write_text('[Desktop Entry]\nType=Application\nName=Shutdown Fixture\nExec=/bin/true\nStartupWMClass='+client['class']+'\n')
        os.environ.update(XDG_DATA_HOME=str(data), XDG_DATA_DIRS=str(data), RUNTJIK_DEMO='0')
        Path(os.environ['RUNTJIK_CONFIG']).write_text('[index]\npath_commands=false\n')
        with (directory/'close-ui.log').open('w') as log:
            shell = subprocess.Popen(['quickshell', '--path', str(home/'qml'), '--no-duplicate'], stdout=log, stderr=log)
            processes.append(shell)
            wait(lambda: shell.poll() is not None or status()['ready'])
            assert shell.poll() is None, (directory/'close-ui.log').read_text()
            subprocess.run(['wtype', '/running shutdown'], check=True)
            wait(lambda: status()['results']==1)
            subprocess.run(['wtype', '-M', 'ctrl', '-k', 'Return', '-m', 'ctrl'], check=True)
            wait(lambda: status()['closing'])
            assert status()['visible'] and not status()['pinned']
            time.sleep(.5)
            assert status()['closing'] and status()['visible']
            restarted.kill()
            restarted.wait(timeout=5)
            # Another fixture shares this class; close all fixture windows to empty the filter.
            for process in processes:
                if process is not shell and process.poll() is None:
                    process.kill();process.wait(timeout=5)
            wait(lambda: status()['results']==0 and not status()['closing'])
            assert status()['visible']
            call('quit');shell.wait(timeout=5)
        assert 'Invalid backend response' not in (directory/'close-ui.log').read_text()
        print('PASS: real close keeps unpinned launcher visible, tracks pending windows and removes closed running apps', flush=True)

    finally:
        for process in processes:
            if process.poll() is None:
                process.kill()
            process.wait(timeout=5)
        print('Synthetic test logs:', directory, flush=True)
