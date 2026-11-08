# Hyprvoice through Codex

This patch adds an explicit `codex` provider to Hyprvoice v1.0.2 (commit
`2b1c94824585f37df260b522e2c343fc1d58ef93`). Both adapters use
`http://127.0.0.1:2455/v1`, omit API credentials, and fail if that proxy is unavailable.
There is no fallback to the public OpenAI API.

- Transcription: `gpt-4o-transcribe`, batch mode.
- Text cleanup: `gpt-6-luna`.
- Recording explicitly uses `pw-record --raw`. PipeWire otherwise writes an AU container
  to stdout with big-endian samples; Hyprvoice expects raw little-endian PCM and would
  send corrupted audio to transcription.
- The local codex-lb instance must provide subscription-backed transcription and chat
  completions with local proxy authentication disabled, as configured on this machine.
  These calls consume the connected account's Codex allowance.

Run `./scripts/hyprvoice-codex/build.sh` to test and rebuild. The default output is
`~/.local/share/hyprvoice-codex/`. A user systemd drop-in selects that binary; normal
upstream binary updates therefore do not silently undo this routing. Rebuild this patch
deliberately when upgrading its pinned upstream version. The build checks capture
arguments alongside the provider tests. Upstream tests that record the live microphone
are skipped. The errors-only notifier test uses a private `notify-send` stub and never
posts notifications to the desktop.

The private `~/.config/hyprvoice/config.toml` must select `provider = "codex"` for both
`[transcription]` and `[llm]`, with the model names above. LLM cleanup remains enabled;
transcription streaming remains disabled. No API key is required for this provider. Keep
configuration and its backups outside Git.

Set `[notifications] type = "errors"` when Bartjik displays the Hyprvoice tray
indicator. Routine recording, transcription, cleanup, cancellation and reload
notifications stay quiet; aborted operations and actual pipeline errors still appear as
critical desktop notifications. The daemon reloads configuration by stopping its current
pipeline, so apply this setting while Hyprvoice is idle.

To smoke-test without recording a microphone or injecting text, generate a synthetic WAV
with `espeak-ng`, convert it with ffmpeg to 16 kHz mono s16le PCM, and run
`~/.local/share/hyprvoice-codex/codex-smoke /path/to/synthetic.pcm`. The checker loads
the actual private config and refuses non-Codex providers.

The launcher `~/.local/bin/hyprvoice` also points to the patched build so CLI
configuration understands the new provider. The installation backs up the previous
private config and records the original service and binary target under
`~/.local/state/hyprvoice-codex/backups/`. To roll back, restore that config, remove
only `~/.config/systemd/user/hyprvoice.service.d/codex.conf`, run restore the launcher
symlink to the path in `original-target.txt`, run `systemctl --user daemon-reload`, and
restart `hyprvoice.service` while idle. Rollback restores the original API-billed
configuration.
