
# Test-only comparison oracle; production launches the Rust binary.
import sys as _sys
from pathlib import Path as _Path
_sys.path.insert(0, str(_Path(__file__).resolve().parent / "reference"))
_sys.path.insert(0, str(_Path(__file__).resolve().parent / "reference/qml"))
import asyncio
import json
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import AsyncMock, patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from bartjik_core import config, connectivity as c, providers
from bartjik_core.actions import Executor
from bartjik_core.engine import Engine
from bartjik_core.io import Unavailable, command

RELAYS = "Japan (jp)\n\tOsaka (osa) @ 0, 0\n\tTokyo (tyo) @ 0, 0\nNetherlands (nl)\n\tAmsterdam (ams) @ 0, 0\nSingapore (sg)\n\tSingapore (sin) @ 0, 0\n\tSingapore (sin) @ 0, 0\n"
TAIL = {"BackendState": "Running", "Self": {"HostName": "demo"}, "TailscaleIPs": ["100.64.0.1"], "Peer": {"peer": {"HostName": "demo-peer", "Online": True, "TailscaleIPs": ["100.64.0.2"]}}}


class ConnectivityTests(unittest.IsolatedAsyncioTestCase):
    def test_models_keep_location_separate_from_ip_and_deduplicate_cities(self):
        catalog = c.parse_relays(RELAYS)
        self.assertEqual(len(catalog['sg']['cities']), 1)
        panel = c.mullvad_panel('connected', 'Japan, Tokyo, IPv4: 192.0.2.1', catalog, ['nl','jp','sg'])
        self.assertEqual((panel['country'], panel['city']), ('Japan', 'Tokyo'))
        self.assertEqual(panel['cities'], [dict(id='osa', name='Osaka', active=False), dict(id='tyo', name='Tokyo', active=True)])
        self.assertNotIn('192.0.2.1', str(panel))
        self.assertEqual([p['id'] for p in panel['countries']], ['nl','jp','sg'])
        self.assertEqual(c.mullvad_panel('connected', 'Japan, Tokyo', {}, [])['cities'], [])
        self.assertEqual(c.mullvad_panel('connected', 'Singapore, Singapore', catalog, [])['cities'], [dict(id='sin', name='Singapore', active=True)])
        panel = c.tailscale_panel(TAIL)
        self.assertEqual(panel['peers'][0]['id'], 'peer')
        self.assertEqual(panel['address'], '100.64.0.1')

    async def test_provider_uses_structured_city_without_ip_annotations(self):
        with patch.object(providers, 'json_command', AsyncMock(return_value={'state':'connected','details':{'location':{'country':'Japan','city':'Tokyo','ipv4':'192.0.2.1'}}})), patch.object(c, 'relay_catalog', AsyncMock(return_value=c.parse_relays(RELAYS))):
            row=(await providers.mullvad({'panel':{'hover':True}}))[0]
            self.assertEqual([city['id'] for city in row['panel']['cities']], ['osa', 'tyo'])
            self.assertEqual(row['panel']['city'],'Tokyo')
            self.assertNotIn('192.0.2.1',str(row))

    async def test_catalog_shared_cached_and_retained_on_error(self):
        with patch.object(c, '_catalog', {}), patch.object(c, '_catalog_until', 0), patch.object(c, '_catalog_lock', None), patch.object(c, 'command', AsyncMock(return_value=RELAYS)) as run:
            results = await asyncio.gather(c.relay_catalog(), c.relay_catalog())
            self.assertEqual(run.await_count, 1)
            self.assertEqual(results[0], results[1])
            c._catalog_until = 0
            run.side_effect = Unavailable('offline')
            self.assertEqual(await c.relay_catalog(), results[0])
            await c.relay_catalog()
            self.assertEqual(run.await_count, 2)

    async def test_explicit_city_selection_and_country_shortcuts_do_not_connect(self):
        with patch.object(c, 'relay_catalog', AsyncMock(return_value=c.parse_relays(RELAYS))), patch.object(c, 'json_command', AsyncMock()) as status, patch.object(c, 'command', AsyncMock(return_value='')) as run:
            await c.act('mullvad','city',{'country':'jp','city':'osa'},{},Executor())
            self.assertEqual(run.call_args.args[0],['mullvad','relay','set','location','jp','osa'])
            await c.act('mullvad','city',{'country':'jp','city':'tyo'},{},Executor())
            self.assertEqual(run.call_args.args[0],['mullvad','relay','set','location','jp','tyo'])
            status.assert_not_called()
            await c.act('mullvad','country','nl',{},Executor())
            self.assertEqual(run.call_args.args[0],['mullvad','relay','set','location','nl'])
            self.assertFalse(any('connect' in call.args[0] for call in run.call_args_list))
            run.reset_mock()
            with self.assertRaises(Unavailable):
                await c.act('mullvad','country','jp;disconnect',{},Executor())
            run.assert_not_called()

    async def test_city_selection_rejects_missing_wrong_country_and_malformed_choices(self):
        invalid = [None, 'osa', [], {}, {'country':[], 'city':'osa'}, {'country':'jp', 'city':{}},
                   {'country':'missing', 'city':'osa'}, {'country':'jp', 'city':'missing'},
                   {'country':'nl', 'city':'tyo'}, {'country':'jp', 'city':'osa;disconnect'}]
        with patch.object(c, 'relay_catalog', AsyncMock(return_value=c.parse_relays(RELAYS))), patch.object(c, 'command', AsyncMock()) as run:
            for selection in invalid:
                with self.subTest(selection=selection), self.assertRaisesRegex(Unavailable, 'city is no longer available'):
                    await c.act('mullvad', 'city', selection, {}, Executor())
            run.assert_not_called()

    async def test_tailscale_copy_ping_and_stopped_state(self):
        with patch.object(c, 'json_command', AsyncMock(return_value=TAIL)), patch.object(c, 'command', AsyncMock(return_value='pong from demo in 12ms')) as run:
            await c.act('tailscale','copy','self',{},Executor())
            self.assertEqual(run.call_args.args[0],['wl-copy'])
            self.assertEqual(run.call_args.kwargs['input_text'],'100.64.0.1')
            self.assertEqual(await c.act('tailscale','ping','peer',{},Executor()),'Reachable · 12ms')
            self.assertEqual(run.call_args.args[0],['tailscale','ping','--c=1','--timeout=3s','100.64.0.2'])
            run.reset_mock()
            with self.assertRaises(Unavailable):
                await c.act('tailscale','ping','unknown',{},Executor())
            run.assert_not_called()
        with patch.object(c, 'json_command', AsyncMock(return_value={**TAIL,'BackendState':'Stopped'})), patch.object(c,'command', AsyncMock(return_value='')) as run:
            with self.assertRaises(Unavailable):
                await c.act('tailscale','ping','peer',{},Executor())
            run.assert_not_called()
            await c.act('tailscale','toggle',None,{},Executor())
            self.assertEqual(run.call_args.args[0],['tailscale','up'])

    async def test_action_failure_feedback_and_origin_checks(self):
        e=Engine(lambda _:None);e.changed=lambda:None
        e.bars=[dict(key='DP-2/bar',screen='DP-2',moduleConfigs={'vpn':{'provider':'mullvad'}},sources={'vpn':'worker'})]
        wake=asyncio.Event();e.workers={'worker':SimpleNamespace(wake=wake)}
        context=dict(barKey='DP-2/bar',monitor='DP-2',module='vpn',action='country',selection='jp')
        with patch.object(c,'act',AsyncMock(side_effect=Unavailable('Could not change location'))) as action:
            await e.panel_action({'context':context})
            self.assertTrue(e.panel_feedback['error'])
            self.assertFalse(e.panel_feedback['busy'])
            self.assertEqual(e.panel_feedback['module'],'vpn')
            self.assertFalse(wake.is_set())
            action.reset_mock()
            await e.panel_action({'context':{**context,'monitor':'DP-1'}})
            action.assert_not_called()

    def test_country_validation(self):
        for countries in ('jp',['japan'],['jp','jp'],['--help']):
            with self.assertRaises(ValueError):
                config.module({'provider':'mullvad','quickCountries':countries})

    async def test_stdin_transport_does_not_interpret_text(self):
        value='100.64.0.1;$(not-a-command)'
        self.assertEqual(await command(['python3','-c','import sys; print(sys.stdin.read())'],input_text=value),value)


