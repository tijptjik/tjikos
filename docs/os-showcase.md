# tjikOS showcase films

The `/os` gallery at TYPE.hk uses a fixed **1200 × 720** frame. Capture the tools
at that geometry, at **60 fps**, with H.264 / yuv420p / faststart MP4 output. All showcase MP4s are silent and contain no audio track.
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

Feature stories (all six films have reviewed native recordings):

| Tool | First beat | Second beat | Third beat |
| --- | --- | --- | --- |
| Notjik | Progress and status | Grouped history and inline reply | Voice playback/recording and urgency |
| Bartjik | Public rates, pointer tracking, bass-driven real artwork | Real workspace previews; VPN, Tailscale, usage, audio, clocks and mode | Exponential detach release; move and retain a chart beside another panel |
| Runtjik | Filter the app grid | Choose an open window | Browse running apps and close them gracefully |
| Tjikup | Start a real workflow | Follow parallel stages with app icons | Rich report with counts, timings and verified outcomes |
| Mailtjik | Search demo mail | Open and read a result | Browse archive status |
| Greetjik | Render a greeting | Change character/style | Pipe a message into the tool |

## Record Notjik

From the TjikOS repository root:

```sh
python3 -B scripts/os_showcase.py notjik --plan
python3 -B scripts/os_showcase.py notjik "$HOME/Videos/tjikos/os/notjik-v8.mp4"
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

- `notjik-v8.mp4`: encoded gallery film.
- `notjik-v8.json`: dimensions, duration, SHA-256, feature timestamps, input
  evidence, and verification results.
- `notjik-v8.review/`: actual compositor screenshots at each major state, plus
  application, compositor, and recorder logs.

The recorder verifies real hover, a narrowed history search, the typed reply,
dismissal through the native demo reply handler, playback position advancing,
recorder progress exceeding three seconds, all three urgency levels, and the
same card centre before/after opening and resetting controls before completing
the file.
Inspect the recorded transitions as well as the still frames before publishing.
Copy the chosen film and a real captured poster into TYPE.hk's
`.local/site-assets/os-previews/notjik-showcase.{mp4,webp}`; keep its existing 1200/720 layout.

The Notjik poster uses the opening identity frame: **Notifications with flair**.
Its feature line is **Live updates. Telegram integration. Replies in place.**


## Record Bartjik

```sh
python3 -B scripts/bartjik_showcase.py --plan
python3 -B scripts/bartjik_showcase.py "$HOME/Videos/tjikos/os/bartjik-next.mp4" \
  --data "$HOME/Videos/tjikos/os/bartjik-public-20261028/snapshot.json"
