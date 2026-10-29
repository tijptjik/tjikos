# Source inventory

Inspected in the Dotjik policy checkout before implementation:

| Source | Retained behavior / implementation |
| --- | --- |
| Waybar config.tmpl; Hypr workspaces.lua.tmpl | Monitor assignments are preset references; actual ownership comes from compositor snapshots/events. EN/拼/速 follows clock; no Input Method tray duplicate. |
| mediaplayer.py | Native MPRIS replaces PyGObject Playerctl; dynamic discovery, metadata and controls. Explicit Quit binding replaces killall. |
| crypto wrapper and installed crypto/config.ini, crypto/waybar_crypto.py, crypto/src/waybar_crypto.py | Configured BTC/SOL/AR, precision 0/0/1, currency USD and empty currency prefix; separate currency items. Key values were never printed/copied. Optional INI reader and exported environment support migration. |
| mullvad.py, tailscale.fish | Connection/location indicator and structured tailnet states; Mullvad account query intentionally omitted so account credentials never enter tooltips. |
| quickshell/audio/audio.py, shell.qml, pointer.py | Independent route/volume targets, 4-point volume steps, stream moving and direct anchors. Discovered/configured routes replace copied device names. |
| quickshell/network/network.py, shell.qml, pointer.py | Interface-aware state and existing panel IPC. nmcli queries avoid adding PyGObject/libnm to the bar runtime. |
| toggle-network, toggle-audio | Adapter calls toggleAt or launches with anchor environment; does not copy the Waybar/AT-SPI lookup. |
| input-method.py | EN/拼/速 mappings and active group profile cycling, polling external changes. |
| codex.py | Maintained `quota.py` calculation/query, SQLite read-only URI, weekly duration selection for both API windows. |
| openrouter.py | Maintained `credits.py` balance/activity formatting, bounded credential-safe fetch implementation. |
| notifications.fish and notjik README/notjik.py/qml/Logic.js/themes/tests | Current status/pause/resume/toggle boundary; XDG configuration, last-valid reload, named themes and argv conventions. No notification-server ownership or notjik edits. |
| powerbutton.fish | Effective hypridle/hyprsunset semantics; explicit suspend/optional poweroff bindings, mocked in tests. |
| monitor-game-mode, monitor-desktop-mode, monitors.lua.tmpl and helpers.lua | Existing persisted mode + Lua reload, never legacy keyword monitor. Workspace dispatch uses supported hl.dsp.focus and workspace.toggle_special forms. |

Runtime commands: Python, Quickshell, hyprctl, nmcli, pactl, date, fcitx5-remote,
pgrep/pkill; optional mullvad, tailscale, notjikctl, existing monitor mode commands,
GUI launchers, systemctl for explicitly configured power actions. No app runtime
requires the old Waybar scripts. Only optional crypto `legacyConfig` and the
external audio/network panels are migration/integration paths.

Installed Quickshell 0.3.2 qmltypes were inspected for MPRIS, SystemTray, Region,
PanelWindow, WlrLayershell and test APIs. Tray menu rendering uses QsMenuOpener and PopupWindow; the root
`UseQApplication` pragma is retained for Qt compatibility. Native tray images load synchronously because the native
provider's Qt object affinity is not suitable for the asynchronous image reader.
