# GTK cards

Notjik's GTK card library lives in `src/gtk/` behind the `gtk-ui` Cargo feature. The
installed synthetic gallery runs with:

```sh
notjik --gtk-cards --demo
notjik --gtk-cards --demo --theme monochrome
```

This gallery does not claim the notification D-Bus name. It uses synthetic notices,
simulates action completion and voice playback/recording, and never sends Telegram
messages, executes notification commands or opens a microphone. Quickshell continues to
deliver desktop notifications. The Control Center/history panel is deferred to a BarTjik
panel.

## Reusable components

| Component    | Responsibility                                                                                                                                     |
| ------------ | -------------------------------------------------------------------------------------------------------------------------------------------------- |
| `Card`       | Stable outer frame, artwork/initials rail, title/body, urgency, grouping control, selection, dismissal, drag intents and Shift-click Markdown copy |
| `primitives` | Theme CSS, wrapping text, field rows, nested results, status strip, literal rainbow text and paged action layouts                                  |
| `details`    | Status, progress, countdown, results, key/value, auth code and todo content; shared snooze presets and calendar                                    |
| `media`      | Local image, GIF and silent video surfaces with unavailable-media fallbacks and visibility/privacy gating                                          |
| `telegram`   | Message text, inline emoji surfaces, media, waveform/seek controls, transcript, reply/quote editor and recording controls                          |
| `clipboard`  | Content-only Markdown export, independent of GTK widgets and action commands                                                                       |
| `model`      | Notice snapshots, host view state, playback/recording state and typed action intents                                                               |

## Renderers

| Type            | GTK presentation and controls                                                                                                          |
| --------------- | -------------------------------------------------------------------------------------------------------------------------------------- |
| `standard`      | Title, body, icon/artwork, urgency and action buttons; unknown/custom types retain this fallback                                       |
| `status`        | Tone, state, detail and optional six-colour rainbow text                                                                               |
| `progress`      | Percentage, label and bounded progress bar; replace the snapshot to update                                                             |
| `timer`         | Countdown from received/updated time, separate from notification expiry; host supplies ticks                                           |
| `results-table` | Field/value rows, ingredient rows, row tones, state and duration                                                                       |
| `key-value`     | Parsed heading and fields replacing the raw body                                                                                       |
| `auth-code`     | Authentication code and explicit clipboard copy                                                                                        |
| `todo`          | Group/due metadata, Done/Dismiss intents, snooze presets and calendar                                                                  |
| `telegram`      | Text, photo, sticker, emoji, GIF/video animation and voice variants; reply, quote, read, open, transcribe and voice-recording controls |

Actions use quiet, paged footers. Telegram uses the original SVG icons and circular play
control; auxiliary controls reveal on hover, keyboard focus or active interaction. Plain
text is never interpreted as markup. Telegram's generated StyledText is converted to
Pango markup only when valid, with plain-message fallback. Media paths are local;
widgets do not fetch remote image URLs.

Text and icon footers share a 30-pixel control row, a 6-pixel separator gap and an
8-pixel bottom inset. Authentication cards keep their centered, borderless Copy code
action visible even when the other card controls are collapsed.

Robot artwork retains the existing urgency pools and stored indices. Fish artwork
resolves stored names and legacy indices through the existing manifest. A same-ID
replacement preserves the reply draft and its original quoted-message target; a new
identity clears that state.

## Host integration

Create `Card::new(notice, state, resource_root, now_ms, callback)`, mount its `widget`,
and supply changes through `update`, `set_state` and `tick`. The host receives a
`CardEvent` containing the notification ID and a typed `Intent`; it remains responsible
for validating the action against the current notification and dispatching native D-Bus
actions, local commands, task operations and Telegram requests.

`ViewState` defaults to locked. Set the confirmed lock/privacy state explicitly. Locked
or paused cards are hidden; archived, history, blocked and busy cards disable actions.
The host handles `Hold` and hover events when calculating expiry. It also supplies group
counts and handles group-opening intents; this library does not build a history panel.
Feed parsed handler presentations as `key-value` snapshots.

Voice playback, speed/seek and recording use explicit host state and intents. The
widgets do not own an audio subprocess or Telegram connection. A production GTK host
must wire those effects to its playback/recording service, propagate failures and
uncertain sends, and clear a reply only after confirmed success with `clear_reply`.
Animation/image display itself is implemented in the card library. This separation keeps
synthetic previews account-free and prevents duplicate side effects during the frontend
migration.

Custom QML components cannot run inside GTK. They currently use the standard fallback;
new native extensions can compose the shared primitives. Production renderer selection
must not change until the notification controller, native IPC/D-Bus adapters and media
action executor are integrated.

## Validation

`tests/gtk_cards.py` launches a private Wayland compositor silently on workspace 7,
checks focus/placement, renders on a private headless output, loads the bundled
synthetic media and exercises 19 card variants in rose-punk and monochrome at two
widths. It covers media readiness and advancing animation frames, original QML reference
captures, icon controls and hidden/revealed states, action dispatch, snoozing, reply
draft preservation, quote-target stability, lock/history gating and uncertain recording
controls. It exports individual card PNGs and a gallery screenshot to a temporary
artifact directory, never capturing the user's desktop.

```sh
dev-check python3 -B apps/notjik/tests/gtk_cards.py
```
