"""Native GTK backend contracts on a private bus; never contact host services."""
import json
import os
from pathlib import Path
import queue
import subprocess
import sys
import tempfile
import threading
import time

APP = Path(__file__).resolve().parents[1]
if '--inside' not in sys.argv:
    raise SystemExit(subprocess.call(['dbus-run-session', '--', '/usr/bin/python3', __file__, '--inside']))
import dbus
import dbus.service
from dbus.mainloop.glib import DBusGMainLoop
from gi.repository import GLib
DBusGMainLoop(set_as_default=True)
bus = dbus.SessionBus(private=True)
calls = []
PLAYER = 'org.mpris.MediaPlayer2.Player'
SNI = 'org.kde.StatusNotifierItem'
WATCHER = 'org.kde.StatusNotifierWatcher'
PROPS = 'org.freedesktop.DBus.Properties'

class Player(dbus.service.Object):
    def __init__(self):
        self.name = dbus.service.BusName('org.mpris.MediaPlayer2.Fixture', bus=bus)
        super().__init__(bus, '/org/mpris/MediaPlayer2')
        self.properties = {'PlaybackStatus': 'Playing', 'CanControl': True, 'CanGoNext': True,
                           'CanGoPrevious': True, 'CanSeek': True, 'CanPlay': True, 'CanPause': True,
                           'Shuffle': False, 'Position': dbus.Int64(2000000),
                           'Metadata': dbus.Dictionary({'xesam:title': 'Fixture track', 'xesam:artist': dbus.Array(['Fixture artist'], signature='s'), 'mpris:trackid': dbus.ObjectPath('/track/one')}, signature='sv')}
    @dbus.service.method(PROPS, in_signature='s', out_signature='a{sv}')
    def GetAll(self, interface):
        return self.properties if interface == PLAYER else {'Identity': 'Fixture', 'DesktopEntry': 'fixture', 'CanRaise': True, 'CanQuit': True}
    @dbus.service.method(PROPS, in_signature='ss', out_signature='v')
    def Get(self, interface, prop):
        return self.GetAll(interface)[prop]
    @dbus.service.method(PROPS, in_signature='ssv')
    def Set(self, interface, prop, value):
        self.properties[prop] = value
        calls.append(('Set', str(prop), bool(value)))
        self.PropertiesChanged(interface, {prop: value}, [])
    @dbus.service.signal(PROPS, signature='sa{sv}as')
    def PropertiesChanged(self, interface, changed, invalidated):
        pass
    @dbus.service.method(PLAYER)
    def Next(self):
        calls.append(('Next',))
    @dbus.service.method(PLAYER)
    def Previous(self):
        calls.append(('Previous',))
    @dbus.service.method(PLAYER)
    def PlayPause(self):
        calls.append(('PlayPause',))
    @dbus.service.method(PLAYER, in_signature='ox')
    def SetPosition(self, track, position):
        calls.append(('SetPosition', str(track), int(position)))

class Item(dbus.service.Object):
    def __init__(self, connection=bus):
        self.name = dbus.service.BusName('org.example.TrayFixture', bus=connection)
        super().__init__(connection, '/StatusNotifierItem')
        self.properties = {'Id': 'FixtureChat', 'Title': 'Synthetic chat', 'IconName': 'mail-unread-symbolic', 'Status': 'Active', 'ItemIsMenu': False, 'Menu': dbus.ObjectPath('/Menu'),
                           'IconPixmap': dbus.Array([(2,2,dbus.ByteArray(bytes([255,255,0,0])*4))],signature='(iiay)')}
    @dbus.service.method(PROPS, in_signature='s', out_signature='a{sv}')
    def GetAll(self, interface):
        return self.properties
    @dbus.service.method(SNI, in_signature='ii')
    def Activate(self, x, y):
        calls.append(('Activate',))
    @dbus.service.method(SNI, in_signature='ii')
    def SecondaryActivate(self, x, y):
        calls.append(('SecondaryActivate',))
    @dbus.service.method(SNI, in_signature='is')
    def Scroll(self, delta, orientation):
        calls.append(('Scroll', int(delta), str(orientation)))
    @dbus.service.signal(SNI)
    def NewStatus(self):
        pass

