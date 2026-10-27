# tjikOS showcase films

The `/os` gallery at TYPE.hk uses a fixed **1200 × 720** frame. Capture the tools
at that geometry, at **60 fps**, with silent H.264 / yuv420p / faststart MP4 output.
Use the same `scripts/ShowcaseStage.qml` frame for all six films. Each app supplies
its own native content and a small director for its real input paths.

## Shared sequence

1. **Identity / promise, about 2 seconds.** Tool icon, one short headline, and one
   sentence about the features to follow.
2. **Two to five feature beats, about 6–15 seconds each.** One headline and a supporting
   sentence at the top. The real application occupies the centre. The chapter
   footer advances between features. Native transitions, pointer movement,
   keyboard input, and enough resting time make each change understandable.
3. **Identity / source, about 3 seconds.** Large tool icon and name, the actual
   repository path, and the six-tool family in its gallery order.

The Rose Punk palette is background `#191724`, panel `#1f1d2e`, surface `#26233a`,
text `#e5e5e5`, muted `#a5a0ba`, pink `#ff53a6`, and cyan `#9ccfd8`. Use `tijpset`,
the native desktop font alias. Leave 48 px at the sides, the top 190 px for the
identity/captions, and the bottom 70 px for the shared footer. Native panels are
sized and placed through temporary demo configuration. Keep panel contents,
rendering, hit testing, and interaction handlers intact.

Suggested feature stories for the remaining tools (not recorded yet):

| Tool | First beat | Second beat | Third beat |
| --- | --- | --- | --- |
| Notjik | Progress and status | Grouped history and inline reply | Voice playback/recording and urgency |
| Bartjik | Expand a live status panel | Move between media/workspace controls | Detach and move a panel |
| Runtjik | Filter the app grid | Switch to a window | Find a shortcut |
| Tjikup | Start a demo workflow | Follow parallel stages | Read the results |
| Mailtjik | Search demo mail | Open and read a result | Browse archive status |
| Greetjik | Render a greeting | Change character/style | Pipe a message into the tool |

## Record Notjik

From the Tooltjik repository root:

```sh
python3 -B scripts/os_showcase.py notjik --plan
python3 -B scripts/os_showcase.py notjik "$HOME/Videos/tooltjik/os/notjik-v4.mp4"
```

Choose an unused output name. Existing MP4s, JSON receipts, and review folders
are never overwritten. Captures and account data belong outside the repository.
Dependencies: Hyprland with the current Lua API, Quickshell, D-Bus, FFmpeg,
wf-recorder, grim, wtype, a C compiler, pkg-config, and wayland-scanner. The
installed Rose Punk tooltip style and fontconfig aliases are copied into the
private environment when present.

The recorder starts its own nested Hyprland, headless output, D-Bus, runtime,
configuration, cache, state, and a temporary copy of Notjik. It never reuses a
running comparison session or changes the installed daemon. A private Wayland
virtual pointer supplies frame-paced minimum-jerk motion and native button
events. `wtype` supplies native key events. The public `notjikctl history show`
command opens history, followed by a native click to acquire keyboard focus and
`/` to filter. The existing demo Telegram handler accepts the inline reply;
no message is sent to an account. The Telegram bridge is disabled in the private
copy. Voice playback uses the bundled WAV with muted output. Voice recording
runs the unmodified production recorder and FFmpeg/Opus pipeline with that WAV
as its real-time input, then uses the native reset control to discard the draft.
It never opens the microphone. All displayed messages are demo fixtures.

The only QML instrumentation adds the background stage, read-only target geometry
and state queries, a drawer viewport sized to the stage, and centred presentation geometry.
The 440 px feature area starts at y=200; the native notification stack is centred
at (600, 420) and stays centred as reply/recording controls expand or collapse. It contains no
QtTest events, direct click-handler calls, direct writes to reply text, or
recreated application panels. Normal IPC supplies demo notifications and native
progress updates; normal input performs the shown UI actions.

Each successful capture produces:

- `notjik-v4.mp4`: encoded gallery film.
- `notjik-v4.json`: dimensions, duration, SHA-256, feature timestamps, input
  evidence, and verification results.
- `notjik-v4.review/`: actual compositor screenshots at each major state, plus
  application, compositor, and recorder logs.

The recorder verifies real hover, a narrowed history search, the typed reply,
dismissal through the native demo reply handler, playback position advancing,
recorder progress exceeding three seconds, all three urgency levels, and the
same card centre before/after opening and resetting controls before completing
the file.
Inspect the recorded transitions as well as the still frames before publishing.
Copy the chosen film and a real captured poster into TYPE.hk's
`public/os-previews/notjik-showcase.{mp4,webp}`; keep its existing 1200/720 layout.
