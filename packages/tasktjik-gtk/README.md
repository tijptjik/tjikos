# Tasktjik GTK views

This crate presents Tasktjik task and reading snapshots. The host supplies commands and
owns persistence, workflow execution, preview transport and installation paths. GTK
objects stay on the main thread. This is a domain-specific view package; `tjik-gtk`
contains the general presentation primitives.

`TaskView::new(send, exit, previews)` accepts an optional `Rc<dyn PreviewProvider>`.
Pass `None` for offline or synthetic views. A provider returns a `PreviewFuture` with
`PagePreview` values and must cancel owned work when that future is dropped. `load` runs
on the GTK thread and must return immediately without blocking I/O. Results are local
thumbnail URIs and descriptions, not executable actions or remote image fetches.

The view deduplicates current references, runs one request of at most four URLs at a
time, and discards unsolicited or removed-reference results. Its cache is scoped to the
current snapshot's references. An empty result retains the fallback for that reference;
removing and reintroducing the reference or creating a new view allows another request.
Disposing the view aborts and releases the pending future. Preview completions update
existing cards rather than rebuilding the collection, retaining widget identity and
selection. Pictures use `tjik-gtk`'s bounded asynchronous image loader.

Reading uses a virtualized row model for list/grid layouts and category headings.
Keyboard selection follows the complete logical order, including off-screen records;
only GTK-bound rows retain card widgets. Width changes regroup grid rows without a
permanent frame callback. Task groups retain their shared outer scrollbar and variable
card heights. Each column creates only viewport/overscan cards; a prefix tree retains
measured heights for logarithmic position lookup. Focused and dragged cards remain alive
until released. Keyboard order is independent of realized card widgets.

Run the synthetic provider, disposal, stale-result and picture-loading regression with
`dev-check python3 -B packages/tasktjik-gtk/scripts/test-previews.py` in this
repository. It opens a private display silently on workspace 7. The host adapter and
Python helper have separate process-cancellation and browser-cleanup tests.

Attachment reads, base64 conversion, clipboard PNG encoding and full-image decoding run
off the GTK thread. Each view admits one image job at a time. Navigation or unmap
invalidates completion, and file attachments also retain the original editor identity
and task revision. Regular-file reads stop at five MiB plus one byte; oversized encoded
responses are rejected before base64 decoding. This bounds encoded input, not native
image-decoder intermediate memory. Dropping a view prevents result application; an
already executing bounded worker operation is allowed to finish.

`dev-check python3 -B packages/tasktjik-gtk/scripts/profile-collections.py` measures
update-to-first-paint latency and widget counts for synthetic 100/1,000-record task and
reading collections. It is an explicit rendered profile, not a latency guarantee.

## License

The Tjik-authored source in this package is licensed under MPL-2.0; see `LICENSE` and
`NOTICE`. Standard MPL secondary-license compatibility is retained, allowing use in
GPLv3 applications. Third-party components retain their own licenses and notices.
