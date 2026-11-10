"""Validate injected preview scheduling on a silent workspace-7 display."""
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "packages/tjik-gtk/scripts"))
from display import rendering_display

native = ROOT / "packages/tjik-gtk/target/native/lib"
env = {**os.environ, "GTK_A11Y": "none", "PKG_CONFIG_PATH": str(native / "pkgconfig"),
       "LD_LIBRARY_PATH": str(native)}
command = ["cargo", "test", "--manifest-path", str(ROOT / "packages/tasktjik-gtk/Cargo.toml"),
           "--locked", "--lib"]
subprocess.run([*command, "--no-run"], env=env, check=True)
subprocess.run(command, env=env, check=True)
with rendering_display():
    for test in ("snooze_calendar_selects_date_and_rejects_past_times", "optimistic_actions_queue_without_waiting_for_server", "completion_draws_then_fades_then_reflows", "task_application_keyboard_and_host_boundary", "previews_bound_batches_reject_stale_results_and_cancel_on_drop",
                 "media_workers_preserve_responsiveness_and_reject_stale_completion",
                 "reading_virtualization_preserves_navigation_groups_and_resize",
                 "virtual_columns_share_scroll_and_bound_widgets",
                 "task_virtualization_preserves_focus_and_revision"):
        subprocess.run([*command, test, "--", "--ignored"], env={**os.environ, **{k: env[k] for k in
                       ("GTK_A11Y", "PKG_CONFIG_PATH", "LD_LIBRARY_PATH")}}, check=True)
