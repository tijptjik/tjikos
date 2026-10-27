"""Pagination, command identity and alias contracts for the launcher."""
import os
from pathlib import Path
import sys
import tempfile
import time
import unittest
from unittest.mock import Mock, patch

sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
import runtjik as r


class NavigationTests(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        root=Path(self.temp.name)
        env=patch.dict(os.environ,{'RUNTJIK_CONFIG':str(root/'absent.conf'),
                                  'XDG_STATE_HOME':str(root/'state'),'XDG_CACHE_HOME':str(root/'cache')})
        env.start();self.addCleanup(env.stop)
        self.launcher=r.Launcher(demo=True)
        self.launcher.checked=time.monotonic()
        self.launcher._icons=Mock()
        self.launcher._icons.resolve.side_effect=lambda name:name
        clients=patch.object(r,'demo_windows',return_value=[])
        clients.start();self.addCleanup(clients.stop)

    def many_apps(self,count=260):
        self.launcher.entries=[r.Entry(f'apps:{i}',f'Entry {i:03d}') for i in range(count)]

    def test_pages_reach_beyond_100_without_eager_icon_resolution(self):
        self.many_apps()
        first=self.launcher.query()
        self.assertEqual(len(first['results']),100)
        self.assertTrue(first['has_more'])
        self.assertEqual(first['next_offset'],100)
        self.assertEqual(self.launcher._icons.resolve.call_count,100)
        second=self.launcher.query(offset=100)
        third=self.launcher.query(offset=200)
        ids=[row['id'] for page in (first,second,third) for row in page['results']]
        self.assertEqual(len(ids),260)
        self.assertEqual(len(set(ids)),260)
        self.assertFalse(third['has_more'])
        self.assertEqual(third['next_offset'],260)
        self.assertEqual(third['results'][-1]['id'],'apps:259')
        self.assertEqual(len(self.launcher.current),260)

    def test_refresh_can_preserve_loaded_extent_and_new_search_resets_page(self):
        self.many_apps()
        self.assertEqual(len(self.launcher.query(limit=200)['results']),200)
        result=self.launcher.query('Entry 259')
        self.assertEqual(len(result['results']),1)
        self.assertEqual(result['offset'],0)
        self.assertFalse(result['has_more'])

    def test_page_snapshot_prevents_rank_changes_from_skipping_entries(self):
        self.many_apps()
        first=self.launcher.query()
        self.launcher.entries.insert(0,r.Entry('apps:new','A new app'))
        second=self.launcher.query(offset=100)
        self.assertEqual(first['results'][-1]['id'],'apps:99')
        self.assertEqual(second['results'][0]['id'],'apps:100')
        self.assertEqual(second['total'],260)
        refreshed=self.launcher.query(limit=200)
        self.assertEqual(refreshed['results'][0]['id'],'apps:0')
        self.assertEqual(refreshed['total'],261)

    def test_invalid_pagination_is_rejected(self):
        for options in ({'offset':-1},{'offset':'100'},{'limit':0},{'limit':True}):
            with self.subTest(options=options),self.assertRaises(ValueError):
                self.launcher.query(**options)

    def test_bang_and_run_query_the_same_commands(self):
        self.launcher.entries=[r.Entry('apps:git','GUI Git'),
                               r.Entry('commands:git','git',provider='commands'),
                               r.Entry('commands:fish','fish',provider='commands')]
        for query in ('/run git','!git','! git'):
            with self.subTest(query=query):
                self.assertEqual([row['id'] for row in self.launcher.query(query)['results']],['commands:git'])
        self.assertEqual({row['id'] for row in self.launcher.query('!')['results']}, {'commands:git','commands:fish'})

    def test_menu_aliases_share_single_rows_and_both_shortcut_queries_work(self):
        rows=self.launcher.query('/')['results']
        self.assertEqual(len(rows),9)
        self.assertNotIn('/shortcuts',{row['id'] for row in rows})
        by_id={row['id']:row for row in rows}
        self.assertIn('!',by_id['/run']['name'])
        self.assertIn('/shortcuts',by_id['/bindings']['name'])
        self.assertEqual(self.launcher.query('/bindings')['results'],self.launcher.query('/shortcuts')['results'])

    def test_retired_hamr_entries_are_removed_during_refresh(self):
        apps=[r.Entry('apps:hamr.desktop','Hamr',data={'executable':'hamr','desktop_id':'hamr.desktop'}),
              r.Entry('apps:keep.desktop','Keep',data={'executable':'keep'})]
        commands=[r.Entry('commands:hamr','hamr',provider='commands',data={'argv':['/bin/hamr']})]
        self.launcher.demo=False
        self.launcher.providers={'apps':Mock(),'commands':Mock()}
        self.launcher.providers['apps'].index.return_value=apps
        self.launcher.providers['commands'].index.return_value=commands
        with patch.object(r,'signature',return_value=[]):
            self.launcher.refresh(True)
        self.assertEqual([entry.id for entry in self.launcher.entries],['apps:keep.desktop'])


class CommandIdentityTests(unittest.TestCase):
    def test_shell_and_flatpak_wrappers_do_not_inherit_gui_icons_or_windows(self):
        apps=[r.Entry('apps:spotify','Spotify',icon='spotify',data={'executable':'flatpak','commandline':'flatpak run com.spotify.Client','classes':['Spotify']}),
              r.Entry('apps:wrapped','Wrapped',icon='wrapped',data={'executable':'bash','commandline':'bash -c wrapped','classes':['Wrapped']})]
        for command in ('bash','flatpak'):
            cli=r.Entry('commands:'+command,command,icon='spotify',provider='commands',data={'argv':['/usr/bin/'+command],'gui':True,'classes':['Spotify']})
            r.decorate_commands([*apps,cli])
            self.assertEqual(cli.icon,r.COMMAND_ICON)
            self.assertFalse(cli.data.get('gui'))
            self.assertNotIn('classes',cli.data)
            self.assertEqual(r.matched_windows(cli,[{'class':'Spotify','address':'0xabc'}],r.DEFAULTS),[])

    def test_ambiguous_executable_does_not_choose_an_arbitrary_app(self):
        apps=[r.Entry('apps:'+name,name,icon=name,data={'executable':'shared','classes':[name]}) for name in ('One','Two')]
        cli=r.Entry('commands:shared','shared',provider='commands',data={'argv':['/usr/bin/shared']})
        r.decorate_commands([*apps,cli])
        self.assertEqual(cli.icon,r.COMMAND_ICON)
        self.assertFalse(cli.data.get('gui'))

    def test_dnf_and_fish_keep_terminal_launching_with_their_subject_icons(self):
        for name,icon in [('dnf','fedora-media-writer'),('dnf5','fedora-media-writer'),('fish','fish')]:
            with self.subTest(command=name):
                entry=r.Entry('commands:'+name,name,provider='commands',data={'argv':['/usr/bin/'+name]})
                r.decorate_commands([entry])
                self.assertEqual(entry.icon,icon)
                self.assertFalse(entry.data.get('gui'))
                with patch.object(r,'dispatch') as dispatch:
                    r.Commands().activate(entry,r.DEFAULTS)
                dispatch.assert_called_once_with(r.DEFAULTS['launch']['terminal']+entry.data['argv'],r.DEFAULTS)
