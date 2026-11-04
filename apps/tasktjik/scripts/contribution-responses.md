# Contribution response sync

`sync_contribution_responses.py` reads the linked Whatsover Rust inbox and writes
`metadata.contributionResponse` as `Accepted` or `Declined` on matching project
contribution tasks. General agreement applies to the contact, without claiming
they promised both a book entry and a video. For combined contribution workflows,
the shared agreement choice is selected too. `Instruction Sent` is completed
when the stored outreach receipt confirms all campaign messages were accepted.
Receipt, review and compilation steps remain unchanged. Existing synced metadata
is projected into the shared workflow once, including after a task merge;
subsequent idle runs preserve manual workflow edits. Unknown replies
leave the field absent, or preserve a previously recorded response.

```sh
python3 apps/tasktjik/scripts/sync_contribution_responses.py \
  --project PROJECT_UUID \
  --state-dir "$HOME/.local/state/tasktjik-contribution-responses"
```

This creates a private `dry-run.json`. Add `--apply` to write revision-checked
updates and advance the checkpoint. Add `--observe-seconds 30` to connect the
existing linked Whatsover account and collect queued messages first. The script
never sends chat messages or initiates pairing. Use `--tasktjik` and `--whatsover`
to select installed executables; `--database` selects the Rust inbox database.

Matching uses task contact IDs, international phone numbers and incoming
WhatsApp sender alternate IDs. Groups, forwarded messages and demo conversations
are excluded. General English commitments/refusals and replies planning what to
contribute (rather than whether to contribute) following a
matching invitation within 14 days are classified automatically. Questions,
conditional answers, medium-specific promises, other languages and uncertain
replies appear in the private report for human review. This intentionally does
not infer agreement from thanks, reactions, attendance or delivery receipts.
It does not transcribe audio, inspect images, or import historical WhatsApp chats.
Coverage is limited to messages already captured by this linked client plus
queued messages collected during observation.

`state.json` keeps the monotonic inbox sequence, boundary hash, and per-contact
last message timestamp/ID and invitation context. Sequence pagination includes
late-arriving messages; timestamps prevent older replies replacing newer
decisions. Dry runs do not advance it. A replaced/reset inbox fails visibly.
`pending.json` retains exact request UUIDs across crashes and lost replies;
checkpoint advancement follows successful writes and read-back verification.
A revision conflict stops the job for review instead of overwriting concurrent
changes. Preserve pending files when investigating failures. `runs/` retains
private evidence and reports, including unclear messages; `last-report.json`
summarizes the latest successful run. All these files stay outside Git, under a
0700 directory with 0600 files. The source inbox is opened read-only.

For the supplied three-hour systemd user timer, install the script as
`~/.local/bin/tasktjik-contribution-responses` (mode 0755), copy the matching
service/timer from `../systemd/` into `~/.config/systemd/user/`, and create a private
`~/.config/tasktjik/contribution-responses.env` containing
`PROJECT_ID=PROJECT_UUID`. Then run `systemctl --user daemon-reload` and
`systemctl --user enable --now tasktjik-contribution-responses.timer`.
It runs at 00:00, 03:00, 06:00, etc. Hong Kong time while the user service manager
is available, with one catch-up run after downtime. It does not wake or power on
the computer. Failures are visible via `systemctl --user status
tasktjik-contribution-responses.service`; private chat text is not printed to the
journal. A shared Whatsover `jobs.lock` and the client's account lock prevent
concurrent session use; a busy account causes a failed run without advancing the
checkpoint. No running receiver or other job is stopped.

Synthetic validation: `python3 -B apps/tasktjik/tests/contribution_responses.py`.

After correcting a classification rule, use `--reconsider-message MESSAGE_ID`
with the same project/state directory to review one previously processed reply.
Add `--apply` to persist the correction. This leaves the inbox sequence unchanged,
preserves newer decisions, and uses the same durable pending-write protocol.