class ExternalPanelTests(unittest.IsolatedAsyncioTestCase):
    async def test_hover_retains_pointer_then_closes_and_click_pins(self):
        with tempfile.TemporaryDirectory() as td:
            (Path(td)/'shell.qml').touch()
            ctx=dict(monitor='DP-2',anchor=dict(x=40,y=900,width=20,height=24),event='hoverEnter')
            hovered=True
            calls=[]
            async def run(args,**kw):
                calls.append(args)
                if args[-1]=='isOpen': return 'false'
                if args[-1]=='geometry': return json.dumps({'hovered':hovered})
                return ''
            ex=Executor()
            with patch('bartjik_core.actions.command',side_effect=run):
                await ex.panel('network',ctx,{'panelPath':td})
                self.assertIn('network',ex.hover_panels)
                ex.leave_panel('network')
                await asyncio.sleep(.7)
                self.assertFalse(any(a[-1]=='close' for a in calls))
                hovered=False
                await asyncio.sleep(.3)
                self.assertTrue(any(a[-1]=='close' for a in calls))
                await ex.panel('network',ctx,{'panelPath':td})
                await ex.panel('network',{**ctx,'event':'leftClick'},{'panelPath':td})
                self.assertNotIn('network',ex.hover_panels)
                before=len(calls)
                await ex.close()
                self.assertEqual(len(calls),before)

    async def test_switching_closes_only_hover_owned_panel(self):
        ex=Executor();ex.hover_panels['audio']=['quickshell','ipc','call','audio']
        with patch('bartjik_core.actions.command',AsyncMock(return_value='')) as run:
            await ex.close_hover_panels()
            run.assert_awaited_once_with(['quickshell','ipc','call','audio','close'],timeout=2)
            self.assertFalse(ex.hover_panels)
