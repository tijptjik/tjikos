"""Focused Reading keyboard checks on an isolated workspace-7 compositor."""
import os
from pathlib import Path
import subprocess
import sys

root = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(root / 'packages/tjik-gtk/scripts'))
from display import rendering_display

native = root / 'packages/tjik-gtk/target/native/lib'
env = {**os.environ, 'GTK_A11Y': 'none', 'PKG_CONFIG_PATH': str(native / 'pkgconfig'),
       'LD_LIBRARY_PATH': str(native), 'CARGO_BUILD_JOBS': '2'}
command = ['cargo', 'test', '--manifest-path', str(root / 'packages/tasktjik-gtk/Cargo.toml'), '--locked', '--lib']
subprocess.run([*command, '--no-run'], env=env, check=True)
with rendering_display():
    for test in ('project_incoming_category_routes_only_inbox', 'projects_follow_filters_and_back_navigation', 'task_editor_pastes_text_and_images', 'reading_keyboard_survives_rebuild_and_maps_enter_modifiers', 'task_application_keyboard_and_host_boundary', 'reading_snooze_returns_to_reading_and_sends_selected_item', 'reading_dates_include_due_and_snooze_horizons', 'responsive_headers_and_reading_dates'):
        subprocess.run([*command, test, '--', '--ignored'], env={**os.environ, **{key: env[key] for key in
                       ('GTK_A11Y', 'PKG_CONFIG_PATH', 'LD_LIBRARY_PATH', 'CARGO_BUILD_JOBS')}}, check=True)
