# Native acceptance

GTK is the sole bar frontend. The service uses `bartjik` with configured edge
reservation. Native assets, helpers and control sockets replace the retired frontend
payload. Historical implementation reports are retained under the repository's
`docs/gtk-migration-history/`; they are not current runtime instructions.

## Verified on 2026-11-10 before cleanup

- 30 backend and 42 GTK unit tests passed; 6 interactive tests require explicit runners.
- Shared desktop adapter: 13 unit tests and 1 external-adapter contract passed.
- `detached_toplevel_render` passed in the current compositor, with pre-map workspace-7
  placement, independent windows, scoped close and handoff snapshot cleanup.
- `gtk_panel_primitives.py --transition` passed: fixed destination allocation, clipping,
  interrupted transitions and equal-size handoff.
- `gtk_input_region.py` passed: the hidden/resized bar releases its Wayland buffer and
  remaps on reveal. The fixture was silently placed on workspace 7 and did not take
  focus.

## Coverage retained after renderer removal

| Area                                               | Native coverage                                                      |
| -------------------------------------------------- | -------------------------------------------------------------------- |
| Dwell, corridor, pin and panel ownership           | `src/gtk/panels.rs`, `panel_view.rs`, transition fixture             |
| Styled provider cards                              | `gtk_panel_primitives.py` with synthetic per-provider rows           |
| SNI, DBusMenu, media and owner changes             | `tests/native_services.py`, `src/native.rs`                          |
| Network and audio commands, stale IDs and failures | `tests/network_adapter.py`, `audio_tools.py`, GTK controller tests   |
| Workspace activation and restart generations       | shared desktop adapter unit and contract tests                       |
| Provider configuration, policy and data            | Rust tests, `native_contracts.py`, retained domain oracles           |
| Recording indicators and charts                    | activity helpers, `screen_record_status.py`, chart/motion unit tests |

Old renderer-specific screenshot and interaction runners were removed with their
implementation. Their exact Qt event coverage is not claimed by the native replacements.
The remaining isolated-compositor harness is for a separately authorized lab
environment; do not run it on this managed desktop.

## Remaining hands-on checks

Physical CJK composition, real device disconnect/reconnect, denied authorization through
real agents, and physical mixed-DPI/rotated thumbnail sharpness remain unverified in
this pass. Synthetic service failures do not replace those checks. Visual review of
flicker under real pointer movement also remains a human acceptance item. Cross-monitor
movement and output removal were accepted by the user. Drag-back docking is
intentionally excluded.

Only one detached view per provider is supported. Native pre-map placement and live
window capture currently require the Hyprland adapter. Unsupported adapters must report
that limitation instead of routing actions to another compositor.

The matched startup/idle measurements are recorded at repository level in
[desktop acceptance](../../../docs/desktop-native-acceptance.md). Active media/thumbnail
GPU workloads have not been measured in the current acceptance pass.