```

The reviewed revision is `bartjik-v4.mp4`. It uses the same 1200 × 720, 60 fps
Rose Punk stage, the opening poster **Panels at your fingertips**, and six chapters:
real market charts and pointer tracking; music; workspace previews/switching;
VPN/Tailscale; usage/audio/clocks/mode; and independent detachable charts.
Each chapter has an explanatory heading and caption. The outro shows Bartjik's
icon, name and app repository path. There is no comparison renderer.

The main bar holds markets, music and workspaces. A second bar holds system and
connection controls. Both appear together first; the lower bar receives its own
section so its panels and buttons have clear space. The native compositor uses
1.2 scale; stage typography stays fixed to its 1200 × 720 reference canvas.

The public snapshot contains fetched CoinGecko BTC/USD, SOL/USD and AR/USD
quotes, changes and 289 actual 24-hour points per coin. It also contains Karla
Blum's **Acid Rain** cover and public iTunes preview, with original URLs,
fetch time and SHA-256 receipts. Source responses, artwork and audio remain
outside Git beside `snapshot.json`. The recorder uses those exact chart points.
Qt MediaPlayer plays the real preview muted in the private session; a 25 ms
bass envelope derived from the same audio drives native album-art movement.
FFmpeg exports video only; the public preview is used solely to drive the visual pulse. The live artwork
meter reads playback streams with `--playback-only` and never queries microphones.
World clocks use the actual current time.

Workspace previews capture actual Kitty/Vim windows displaying public source
files in the private compositor, then a native click switches its real workspace.
The screencopy view is initialized after it has a native window. VPN/Tailscale,
quota, audio-device and mode data/actions stay isolated demo state; this film
never changes the host network, VPN, volumes or monitor mode. Native pointer
motion, clicks, scrolling and handlers remain active. No QtTest events or direct
click-handler invocation is used.

Detachment preserves the snapshot until the native floating window is placed,
then uses a 420 ms exponential release with a small scale settle. The receipt
records release progress, frame counts, actual compositor geometry and the
independence of the two charts. A second real pointer gesture moves the native
window and its native close control finishes the sequence. MP4, JSON evidence
and `.review/` frames/logs follow the no-overwrite convention used by Notjik.
Inspect the actual recorded transitions before publishing. Stage versioned media
in TYPE.hk's `.local/site-assets/os-previews/` and use its task-scoped R2 uploader.

## Record Runtjik, Mailtjik, Tjikup and Greetjik

All four use `scripts/suite_showcase.py`, the standalone `SuiteStage.qml` host,
and the same 1200 × 720 / 60 fps Rose Punk frame. They end with the tool icon,
name, app repository path and family icons. Each poster is a real opening frame.

```sh
python3 -B scripts/suite_showcase.py runtjik --plan
python3 -B scripts/suite_showcase.py runtjik "$HOME/Videos/tjikos/os/runtjik-next.mp4"
# Replace runtjik with mailtjik, tjikup or greetjik for the other films.
```

The current masters are `runtjik-v2.mp4`, `mailtjik-v1.mp4`, `tjikup-v2.mp4` and
`greetjik-v1.mp4` under `~/Videos/tjikos/os/`, with JSON input receipts and
`.review/` frames, terminal text and logs. The same no-overwrite rules apply.
Build the current Mailtjik, Tjikup and Greetjik executables into
`/tmp/tjikos-showcase-build/bin/` first. The terminal runs without shell startup
files, uses a private history, and removes inherited `NO_COLOR`. Native Kitty
geometry is verified before recording the app. No private mail, user history,
real network actions or production workflow is used.

- **Runtjik — Find it. Run it.** Its actual demo renderer shows a complete app
  grid, fuzzy search, two-instance picker and a task-manager view that gracefully
  closes applications and reflows the remaining running apps.
  Native keyboard events do the work. Cooperative GTK fixtures on private workspaces
  receive real compositor close requests; event receipts and zero exit codes
  verify shutdown. Only manifest-allowlisted fixture processes can be targeted. A temporary UI copy bounds the grid and
  centres it below the caption. Installed Rose Punk application icons are used.
  The current managed primary `runtjik.svg` is copied to `docs/icons/` before
  capture; its SHA-256 is retained in the film receipt.
- **Mailtjik — Your mail, worth keeping.** Its production Go TUI and MIME reader
  show combined participant/subject/year filters, a rich message and inline
  image, the EML export control, account/year selection with label exclusions,
  and a final preview of 20 distinct messages. All mail is synthetic `--demo`
  data. Both native preview confirmation steps are shown; no capture is started.
- **Tjikup — One command. A clear outcome.** An actual successful workflow runs
  six app test suites in parallel, then creates and verifies a package of their
  reports. The final rich report shows all six current app icons, per-app test
  counts and timings, the verified-package result, and the success summary.
  This take records 198 passing checks. The native Kitty renderer is copied
  from `scripts/fleet_update` in the theme source checkout; it renders current
  primary SVGs as 32px Unicode-placeholder sprites inside the terminal. The workflow
  uses the production runner and Progress API. Intentional failure cases inside
  tests cannot write into the outer workflow report. The busy segment alone is
  encoded at 3×; typing, captions and the success summary retain normal timing.
  Raw segment boundaries and final chapter times are in the JSON receipt. The
  verified report package, workflow, check script, command results and hashes
  remain in the final review folder.
- **Greetjik — Start every session with a quip.** Actual Rust output in Kitty
  shows a piped quip and Twilight, then selected Rainbow and Pinkie characters
  with new messages. It is a small bit of terminal fun. Complete quotes and
  artwork fit inside the frame without clipping.

Capture contracts can be checked without a desktop with
`python3 -B -m unittest discover -s scripts/tests -v`. Always review the actual
encoded motion and decode each master before uploading. Only the workflow film
uses a speed edit; all application rendering and input are native.
