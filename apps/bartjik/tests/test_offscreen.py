import unittest
from unittest.mock import patch
from bartjik_core import config, workspaces


class OffscreenTests(unittest.TestCase):
    def setUp(self):
        self.monitor = dict(id=2, name='DP-2', x=3440, y=720, width=1920,
                            height=1080, scale=1, transform=0,
                            activeWorkspace={'id': 13}, specialWorkspace={'id': 0})
        self.snap = {'monitors': [self.monitor], 'clients': [],
                     'workspaces': [{'id': 13, 'name': '13', 'monitor': 'DP-2'}]}

    def client(self, address, x, **overrides):
        client = dict(address=address, monitor=2, workspace={'id': 13},
                      at=[x, 720], size=[960, 1080], mapped=True, hidden=False,
                      floating=False, pinned=False, fullscreenHandler='scrolling',
                      **{'class': 'kitty'})
        client.update(overrides)
        self.snap['clients'].append(client)
        return client

    def ids(self, side):
        return [c['address'] for c in workspaces.offscreen(self.snap, self.monitor)[side]]

    def test_geometry_slivers_order_and_duplicate_apps(self):
        self.client('near-left', 2480)
        self.client('far-left', 1400)
        self.client('visible', 3440)
        self.client('partial', 3000)
        self.client('sliver-left', 2488)
        self.client('sliver-right', 5352)
        self.client('right', 5400)
        self.assertEqual(self.ids('left'), ['far-left', 'near-left', 'sliver-left'])
        self.assertEqual(self.ids('right'), ['sliver-right', 'right'])
        # Silent viewport movement must change sides without changing workspace.
        self.snap['clients'][0]['at'][0] = 5401
        self.assertEqual(self.ids('left'), ['far-left', 'sliver-left'])
        self.assertEqual(self.ids('right'), ['sliver-right', 'right', 'near-left'])

    def test_excludes_other_workspaces_monitors_and_non_tiled_windows(self):
        for key, value in [('workspace', {'id': 14}), ('monitor', 0),
                           ('floating', True), ('pinned', True), ('hidden', True),
                           ('mapped', False), ('fullscreenHandler', 'master')]:
            self.client(key, 6000, **{key: value})
        self.assertEqual(self.ids('right'), [])
        self.client('ordinary', 6000)
        self.monitor['specialWorkspace'] = {'id': -99}
        # Previewing the ordinary workspace still describes its retained viewport.
        self.assertEqual(self.ids('right'), ['ordinary'])

    def test_hovered_inactive_workspace_uses_its_own_windows(self):
        self.snap['workspaces'].append({'id': 14, 'name': '14', 'monitor': 'DP-2'})
        self.client('active-right', 5400)
        self.client('inactive-left', 2000, workspace={'id': 14})
        rows = workspaces.view(self.snap, 'DP-2', {'offscreen': True})
        self.assertEqual([c['address'] for c in rows[0]['offscreen']['right']], ['active-right'])
        self.assertEqual([c['address'] for c in rows[1]['offscreen']['left']], ['inactive-left'])
        self.assertFalse(rows[1]['state']['active'])

    def test_scale_transform_and_missing_geometry(self):
        self.monitor['scale'] = 2
        self.client('right', 4400)
        self.assertEqual(self.ids('right'), ['right'])
        self.monitor['transform'] = 1
        self.snap['clients'][0]['at'][0] = 3980
        self.assertEqual(self.ids('right'), ['right'])
        self.monitor['width'] = self.monitor['height'] = 0
        self.assertEqual(self.ids('right'), [])

    def test_opt_in_and_config_validation(self):
        self.client('right', 5400, title='Project notes — Kitty')
        self.assertNotIn('offscreen', workspaces.view(self.snap, 'DP-2', {})[0])
        row = workspaces.view(self.snap, 'DP-2', {'offscreen': True})[0]
        self.assertEqual(row['offscreen']['right'][0]['address'], 'right')
        self.assertEqual(row['offscreen']['right'][0]['title'], 'Project notes — Kitty')
        config.module({'provider': 'workspaces', 'offscreen': True})
        with self.assertRaises(ValueError):
            config.module({'provider': 'workspaces', 'offscreen': 'true'})


class SnapshotTests(unittest.IsolatedAsyncioTestCase):
    async def test_snapshot_retains_geometry_and_window_titles(self):
        async def command(args, **kwargs):
            return {'monitors': [dict(id=1, name='DP-1', x=10, width=1920)],
                    'workspaces': [],
                    'clients': [dict(address='0x1', at=[1, 2], size=[3, 4],
                                     title='Project notes — Kitty', **{'class': 'kitty'})]}[args[-1]]
        with patch('bartjik_core.workspaces.json_command', command):
            snap = await workspaces.snapshot()
        self.assertEqual(snap['monitors'][0]['x'], 10)
        self.assertEqual(snap['clients'][0]['at'], [1, 2])
        self.assertEqual(snap['clients'][0]['title'], 'Project notes — Kitty')
