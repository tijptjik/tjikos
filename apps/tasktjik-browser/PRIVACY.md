# Tasktjik privacy

Tasktjik reads the active page's title and URL when you invoke its save shortcut or its
toolbar button. It does not read page contents, cookies, history, passwords or other
tabs. Private browsing is excluded.

Unsaved drafts and uncertain submissions are retained in this extension's local browser
storage, including their URLs, titles, editor fields and random request IDs. They remain
until saved, explicitly discarded, or the extension's data is removed. Tasktjik does not
use browser sync. A confirmed save removes the persistent draft; the running extension
may retain its last result in memory.

The sidebar requests your domain names, project names and template definitions from
Tasktjik to populate the editor. These choices are held in panel memory. The native
helper filters out unrelated tasks before responding to the extension.

When saving, the extension passes the reading fields to its installed native helper. The
helper invokes the local Tasktjik client, which uses your existing configuration,
authentication and authority. The browser extension never receives those credentials.
Saved items follow Tasktjik's normal storage and retention behavior.

Tasktjik has no analytics, advertising or independent cloud service. Removing the
extension does not delete items already saved in Tasktjik. To remove the native helper,
delete `~/.local/lib/readtjik` and
`~/.mozilla/native-messaging-hosts/org.tjikos.readtjik.json`.