class Menu(dbus.service.Object):
    def __init__(self):
        super().__init__(bus, '/Menu')
        self.exists = True
    @dbus.service.method('com.canonical.dbusmenu', in_signature='i', out_signature='b')
    def AboutToShow(self, item):
        calls.append(('AboutToShow', int(item)))
        return False
    @dbus.service.method('com.canonical.dbusmenu', in_signature='iias', out_signature='u(ia{sv}av)')
    def GetLayout(self, parent, depth, properties):
        children = []
        if self.exists:
            for i, label, enabled in [(1, 'Toggle', True), (2, 'Disabled', False)]:
                children.append(dbus.Struct((dbus.Int32(i), dbus.Dictionary({'label':label,'enabled':enabled,'toggle-type':'checkmark','toggle-state':dbus.Int32(0)}, signature='sv'),dbus.Array([],signature='v')), signature='ia{sv}av', variant_level=1))
        return dbus.UInt32(1), (dbus.Int32(0),dbus.Dictionary({},signature='sv'),dbus.Array(children,signature='v'))
    @dbus.service.method('com.canonical.dbusmenu', in_signature='isvu')
    def Event(self, item, event, value, timestamp):
        calls.append(('MenuEvent',int(item),str(event)))

player, item, menu = Player(), Item(), Menu()
loop = GLib.MainLoop()
threading.Thread(target=loop.run,daemon=True).start()
with tempfile.TemporaryDirectory(prefix='bartjik-services-') as tmp:
    root=Path(tmp)
    config={'modules':{'music':{'provider':'playerctl','players':['Fixture']},'tray':{'provider':'icontray'}},'bars':[{'id':'fixture','islands':[{'id':'one','modules':['music','tray']}]}]}
    (root/'config.json').write_text(json.dumps(config))
    env={**os.environ,'BARTJIK_HOME':str(APP),'BARTJIK_CONFIG':str(root/'config.json'),'BARTJIK_FRONTEND':'gtk','XDG_CACHE_HOME':str(root/'cache'),'XDG_STATE_HOME':str(root/'state'),'XDG_RUNTIME_DIR':str(root/'runtime')}
    (root/'runtime').mkdir()
    env.pop('HYPRLAND_INSTANCE_SIGNATURE',None)
    p=subprocess.Popen([str(APP/'target/release/bartjik-core'),'serve'],env=env,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
    received=queue.Queue()
    threading.Thread(target=lambda: [received.put(json.loads(line)) for line in p.stdout],daemon=True).start()
    def send(value):
        p.stdin.write(json.dumps(value)+'\n');p.stdin.flush()
    def wait(predicate,timeout=8):
        end=time.monotonic()+timeout
        while time.monotonic()<end:
            try:
                value=received.get(timeout=.1)
                if predicate(value):return value
            except queue.Empty:pass
        raise AssertionError('Native response timed out; calls='+repr(calls))
    def row(state,module):
        if not state.get('bars'):return None
        source=state['bars'][0]['sources'][module]
        rows=state.get('providers',{}).get(source,{}).get('items',[])
        return rows[0] if rows else None
    def action(module, target, event, part='icon'):
        send({'op':'action','context':{'barKey':'SYNTHETIC/fixture','monitor':'SYNTHETIC','module':module,'item':target,'part':part,'event':event}})
    def called(name):
        end=time.monotonic()+5
        while time.monotonic()<end:
            if any(c[0]==name for c in calls):return
            time.sleep(.01)
        raise AssertionError((name,calls))
    try:
        send({'op':'screens','screens':[{'name':'SYNTHETIC','width':1000,'height':700,'scale':1}]})
        state=wait(lambda v:row(v,'music') is not None)
        media=row(state,'music');assert media['track']['value']=='Fixture track'
        watcher=dbus.Interface(bus.get_object(WATCHER,'/StatusNotifierWatcher'),WATCHER)
        watcher.RegisterStatusNotifierItem('org.example.TrayFixture')
        state=wait(lambda v:row(v,'tray') is not None)
        tray=row(state,'tray');assert tray['icon']['kind']=='pixmap'
        action('music',media['id'],'scrollDown','track');called('Next')
        action('music',media['id'],'scrollUp','track');called('SetPosition')
        action('music',media['id'],'leftClick');called('PlayPause')
        # Panel buttons carry explicit intents, not configurable mouse events.
        panel_context = {'barKey':'SYNTHETIC/fixture','monitor':'SYNTHETIC','module':'music',
                         'item':media['id'],'nativeOwner':media['native']['owner']}
        for intent, method in [('player.next','Next'),('player.previous','SetPosition'),
                               ('player.toggle','PlayPause'),('player.shuffle','Set')]:
            before = sum(c[0] == method for c in calls)
            send({'op':'mediaAction','context':{**panel_context,'action':intent}})
            deadline = time.monotonic() + 5
            while sum(c[0] == method for c in calls) == before and time.monotonic() < deadline:
                time.sleep(.01)
            assert sum(c[0] == method for c in calls) == before + 1, (intent,calls)
        before = len(calls)
        send({'op':'mediaAction','context':{**panel_context,'action':'player.next','nativeOwner':':0.stale'}})
        wait(lambda v: bool(v.get('actionError')))
        assert len(calls) == before, 'Stale panel owner dispatched a media action'
        action('tray',tray['id'],'leftClick');called('Activate')
        action('tray',tray['id'],'middleClick');called('SecondaryActivate')
        action('tray',tray['id'],'scrollDown');called('Scroll')
        item.properties['Status']='NeedsAttention';item.NewStatus()
        wait(lambda v:row(v,'tray') is not None and row(v,'tray')['state']['urgent'])
        context={'barKey':'SYNTHETIC/fixture','monitor':'SYNTHETIC','module':'tray','item':tray['id']}
        send({'op':'nativeMenu','context':context,'menuId':0})
        response=wait(lambda v:v.get('kind')=='menu');assert not response.get('error'),response
        assert len(response['menu']['layout'][2])==2
        send({'op':'nativeMenuEvent','context':context,'menuId':1});called('MenuEvent')
        send({'op':'nativeMenuEvent','context':context,'menuId':2})
        assert wait(lambda v:v.get('kind')=='menu' and 'error' in v)['error']=='Menu item unavailable'
        menu.exists=False
        send({'op':'nativeMenuEvent','context':context,'menuId':1})
        assert 'removed' in wait(lambda v:v.get('kind')=='menu' and 'error' in v)['error']
        item.remove_from_connection()
        bus.release_name('org.example.TrayFixture')
        wait(lambda v: v.get('ready') is True and row(v,'tray') is None)
        previous_calls=len(calls)
        action('tray',tray['id'],'leftClick')
        time.sleep(.25)
        assert len(calls)==previous_calls, 'Removed tray target was dispatched'
        replacement_bus=dbus.SessionBus(private=True)
        replacement_bus.set_exit_on_disconnect(False)
        replacement=Item(replacement_bus)
        replacement_watcher=dbus.Interface(replacement_bus.get_object(WATCHER,'/StatusNotifierWatcher'),WATCHER)
        replacement_watcher.RegisterStatusNotifierItem('/StatusNotifierItem')
        state=wait(lambda v:row(v,'tray') is not None)
        assert row(state,'tray')['id'] != tray['id'], 'Reused stale bus owner identity'
        replacement.remove_from_connection()
        replacement_bus.close()
        print('PASS owner loss, stale targets, path registration and private-bus MPRIS state/actions, SNI watcher registration, activation/scroll, attention and validated DBusMenu events')
    finally:
        p.stdin.close()
        try:p.wait(timeout=5)
        except subprocess.TimeoutExpired:p.kill();p.wait()
        if p.returncode:print(p.stderr.read(),file=sys.stderr)
        loop.quit()
