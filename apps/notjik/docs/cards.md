# Card types and calling Notjik

Notjik accepts ordinary desktop notifications and richer cards from `notjikctl`.
The CLI uses Quickshell IPC; a running Notjik session is required.

```sh
notify-send 'Flight recorder secured' 'Black box synced.'
notjikctl send 'Flight recorder secured' 'Black box synced.' --app flight-deck
```

`notify-send` uses the standard desktop D-Bus interface. Use `notjikctl` when you
need structured status, timers, progress or explicit command buttons.

## Types at a glance

| Card | How to request it | Data |
| --- | --- | --- |
| Standard | `--type standard` (default) | Title and optional body |
| Icon / artwork | Any type with `--icon` or `--image` | Icon name, or local image path |
| Status / error | `--type status` | `state`, optional `detail`, `tone` |
| Progress / update | `--type progress` | `value` from 0 to 100, optional `label` |
| Results table | `--type results-table` | `results` rows, optional `state`, `tone`, `duration` |
| Countdown | `--type timer` | `seconds`, optional `label`, `doneLabel` |
| Multiple actions | Any type with `--actions` | JSON array of labels and command argument arrays |
| Urgency | Any type with `--urgency` | `low`, `normal`, `critical` |
| Grouped | Any type with `--group` | Shared group key within the sender application |
| Telegram | Optional Telegram bridge | Text, photo, voice, sticker, emoji and animation |

Icon, error, update, urgency and grouped are presentation variations, not separate
`--type` values. Telegram cards come from the bridge; the synthetic demo provides
an account-free way to inspect them.

## Standard cards and artwork

```sh
notjikctl send 'Relay link established' 'Earth, eventually.' \
  --app flight-deck --icon network-wireless-symbolic --timeout 8s
notjikctl send 'Surface survey' 'First light on Nacre.' \
  --image /absolute/path/surface.png --timeout never
```

`--icon` uses an installed icon name. `--image` supplies local artwork. Omitting
artwork allows the configured robot fallback. `--timeout` accepts durations such
as `8s`, `2m` and `never`; omitting it uses the configured urgency timeout.

## Status and errors

```sh
notjikctl send 'Relay link established' 'Engineering · deck 4' --type status \
  --data '{"state":"Connected","tone":"success","detail":"Deep-space relay online"}'
notjikctl send 'Reactor fault' 'Engineering · deck 4' --type status \
  --urgency critical --timeout never \
  --data '{"state":"Failed","tone":"critical","detail":"Containment field unstable"}'
```

`tone: "critical"` uses the error accent; `"success"` uses the success accent.
Other tone values use the theme accent. Urgency controls the notification's
priority and default lifetime independently of its status text.

Set `"rainbow-text": true` to render a status state character-by-character in
Tjikup's six-colour rainbow palette. This is intended for celebratory, compact states
such as `ALL SYSTEMS GO`; the status tone still controls the leading indicator.

## Update one card in place

Use the same `--app` and `--id` for each update. IDs are scoped to the application.

```sh
notjikctl send 'Star chart transfer' --app navigation --id sector-map \
  --type progress --data '{"value":10,"label":"Receiving star charts"}' --timeout never
notjikctl send 'Star chart transfer' --app navigation --id sector-map \
  --type progress --data '{"value":75,"label":"Resolving jump coordinates"}' --timeout never
notjikctl send 'Sector mapped' --app navigation --id sector-map --timeout 5s
```

`send` prints the actual notification ID. You can also capture it and pass it to
`--replace`; sender-chosen `--id` is more convenient for long-running scripts.

## Timers

```sh
notjikctl send 'Jump window' --type timer --timeout never \
  --data '{"seconds":30,"label":"Spooling the jump drive","doneLabel":"Coordinates locked"}'
```

The countdown is live. `--timeout never` keeps the finished card visible; its
notification lifetime and countdown duration are separate settings.

## Action buttons

```sh
notjikctl send 'Unidentified cargo aboard' 'It is warm. It is humming.' \
  --actions '[{"label":"Open manifest","command":["xdg-open","https://github.com/tijptjik/tjikos"]},{"label":"Open home folder","command":["xdg-open","."]}]'
```

Buttons reveal on hover or keyboard selection. Commands are argument arrays,
not shell strings, and run only when invoked. `--resident` keeps a notification
open after an action. Native D-Bus actions are returned to the originating app;
they do not grant that app arbitrary command execution through notification hints.

## Grouping and history

```sh
notjikctl send 'Maya' 'That moon was not on the chart.' --app bridge --group survey
notjikctl send 'IO' 'Neither was the second one.' --app bridge --group survey
notjikctl send 'Maya' 'Tell me those are moons.' --app bridge --group survey
notjikctl history show
notjikctl history hide
```

With grouping enabled, related notices share a card and count indicator. Hovering
the grouped card opens its conversation in notification history. History is local;
`--transient` excludes a notice unless `history.logTransient` is enabled.

## Telegram cards

The optional bridge supplies text replies, photo previews and mark-as-read,
voice playback and recording, and animated media with quote replies.
Follow [Telegram setup and account requirements](../README.md#telegram-photos-and-voice-messages)
for a real account. Media and microphone support have additional dependencies.

To preview those interactions with synthetic messages and no account or microphone:

```sh
notjikctl demo cycle --only telegram-text telegram-photo telegram-voice telegram-gif
```

These directed demos simulate sends and recording. They do not send Telegram messages.

## Controls and scripting

```sh
notjikctl list                   # active cards and their IDs
notjikctl status                 # server, queue and privacy state
notjikctl pause                  # temporarily hide notifications
notjikctl resume
notjikctl dismiss local-1        # use an ID returned by send or list
notjikctl logs --limit 20 --json
notjikctl send --help
```

See [configuration](../README.md#configuration),
[keyboard and history controls](../README.md#history-and-keyboard-navigation),
[custom QML components](../README.md#styling-and-components), and
[D-Bus compatibility](comparison.md) for the full contracts and current limits.

## Results tables

`results-table` is built into Notjik and needs no `components` registration.
Each row in `results` has a `label`, `value`, optional `tone`, and optional
`ingredients`: secondary rows with `label` and `value`. The footer accepts
`state` (defaults to `Running`), `tone`, and `duration`. Critical row values use
the error accent; other values use the theme accent. Footer tone supports
`critical`, `success`, or the theme accent. Set `"rainbow-text": true` to colour
the footer state character-by-character, matching Tjikup's completion banner.
Text is rendered literally, including when rainbow colouring is enabled.

```sh
notjikctl send 'Surface survey complete' --app survey --id sector-report \
  --type results-table --timeout never \
  --data '{"results":[{"label":"Sectors mapped","value":"12","ingredients":[{"label":"New signals","value":"3"}]}],"state":"Complete","tone":"success","duration":"42s"}'
```

Sending the same application and ID again updates the table in place. The QML
component ships at `qml/components/ResultsTable.qml` in the Notjik package.
