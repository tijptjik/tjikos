# Shared Tasktjik GTK view

`TaskView` is an embeddable GTK4 task/reading/project/template interface. Runtjik
hosts it and supplies a request callback; another GTK application can host the
same widget without copying the task editor, theme, or mutation handling.

The authority owns recurrence, workflow eligibility, verification, revisions and
storage. This package submits existing `app-action` requests and displays returned
state. Requests are serialized, edits retain their original revision, and an
editor closes only after its save is acknowledged. An acknowledged image attachment
may advance that editor's own revision; unrelated refreshes cannot do so.

Unchanged snapshots preserve task widgets. Refreshes do not rebuild an active
editor or drag gesture. Templates create fresh workflow node IDs and remap their
dependencies. Images are loaded through the authority and never stored here.

The native integration suite lives in `apps/runtjik/tests/gtk_ui.py`. Run it via
`dev-check`; its dedicated nested compositor is placed silently on workspace 7.
