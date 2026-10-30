import contextlib
import importlib.util
import json
import os
import shutil
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import runtjik as r


class LauncherTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.env = patch.dict(os.environ, {
            "XDG_CONFIG_HOME": str(self.root / "config"), "XDG_STATE_HOME": str(self.root / "state"),
            "XDG_CACHE_HOME": str(self.root / "cache"), "XDG_DATA_HOME": str(self.root / "data"),
            "XDG_DATA_DIRS": str(self.root / "system"), "RUNTJIK_CONFIG": str(self.root / "runtjik.conf"),
            "PATH": str(self.root / "bin"), "XDG_CURRENT_DESKTOP": "Hyprland"})
        self.env.start()
        self.addCleanup(self.env.stop)
        self.settings = r.config()
        self.history_filter = patch.object(r, "fish_commands", return_value={"example", "not-runnable"})
        self.history_filter.start()
        self.addCleanup(self.history_filter.stop)

    def app(self, directory, filename="test.desktop", extra=""):
        directory.mkdir(parents=True, exist_ok=True)
        path = directory / filename
        path.write_text("[Desktop Entry]\nType=Application\nName=Test App\nExec=/bin/true\n" + extra)
        return path

    def command(self, directory, name="example"):
        directory.mkdir(parents=True, exist_ok=True)
        path = directory / name
        path.write_text("#!/bin/sh\nexit 0\n")
        path.chmod(0o755)
        return path

    def launcher(self):
        icons = patch.object(r, "Icons")
        self.addCleanup(icons.stop)
        icons.start().return_value.resolve.side_effect = lambda name: name
        launcher = r.Launcher()
        self.addCleanup(launcher.store.db.close)
        return launcher

    def test_frecency_decay_success_switches_and_rebuild(self):
        store = r.Store(self.root / "history.sqlite3")
        self.addCleanup(store.db.close)
        week = r.FRECENCY_HALF_LIFE
        with patch.object(r.time, "time", return_value=100):
            store.record("old", "launch")
            store.record("old", "launch")
            store.record("ignored", "launch", False)
            store.record("ignored", "refresh")
        with patch.object(r.time, "time", return_value=100 + 2 * week):
            store.record("new", "switch")
            scores = store.frecency()
            self.assertEqual(scores, {"old": .5, "new": 1})
            store.rebuild()
            self.assertEqual(store.frecency(), scores)
            # An older timestamp after a wall-clock adjustment must not amplify history.
            with patch.object(r.time, "time", return_value=100 + week):
                store.record("new", "launch")
            self.assertEqual(store.frecency()["new"], 1.5)

    def test_frecency_backfills_existing_database_once(self):
        path = self.root / "legacy.sqlite3"
        store = r.Store(path)
        store.record("existing", "launch")
        expected = store.frecency(now=0)
        store.db.execute("DROP TABLE frecency")
        store.db.commit()
        store.db.close()
        store = r.Store(path)
        self.addCleanup(store.db.close)
        self.assertEqual(store.frecency(now=0), expected)
        store.db.execute("DELETE FROM events")
        store.db.commit()
        other = r.Store(path)
        self.addCleanup(other.db.close)
        self.assertEqual(other.frecency(now=0), expected)

    def test_running_filter_alias_search_and_pagination(self):
        launcher = self.launcher()
        launcher.entries = [r.Entry("apps:a", "Alpha"), r.Entry("apps:b", "Beta"),
                            r.Entry("apps:c", "Closed"),
                            r.Entry("commands:a", "Alpha", provider="commands")]
        with patch.object(r, "windows", return_value=[]), patch.object(r, "matched_windows", side_effect=lambda e, *_: [] if e.id == "apps:c" else [{}]):
            for prefix in ("/processes", "?"):
                result = launcher.query(prefix, limit=1)
                self.assertEqual(result["total"], 2)
                self.assertEqual(result["results"][0]["id"], "apps:a")
                self.assertEqual(launcher.query(prefix, offset=1, limit=1)["results"][0]["id"], "apps:b")
                self.assertEqual([row["id"] for row in launcher.query(prefix + " bet")["results"]], ["apps:b"])
                self.assertEqual(launcher.activate(prefix), {"kind": "edit", "text": prefix + " "})
        with patch.object(r, "windows", return_value=[]):
            self.assertEqual(launcher.query("/processes")["results"], [])
        self.assertEqual(launcher.query("/proc")["results"][0]["id"], "/processes")

    def test_run_mode_separates_commands_and_preserves_frecency(self):
        launcher = self.launcher()
        launcher.entries = [r.Entry("apps:a", "Alpha"),
                            r.Entry("commands:z", "zebra", provider="commands"),
                            r.Entry("commands:a", "alpha", provider="commands")]
        launcher.store.record("commands:z", "switch")
        with patch.object(r, "windows", return_value=[]):
            self.assertEqual([e["id"] for e in launcher.query()["results"]], ["apps:a"])
            self.assertEqual(launcher.query("/run")["results"][0]["id"], "commands:z")
            self.assertEqual(launcher.query("/run alp")["results"][0]["id"], "commands:a")
            self.assertEqual(launcher.activate("/run"), {"kind": "edit", "text": "/run "})
            self.assertEqual(launcher.query("alpha")["results"][0]["id"], "apps:a")
        self.assertEqual(launcher.activate("/pin"), {"kind": "pin"})

    def test_live_duplicates_are_not_hidden_and_retired_history_is_preserved(self):
        directory = self.root / "data/applications"
        old = self.app(directory, "dev.zed.desktop")
        self.app(directory, "dev.zed.Zed.desktop")
        self.assertEqual(len([e for e in r.Apps().index(self.settings) if e.data["desktop_id"] in ("dev.zed.desktop", "dev.zed.Zed.desktop")]), 2)
        old.unlink()
        entries = [e for e in r.Apps().index(self.settings) if e.data["desktop_id"] == "dev.zed.Zed.desktop"]
        self.assertEqual(len(entries), 1)
        launcher = self.launcher()
        launcher.entries = entries
        launcher.store.record("apps:dev.zed.desktop", "launch")
        launcher.store.record("apps:dev.zed.Zed.desktop", "launch")
        with patch.object(r, "windows", return_value=[]):
            self.assertEqual(launcher.query()["results"][0]["count"], 2)

    def test_rank_order_stays_stable_until_search_reset_or_reopen(self):
        launcher = self.launcher()
        launcher.entries = [r.Entry("apps:a", "Alpha"), r.Entry("apps:z", "Zebra")]
        with patch.object(r, "windows", return_value=[]):
            initial = [e["id"] for e in launcher.query()["results"]]
            launcher.store.record("apps:z", "launch")
            self.assertEqual([e["id"] for e in launcher.query()["results"]], initial)
            launcher.query("z")
            self.assertEqual(launcher.query()["results"][0]["id"], "apps:z")
            for _ in range(3):launcher.store.record("apps:a", "launch")
            self.assertEqual(launcher.query()["results"][0]["id"], "apps:z")
            launcher.reset_ranking()
            self.assertEqual(launcher.query()["results"][0]["id"], "apps:a")

    def test_application_presentation_and_exclusions(self):
        directory = self.root / "data/applications"
        for desktop_id in ["gimp.desktop", "obsidian-appimage.desktop", "Zoom.desktop", *r.HIDDEN_APPS]:
            self.app(directory, desktop_id)
        result = {e.data["desktop_id"]: e for e in r.Apps().index(self.settings)}
        self.assertEqual(result["gimp.desktop"].name, "GIMP")
        self.assertEqual(result["obsidian-appimage.desktop"].name, "Markdown Viewer")
        self.assertEqual(result["Zoom.desktop"].icon, "zoom")
        self.assertEqual(result["Zoom.desktop"].name, "Zoom")
        self.assertFalse(set(result) & r.HIDDEN_APPS)

    def test_gui_commands_use_application_icons_and_no_terminal_wrapper(self):
        gui = r.Entry("apps:blueman-manager.desktop", "Bluetooth", icon="blueman", data={
            "executable":"blueman-manager", "terminal":False, "classes":["blueman-manager"]})
        command = r.Entry("commands:blueman-manager", "blueman-manager", provider="commands", data={"argv":["/bin/blueman-manager"]})
        cli = r.Entry("commands:echo", "echo", provider="commands", data={"argv":["/bin/echo"]})
        entries = r.decorate_commands([gui, command, cli])
        self.assertEqual(command.icon, "blueman")
        self.assertTrue(command.data["gui"])
        with patch.object(r, "dispatch") as dispatch:
            r.Commands().activate(command, self.settings)
            dispatch.assert_called_with(["/bin/blueman-manager"], self.settings)
            r.Commands().activate(cli, self.settings)
            dispatch.assert_called_with(self.settings["launch"]["terminal"] + ["/bin/echo"], self.settings)

    def write_fish_history(self, commands):
        path = r.fish_history_path()
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text("".join("- cmd: " + command.replace("\\", "\\\\").replace("\n", "\\n") + "\n  when: 1\n" for command in commands))

    def real_history(self):
        self.history_filter.stop()
        original = r.shutil.which
        parser = shutil.which("fish_indent", path="/usr/bin:/bin")
        if not parser:
            self.skipTest("requires Fish parser")
        lookup = patch.object(r.shutil, "which", side_effect=lambda name, *a, **kw: parser if name == "fish_indent" else original(name, *a, **kw))
        lookup.start()
        self.addCleanup(lookup.stop)

    def test_fish_history_command_positions_and_wrappers(self):
        self.real_history()
        self.write_fish_history(['sudo -u root git status', 'env TOKEN=private rg needle | bat',
                                 'echo "not-a-command; danger"', 'command /usr/bin/kitty',
                                 'printf "%s" (never-execute-secret)', 'mullvad-exclude curl example.org',
                                 '$dynamic should-not-appear'])
        names = r.fish_commands()
        self.assertTrue({"git", "rg", "bat", "kitty", "curl"} <= names)
        self.assertTrue(names.isdisjoint({"root", "status", "needle", "danger", "private", "never-execute-secret", "should-not-appear"}))

    def test_history_refresh_adds_and_removes_whitelisted_commands(self):
        self.real_history()
        (self.root / "runtjik.conf").write_text("[index]\ndesktop_apps=false\n")
        self.command(self.root / "bin", "first")
        self.command(self.root / "bin", "second")
        self.write_fish_history(["first --secret not-for-cache"])
        launcher = self.launcher()
        self.assertEqual([e.name for e in launcher.entries], ["first"])
        self.write_fish_history(["second ignored-argument"])
        launcher.refresh()
        self.assertEqual([e.name for e in launcher.entries], ["second"])
        cache = (r.xdg("cache") / "index.json").read_text()
        self.assertNotIn("ignored-argument", cache)
        self.assertNotIn("not-for-cache", cache)
        r.fish_history_path().unlink()
        launcher.refresh(True)
        self.assertEqual(launcher.entries, [])

    def test_missing_history_never_falls_back_to_all_path_commands(self):
        self.real_history()
        self.command(self.root / "bin")
        self.assertEqual(r.Commands().index(self.settings), [])

    def test_fish_history_namespace_and_escaped_records(self):
        self.real_history()
        with patch.dict(os.environ, {"fish_history": "work"}):
            self.write_fish_history(["example one\nsecond two", "echo 'literal\\ntext'"])
            self.assertEqual(r.fish_history_path().name, "work_history")
            self.assertTrue({"example", "second"} <= r.fish_commands())
        with patch.dict(os.environ, {"fish_history": ""}):
            self.assertEqual(r.fish_commands(), set())

    def test_history_counts_only_success_and_rebuild(self):
        store = r.Store(self.root / "history.db")
        self.addCleanup(store.db.close)
        store.record("apps:one", "launch")
        store.record("apps:one", "launch", False)
        store.record("apps:one", "switch")
        self.assertEqual(store.counts()["launch:apps:one"]["count"], 1)
        before = store.counts()
        store.db.execute("DELETE FROM counts")
        store.rebuild()
        self.assertEqual(before, store.counts())
        self.assertEqual(store.db.execute("SELECT COUNT(*) FROM events").fetchone()[0], 3)

    def test_two_connections_do_not_lose_updates(self):
        first, second = r.Store(self.root / "history.db"), r.Store(self.root / "history.db")
        self.addCleanup(first.db.close)
        self.addCleanup(second.db.close)
        for _ in range(10):
            first.record("app", "launch")
            second.record("app", "launch")
        self.assertEqual(first.counts()["launch:app"]["count"], 20)

    def test_shift_enter_without_instances_uses_normal_launch_and_counts(self):
        self.command(self.root / "bin")
        launcher = self.launcher()
        for prepare in (False, True):
            with self.subTest(prepare=prepare), patch.object(r, "windows", return_value=[]), patch.object(r, "dispatch") as dispatch:
                response = launcher.activate("commands:example", "switch", prepare=prepare)
                if prepare:
                    self.assertEqual(response, {"kind":"launch", "entry":"commands:example"})
                    dispatch.assert_not_called()
                    response = launcher.activate(response["entry"], "launch")
                self.assertEqual(response["kind"], "activated")
                dispatch.assert_called_once()
        counts = launcher.store.counts()
        self.assertEqual(counts["launch:commands:example"]["count"], 2)
        self.assertNotIn("switch:commands:example", counts)

    def test_failed_shift_enter_fallback_does_not_record_success(self):
        self.command(self.root / "bin")
        launcher = self.launcher()
        with patch.object(r, "windows", return_value=[]), patch.object(r, "dispatch", side_effect=RuntimeError("launch failed")):
            with self.assertRaisesRegex(RuntimeError, "launch failed"):
                launcher.activate("commands:example", "switch")
        self.assertNotIn("launch:commands:example", launcher.store.counts())
        self.assertNotIn("switch:commands:example", launcher.store.counts())

    def test_prepare_switch_does_not_focus_before_layer_releases_keyboard(self):
        launcher = r.Launcher(demo=True)
        client = r.demo_windows()[0]
        with patch.object(r, "demo_windows", return_value=[client]), patch.object(r, "focus_window") as focus:
            result = launcher.activate("apps:firefox.desktop", "switch", prepare=True)
            self.assertEqual(result["kind"], "windows")
            self.assertTrue(result["auto_select"])
            focus.assert_not_called()
            result = launcher.activate("apps:firefox.desktop", "switch", client["address"])
            self.assertEqual(result["kind"], "activated")

    def icon_manifest(self):
        source = self.root / "theme" / "icon.png"
        source.parent.mkdir()
        source.write_bytes(b"cached artwork")
        icons = r.Icons(self.settings)
        icons.manifest.parent.mkdir(parents=True)
        icons.manifest.write_text(json.dumps({"schema": 1,
            "watched": {str(source.parent): icons.stamp(source.parent)},
            "records": {str(source): {"source": str(source), "resolved": str(source), "stamp": icons.stamp(source)}}}))
        return source

    def test_cached_icons_do_not_initialize_gtk(self):
        source = self.icon_manifest()
        with patch.object(r.Icons, "initialize", side_effect=AssertionError("GTK loaded on a cache hit")):
            icons = r.Icons(self.settings)
            self.assertEqual(icons.resolve(str(source)), str(source))
            self.assertIsNone(icons.theme)

    def test_cached_icon_source_edit_requires_resolution(self):
        source = self.icon_manifest()
        source.write_bytes(b"changed artwork")
        icons = r.Icons(self.settings)
        with patch.object(icons, "initialize") as initialize:
            self.assertEqual(icons.resolve(str(source)), str(source))
            initialize.assert_called_once()

    def test_new_theme_file_invalidates_icon_lookup_manifest(self):
        source = self.icon_manifest()
        (source.parent / "new-icon.png").write_bytes(b"new")
        self.assertEqual(r.Icons(self.settings).records, {})

    def test_removed_cached_icon_is_not_returned(self):
        source = self.icon_manifest()
        source.unlink()
        icons = r.Icons(self.settings)
        self.assertEqual(icons.records, {})

    def test_cached_command_icons_match_terminal_without_reindexing(self):
        (self.root / "runtjik.conf").write_text("[index]\ndesktop_apps=false\n")
        self.command(self.root / "bin")
        self.launcher()
        cache = r.xdg("cache") / "index.json"
        payload = json.loads(cache.read_text())
        payload["entries"][0]["icon"] = "utilities-terminal"
        cache.write_text(json.dumps(payload))
        with patch.object(r.Commands, "index", side_effect=AssertionError("Unexpected reindex")):
            launcher = self.launcher()
        with patch.object(r, "windows", return_value=[]):
            self.assertEqual(launcher.query("/run")["results"][0]["icon"], "org.gnome.Terminal")

    def test_path_precedence_and_executable_filter(self):
        first, second = self.root / "bin", self.root / "other"
        self.command(first)
        self.command(second)
        self.command(first, "not-runnable").chmod(0o644)
        with patch.dict(os.environ, {"PATH": f"{first}:{second}:.:"}):
            entries = r.Commands().index(self.settings)
        self.assertEqual(len(entries), 1)
        self.assertEqual(entries[0].data["argv"], [str(first / "example")])

    @unittest.skipUnless(importlib.util.find_spec("gi"), "requires Gio")
    def test_hidden_override_and_desktop_visibility(self):
        user, system = self.root / "data/applications", self.root / "system/applications"
        self.app(user, extra="Hidden=true\n")
        self.app(system)
        self.app(system, "hidden.desktop", "NoDisplay=true\n")
        self.app(system, "other.desktop", "OnlyShowIn=KDE;\n")
        self.app(system, "missing.desktop", "TryExec=/no/such/executable\n")
        self.app(system, "good.desktop", "StartupWMClass=Example\n")
        with patch.object(r, "app_dirs", return_value=[user, system]):
            entries = r.Apps().index(self.settings)
        self.assertEqual([e.id for e in entries], ["apps:good.desktop"])
        self.assertEqual(entries[0].data["classes"], ["Example", "good"])

    def test_cache_rebuild_after_remove_and_chmod(self):
        self.settings["index"]["desktop_apps"] = False
        (self.root / "runtjik.conf").write_text("[index]\ndesktop_apps=false\n")
        path = self.command(self.root / "bin")
        launcher = self.launcher()
        self.assertEqual(len(launcher.entries), 1)
        path.chmod(0o644)
        launcher.refresh()
        self.assertEqual(launcher.entries, [])
        path.chmod(0o755)
        launcher.refresh()
        self.assertEqual(len(launcher.entries), 1)
        path.unlink()
        launcher.refresh(True)
        self.assertEqual(launcher.entries, [])

    def test_bad_cache_recovers(self):
        (self.root / "runtjik.conf").write_text("[index]\ndesktop_apps=false\n")
        cache = r.xdg("cache") / "index.json"
        cache.parent.mkdir(parents=True)
        cache.write_text("broken json")
        self.command(self.root / "bin")
        self.assertEqual(len(self.launcher().entries), 1)
        self.assertEqual(json.loads(cache.read_text())["version"], r.VERSION)

    def test_fuzzy_ranking_and_multiword(self):
        firefox = r.Entry("apps:firefox", "Firefox", "Web browser")
        self.assertIsNotNone(r.match_score("ffx", firefox))
        self.assertIsNotNone(r.match_score("fire web", firefox))
        self.assertIsNone(r.match_score("editor", firefox))
        self.assertGreater(r.match_score("firefox", firefox), r.match_score("fox", firefox))

    def test_exact_window_matching_no_substring_collision(self):
        entry = r.Entry("apps:foo", "Foo", data={"classes": ["Foo"]})
        clients = [{"class": "foo"}, {"class": "foobar"}, {"class": "other", "initialClass": "FOO"}]
        self.assertEqual(r.matched_windows(entry, clients, self.settings), [clients[0], clients[2]])

    def test_multiple_windows_requires_second_confirmation(self):
        with patch.object(r, "Icons") as icons:
            icons.return_value.resolve.side_effect = lambda name: name
            launcher = r.Launcher(demo=True)
            launcher._icons = icons.return_value
        response = launcher.activate("apps:firefox.desktop", "switch")
        self.assertEqual(response["kind"], "windows")
        self.assertEqual(len(response["results"]), 2)
        self.assertEqual(launcher.activate("apps:firefox.desktop", "switch", "0xdemo2")["kind"], "activated")
        with self.assertRaisesRegex(ValueError, "closed"):
            launcher.activate("apps:firefox.desktop", "switch", "0xgone")

    def test_lua_and_legacy_focus_special_workspace_without_hiding_it(self):
        client = {"address": "0xabc", "monitor": 1, "workspace": {"name": "special:chat"}}
        monitor = {"id": 1, "name": "DP-1", "specialWorkspace": {"name": ""}}
        commands = r.focus_commands(client, [monitor])
        self.assertEqual(commands[-1], ['hyprctl', 'dispatch', 'hl.dsp.focus({ window = "address:0xabc" })'])
        self.assertIn('toggle_special("chat")', commands[1][-1])
        monitor['specialWorkspace']['name'] = 'special:chat'
        self.assertEqual(len(r.focus_commands(client, [monitor])), 2)
        self.assertEqual(r.focus_commands(client, [monitor], False)[-1], ['hyprctl', 'dispatch', 'focuswindow', 'address:0xabc'])
        with self.assertRaises(ValueError):
            r.focus_commands({**client, 'address': 'malformed'}, [monitor])

    def test_focus_normal_window_dismisses_open_special_workspaces(self):
        client = {"address": "0xabc", "monitor": 1, "workspace": {"name": "2"}}
        monitors = [{"id": 0, "name": "DP-2", "specialWorkspace": {"name": "special:chat"}},
                    {"id": 1, "name": "DP-1", "specialWorkspace": {"name": "special:codex"}}]
        for lua in (True, False):
            with self.subTest(lua=lua):
                commands = r.focus_commands(client, monitors, lua)
                self.assertEqual(len(commands), 6)
                self.assertIn("DP-2", commands[0][-1])
                self.assertIn("chat", commands[1][-1])
                self.assertIn("DP-1", commands[2][-1])
                self.assertIn("codex", commands[3][-1])
                self.assertIn("DP-1", commands[4][-1])
                self.assertIn("address:0xabc", commands[5][-1])

    def test_focus_special_target_keeps_it_open_and_dismisses_other_overlays(self):
        client = {"address": "0xabc", "monitor": 1, "workspace": {"name": "special:chat"}}
        monitors = [{"id": 0, "name": "DP-2", "specialWorkspace": {"name": "special:codex"}},
                    {"id": 1, "name": "DP-1", "specialWorkspace": {"name": "special:chat"}}]
        for lua in (True, False):
            with self.subTest(lua=lua):
                commands = r.focus_commands(client, monitors, lua)
                self.assertEqual(len(commands), 4)
                self.assertIn("codex", commands[1][-1])
                self.assertFalse(any("chat" in command[-1] for command in commands))
                self.assertIn("DP-1", commands[-2][-1])

    def test_focus_hidden_special_target_replaces_other_overlay(self):
        client = {"address": "0xabc", "monitor": 1, "workspace": {"name": "special:chat"}}
        monitors = [{"id": 1, "name": "DP-1", "specialWorkspace": {"name": "special:codex"}}]
        for lua in (True, False):
            with self.subTest(lua=lua):
                commands = r.focus_commands(client, monitors, lua)
                self.assertEqual(len(commands), 5)
                self.assertIn("codex", commands[1][-1])
                self.assertIn("chat", commands[-2][-1])
                self.assertIn("address:0xabc", commands[-1][-1])

    def test_focus_normal_window_closes_unnamed_special_workspace(self):
        client = {"address": "0xabc", "monitor": 1, "workspace": {"name": "2"}}
        monitors = [{"id": 1, "name": "DP-1", "specialWorkspace": {"name": "special"}}]
        self.assertEqual(r.focus_commands(client, monitors)[1][-1], 'hl.dsp.workspace.toggle_special("")')
        self.assertEqual(r.focus_commands(client, monitors, False)[1], ["hyprctl", "dispatch", "togglespecialworkspace", ""])

    def test_failed_launch_does_not_increment_count(self):
        (self.root / "runtjik.conf").write_text("[index]\ndesktop_apps=false\n")
        self.command(self.root / "bin")
        launcher = self.launcher()
        with patch.object(r, "dispatch", side_effect=RuntimeError("failed")):
            with self.assertRaises(RuntimeError):
                launcher.activate("commands:example")
        self.assertNotIn("launch:commands:example", launcher.store.counts())

    def test_success_updates_stats_immediately(self):
        (self.root / "runtjik.conf").write_text("[index]\ndesktop_apps=false\n")
        self.command(self.root / "bin")
        launcher = self.launcher()
        with patch.object(r, "dispatch"):
            launcher.activate("commands:example")
        self.assertEqual(launcher.store.counts()["launch:commands:example"]["count"], 1)

    def test_config_rejects_invalid_settings(self):
        for content in ('[ui]\ncolumns=0', '[ui]\nview="tiles"', '[ui]\nwidht=700', '[launch]\nterminal=[]'):
            with self.subTest(content=content):
                (self.root / "runtjik.conf").write_text(content)
                with self.assertRaises(ValueError):
                    r.config()

    def test_config_command_preserves_comments_and_unrelated_settings(self):
        path=self.root / "runtjik.conf"
        original='# personal\n[ui]\nview = "grid" # keep this\ncolumns = 5\n\n[theme]\naccent = "#ff53a6"\n'
        path.write_text(original)
        settings=r.save_setting('ui.view', 'list')
        self.assertEqual(settings['ui']['view'], 'list')
        saved=path.read_text()
        self.assertIn('# keep this', saved)
        self.assertIn('columns = 5', saved)
        self.assertIn('[theme]\naccent = "#ff53a6"', saved)
        self.assertTrue(saved.startswith('# personal'))
        r.save_setting('history.enabled', 'false')
        self.assertFalse(r.config()['history']['enabled'])

    def test_invalid_config_command_does_not_touch_file(self):
        path=self.root / 'runtjik.conf'
        original='[ui]\ncolumns=5\n'
        path.write_text(original)
        for key,value in [('ui.columns','0'),('ui.view','tiles'),('plugins.enabled','[3]'),('history.enabled','"yes"')]:
            with self.subTest(key=key,value=value),self.assertRaises(ValueError):
                r.save_setting(key,value)
            self.assertEqual(path.read_text(),original)

    def test_slash_commands_are_alphabetical_including_filtered_results(self):
        launcher = self.launcher()
        for query in ("/", "/s", "/r"):
            with self.subTest(query=query):
                names = [row["name"] for row in launcher.query(query)["results"]]
                self.assertGreater(len(names), 1)
                self.assertEqual(names, sorted(names, key=str.casefold))

    def test_launcher_commands_use_companion_artwork_in_all_views(self):
        launcher = self.launcher()
        rows = {row["id"]: row["icon"] for row in launcher.query("/")["results"]}
        self.assertEqual(rows, {"/processes":"runtjik-stats", "/run":"org.gnome.Terminal", "/refresh":"runtjik-refresh", "/pin":"runtjik-pin",
                               "/grid":"runtjik-grid", "/list":"runtjik-list",
                               "/stats":"runtjik-stats", "/config":"runtjik-config",
                               "/bindings":"runtjik-shortcuts", "/define":"accessories-dictionary", "/synonym":"accessories-dictionary", "/cliphist":"edit-paste", "/rec":r.Recorder.icon, "/grab":r.Screenshots.icon, "/todo":r.Todo.icon})
        self.assertEqual({row["icon"] for row in launcher.query("/config")["results"]}, {"runtjik-config"})
        self.assertEqual({row["icon"] for row in launcher.query("/shortcuts")["results"]}, {"runtjik-shortcuts"})
        launcher._icons.resolve.side_effect = OSError("Artwork unavailable")
        self.assertEqual(len(launcher.query("/")["results"]), 15)

    def test_shortcuts_alias_has_identical_read_only_rows(self):
        (self.root / 'runtjik.conf').write_text('[index]\ndesktop_apps=false\n')
        launcher=self.launcher()
        a=launcher.query('/shortcuts')['results']
        b=launcher.query('/bindings')['results']
        self.assertEqual(a,b)
        self.assertTrue(all(row['disabled'] for row in a))
        self.assertIn('Super+Escape',[row['name'] for row in a])

    def test_config_command_applies_and_rejects_without_launch(self):
        path=self.root / 'runtjik.conf'
        path.write_text('[index]\ndesktop_apps=false\n')
        launcher=self.launcher()
        self.assertTrue(launcher.query('/config ui.width 1')['results'][0]['disabled'])
        with patch.object(r,'windows',return_value=[]):
            result=launcher.activate('/config ui.columns 6')
        self.assertTrue(result['apply_config'])
        self.assertEqual(launcher.settings['ui']['columns'],6)
        self.assertEqual(r.config()['ui']['columns'],6)

    def test_plugin_contract_dynamic_results_and_actions(self):
        (self.root / "runtjik.conf").write_text('[index]\ndesktop_apps=false\n[plugins]\nenabled=["ExampleTabs"]\n')
        path = r.xdg("config") / "plugins/ExampleTabs.py"
        path.parent.mkdir(parents=True)
        path.write_text('''API_VERSION = 1
class Provider:
    name = "ExampleTabs"
    commands = {"/tabs": "Browser tabs"}
    def query(self, query, settings):
        return [{"id":"ExampleTabs:123", "provider":self.name, "name":"Example tab"}]
    def activate(self, entry, settings, mode):
        self.last_action = (entry.id, mode)
''')
        launcher = self.launcher()
        with patch.object(r, "windows", return_value=[]):
            result = launcher.query("example")
        self.assertEqual(result["results"][0]["provider"], "ExampleTabs")
        launcher.activate("ExampleTabs:123", "switch")
        self.assertEqual(launcher.providers["ExampleTabs"].last_action, ("ExampleTabs:123", "switch"))
        self.assertEqual(launcher.activate("/tabs"), {"kind": "edit", "text": "/tabs "})
        self.assertEqual(launcher.query("/tabs irrelevant-to-title")["results"][0]["id"], "ExampleTabs:123")
        self.assertIn("/tabs", {row["id"] for row in launcher.query("/")["results"]})

    def test_launch_uses_argv_without_shell_and_separate_service(self):
        with patch.dict(os.environ, {"DBUS_SESSION_BUS_ADDRESS": "test"}), patch.object(r.shutil, "which", return_value="/bin/tool"), patch.object(r.subprocess, "run") as run:
            run.return_value.returncode = 0
            r.dispatch(["tool", "$(touch /tmp/never)", "a b"], self.settings)
        argv = run.call_args.args[0]
        self.assertEqual(argv[-3:], ["tool", "$(touch /tmp/never)", "a b"])
        self.assertIn("--property=Type=exec", argv)
        self.assertIn("--property=ExitType=cgroup", argv)
        self.assertNotIn("shell", run.call_args.kwargs)


if __name__ == "__main__":
    unittest.main()
