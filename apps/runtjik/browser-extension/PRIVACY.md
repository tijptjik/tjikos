# Runtjik Browser Bridge privacy

The extension connects Firefox or Zen to the separately installed Runtjik
application on the same computer. It sends normal-window and unpinned-tab
titles, URLs, favicon references and tab status to Runtjik when requested.
History searches send matching HTTP(S) history titles, URLs, visit counts and
last-visit times. These are declared as browsing activity during installation.

The extension does not send this information to a remote service, collect
telemetry, read page bodies, or persist a copy of your browsing history.
Private windows are excluded. It can activate and close tabs, open history
results, and delete a URL's history only in response to Runtjik commands.

Runtjik holds browser results in memory. It may fetch HTTPS favicon URLs
without browser cookies; those icon servers receive the usual network request.
Runtjik's optional command history can retain search terms you type. Turn off
`[history].enabled` in Runtjik to disable that feature. If you explicitly
configure offline history profiles, the native Runtjik app can also read their
existing history databases without the extension or browser running. It does
not create a history copy or index and never edits those databases.

Disable or remove the extension in the browser's Add-ons Manager to stop its
bridge. Remove Runtjik's `Browser.history_profiles` configuration separately
to disable native offline history access. No account or telemetry is required.
