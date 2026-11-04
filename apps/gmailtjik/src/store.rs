use crate::model::{Draft, Message, Page};
use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Serialize, de::DeserializeOwned};
use std::{
    fs,
    io::Write,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

pub fn private_dir(path: &Path) -> Result<()> {
    // Resolve existing ancestors before creating anything: never put mail inside a checkout.
    let existing = path
        .ancestors()
        .find(|p| p.exists())
        .context("No existing data parent")?;
    let resolved = existing.canonicalize()?;
    if in_repository(&resolved) {
        bail!("Private Gmail data must be outside a Git repository");
    }
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true).mode(0o700).create(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

pub fn in_repository(path: &Path) -> bool {
    path.ancestors()
        .any(|p| p.join(".git").is_file() || p.join(".git/HEAD").is_file())
}

pub fn data_dir() -> Result<PathBuf> {
    let path = if let Some(path) = std::env::var_os("GMAILTJIK_DATA_DIR") {
        PathBuf::from(path)
    } else if let Some(path) = std::env::var_os("XDG_DATA_HOME") {
        PathBuf::from(path).join("gmailtjik")
    } else {
        PathBuf::from(std::env::var_os("HOME").context("HOME is not set")?)
            .join(".local/share/gmailtjik")
    };
    if !path.is_absolute() {
        bail!("Gmail data directory must be absolute");
    }
    private_dir(&path)?;
    Ok(path)
}

pub fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().context("Missing parent")?;
    private_dir(parent)?;
    let temp = parent.join(format!(".write-{:016x}", rand::random::<u64>()));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temp, path)?;
        fs::File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

pub struct Store {
    pub(crate) db: Connection,
}

impl Store {
    pub fn open(dir: Option<&Path>) -> Result<Self> {
        let db = if let Some(dir) = dir {
            private_dir(dir)?;
            let path = dir.join("mail.sqlite");
            // Only SQLite may open/close database descriptors. Closing an unrelated
            // descriptor can release this process's POSIX locks on other connections.
            // The parent is private (0700), including during initial creation.
            let db = Connection::open(&path)?;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
            db
        } else {
            Connection::open_in_memory()?
        };
        db.execute_batch(
            "PRAGMA busy_timeout=5000; PRAGMA journal_mode=WAL;
            CREATE TABLE IF NOT EXISTS kv(key TEXT PRIMARY KEY, value TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS messages(id TEXT PRIMARY KEY, value TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS searches(key TEXT PRIMARY KEY, value TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS partial_searches(key TEXT PRIMARY KEY, value TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS deleted_messages(id TEXT PRIMARY KEY);",
        )?;
        // An already-running sync process can finish an older read after a
        // permanent deletion. Enforce tombstones inside SQLite so those writers
        // cannot restore deleted content, even before their binary is reopened.
        for operation in ["INSERT", "UPDATE"] {
            db.execute_batch(&format!(
                "CREATE TRIGGER IF NOT EXISTS messages_deleted_{operation}
                 BEFORE {operation} ON messages
                 WHEN EXISTS(SELECT 1 FROM deleted_messages WHERE id=NEW.id)
                 BEGIN SELECT RAISE(IGNORE); END;"
            ))?;
            for table in ["searches", "partial_searches"] {
                db.execute_batch(&format!(
                    "CREATE TRIGGER IF NOT EXISTS {table}_deleted_{operation}
                     AFTER {operation} ON {table}
                     WHEN EXISTS(
                         SELECT 1 FROM json_each(NEW.value,'$.messages') AS member
                         JOIN deleted_messages AS deleted
                         ON deleted.id=json_extract(member.value,'$.id')
                     ) BEGIN
                         UPDATE {table} SET value=json_set(NEW.value,'$.messages',json((
                             SELECT json_group_array(json(member.value))
                             FROM json_each(NEW.value,'$.messages') AS member
                             WHERE NOT EXISTS(SELECT 1 FROM deleted_messages AS deleted
                                 WHERE deleted.id=json_extract(member.value,'$.id'))
                         )),'$.next','') WHERE key=NEW.key;
                     END;"
                ))?;
            }
            db.execute_batch(&format!(
                "CREATE TRIGGER IF NOT EXISTS threads_deleted_{operation}
                 AFTER {operation} ON kv
                 WHEN EXISTS(
                     SELECT 1 FROM json_each(CASE WHEN NEW.key LIKE 'thread:%'
                         THEN NEW.value ELSE '[]' END) AS member
                     JOIN deleted_messages AS deleted
                     ON deleted.id=json_extract(member.value,'$.id')
                 ) BEGIN
                     UPDATE kv SET value=(
                         SELECT json_group_array(json(member.value))
                         FROM json_each(NEW.value) AS member
                         WHERE NOT EXISTS(SELECT 1 FROM deleted_messages AS deleted
                             WHERE deleted.id=json_extract(member.value,'$.id'))
                     ) WHERE key=NEW.key;
                 END;"
            ))?;
        }
        let store = Self { db };
        store.init_actions()?;
        store.init_cache()?;
        Ok(store)
    }
    pub fn get<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        let json: Option<String> = self
            .db
            .query_row("SELECT value FROM kv WHERE key=?", [key], |r| r.get(0))
            .optional()?;
        json.map(|s| serde_json::from_str(&s).map_err(Into::into))
            .transpose()
    }
    pub fn put<T: Serialize>(&self, key: &str, value: &T) -> Result<()> {
        self.db.execute(
            "INSERT OR REPLACE INTO kv VALUES(?,?)",
            params![key, serde_json::to_string(value)?],
        )?;
        Ok(())
    }
    pub fn message(&self, id: &str) -> Result<Option<Message>> {
        let json: Option<String> = self
            .db
            .query_row("SELECT value FROM messages WHERE id=?", [id], |r| r.get(0))
            .optional()?;
        let mut message: Option<Message> = json.map(|s| serde_json::from_str(&s)).transpose()?;
        if let Some(message) = &mut message {
            self.normalize_observed(std::slice::from_mut(message))?;
        }
        Ok(message)
    }
    pub fn save_message(&self, message: &Message) -> Result<()> {
        self.db.execute(
            "INSERT OR REPLACE INTO messages VALUES(?,?)",
            params![message.id, serde_json::to_string(message)?],
        )?;
        Ok(())
    }
    /// A complete Gmail thread response replaces cached membership, not just bodies.
    /// Missing members are evicted locally, without claiming permanent deletion.
    pub fn reconcile_thread(&self, id: &str, messages: &[Message]) -> Result<()> {
        anyhow::ensure!(
            messages.iter().all(|m| m.thread_id == id),
            "Mismatched thread response"
        );
        let ids = serde_json::to_string(&messages.iter().map(|m| &m.id).collect::<Vec<_>>())?;
        let transaction = self.db.unchecked_transaction()?;
        for table in ["searches", "partial_searches"] {
            transaction.execute(&format!(
                "UPDATE {table} SET value=json_set(value,'$.messages',json((
                    SELECT json_group_array(json(member.value)) FROM json_each({table}.value,'$.messages') AS member
                    WHERE json_extract(member.value,'$.threadId') IS NOT ?1
                       OR json_extract(member.value,'$.id') IN (SELECT value FROM json_each(?2))
                ))) WHERE EXISTS(SELECT 1 FROM json_each({table}.value,'$.messages') AS member
                    WHERE json_extract(member.value,'$.threadId')=?1)"
            ), params![id, ids])?;
        }
        transaction.execute("DELETE FROM messages WHERE json_extract(value,'$.threadId')=?1 AND id NOT IN (SELECT value FROM json_each(?2))", params![id, ids])?;
        transaction.execute("DELETE FROM mail_summaries WHERE thread_key=?1 AND id NOT IN (SELECT value FROM json_each(?2))", params![id, ids])?;
        self.put(&format!("thread:{id}"), &messages)?;
        transaction.commit()?;
        Ok(())
    }

    pub fn invalidate(&self, id: &str) -> Result<()> {
        self.invalidate_cached_message(id)?;
        self.db.execute("DELETE FROM messages WHERE id=?", [id])?;
        Ok(())
    }
    pub fn invalidate_all(&self) -> Result<()> {
        self.db.execute("DELETE FROM messages", [])?;
        self.invalidate_page_freshness()?;
        Ok(())
    }
    #[cfg(test)]
    pub fn invalidate_mail_views(&self) -> Result<()> {
        self.db.execute_batch(
            "DELETE FROM messages; DELETE FROM searches; DELETE FROM partial_searches;
             DELETE FROM kv WHERE key LIKE 'thread:%'; DELETE FROM mail_summaries;",
        )?;
        self.invalidate_page_freshness()?;
        Ok(())
    }
    /// Forget only Gmail-confirmed permanent deletions, atomically across every
    /// cache source. One prepared insert per ID avoids SQLite variable limits.
    pub fn remove_messages(&self, ids: &[String]) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let transaction = self.db.unchecked_transaction()?;
        {
            let mut insert =
                transaction.prepare("INSERT OR IGNORE INTO deleted_messages(id) VALUES(?)")?;
            for id in ids {
                insert.execute([id])?;
            }
        }
        transaction.execute(
            "DELETE FROM messages WHERE id IN (SELECT id FROM deleted_messages)",
            [],
        )?;
        for table in ["searches", "partial_searches"] {
            // Deletion can shift any Gmail search's pagination. Keep useful
            // snapshots but require a fresh response before following a cursor.
            transaction.execute(
                &format!(
                    "UPDATE {table} SET value=json_set(value,'$.messages',json((
                         SELECT json_group_array(json(member.value))
                         FROM json_each({table}.value,'$.messages') AS member
                         WHERE NOT EXISTS(SELECT 1 FROM deleted_messages AS deleted
                             WHERE deleted.id=json_extract(member.value,'$.id'))
                     )),'$.next','')"
                ),
                [],
            )?;
        }
        transaction.execute(
            "UPDATE kv SET value=(
                 SELECT json_group_array(json(member.value)) FROM json_each(kv.value) AS member
                 WHERE NOT EXISTS(SELECT 1 FROM deleted_messages AS deleted
                     WHERE deleted.id=json_extract(member.value,'$.id'))
             ) WHERE key LIKE 'thread:%'",
            [],
        )?;
        transaction.execute(
            "DELETE FROM kv WHERE key LIKE 'thread:%' AND value='[]'",
            [],
        )?;
        transaction.commit()?;
        Ok(())
    }
    pub fn apply_labels(&self, id: &str, add: &[String], remove: &[String]) -> Result<()> {
        self.update_summary_labels(id, add, remove)?;
        let mut stmt = self.db.prepare(
            "SELECT 'searches',key,value FROM searches
             UNION ALL SELECT 'partial_searches',key,value FROM partial_searches",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        for (table, key, value) in rows {
            let (label, query, _): (String, String, String) = serde_json::from_str(&key)?;
            if !query.is_empty() && query != "is:unread" {
                self.db
                    .execute(&format!("DELETE FROM {table} WHERE key=?"), [key])?;
                continue;
            }
            let mut page: Page = serde_json::from_str(&value)?;
            for message in &mut page.messages {
                if message.id == id {
                    message.label_ids.retain(|l| !remove.contains(l));
                    for label in add {
                        if !message.label_ids.contains(label) {
                            message.label_ids.push(label.clone());
                        }
                    }
                }
            }
            page.retain_conversations(|m| {
                (label.is_empty() || m.label_ids.contains(&label))
                    && (label == "TRASH" || !m.label_ids.iter().any(|l| l == "TRASH"))
                    && (query != "is:unread" || m.unread())
            });
            self.db.execute(
                &format!("UPDATE {table} SET value=? WHERE key=?"),
                params![serde_json::to_string(&page)?, key],
            )?;
        }
        // A thread cache must not resurrect pre-action read/label state in the reader.
        self.db
            .execute("DELETE FROM kv WHERE key LIKE 'thread:%'", [])?;
        self.invalidate(id)
    }
    #[cfg(test)]
    pub fn page(&self, key: &str) -> Result<Option<Page>> {
        let json: Option<String> = self
            .db
            .query_row("SELECT value FROM searches WHERE key=?", [key], |r| {
                r.get(0)
            })
            .optional()?;
        let page: Option<Page> = json.map(|s| serde_json::from_str(&s)).transpose()?;
        // Old message-page cursors are not interchangeable with thread-page cursors.
        Ok(page.filter(|page| page.threaded))
    }
    pub fn save_page(&self, key: &str, page: &Page) -> Result<()> {
        self.db.execute(
            "INSERT OR REPLACE INTO searches VALUES(?,?)",
            params![key, serde_json::to_string(page)?],
        )?;
        self.db
            .execute("DELETE FROM partial_searches WHERE key=?", [key])?;
        self.mark_page_fresh(key)?;
        Ok(())
    }

    /// Preserve progress without replacing a completed view or inventing a cursor.
    pub fn save_partial_page(&self, key: &str, page: &Page) -> Result<()> {
        if !page.threaded || page.messages.is_empty() {
            return Ok(());
        }
        if let Some(previous) = self.partial_page(key)?
            && previous.conversations().len() > page.conversations().len()
        {
            return Ok(());
        }
        let mut partial = page.clone();
        partial.next.clear();
        for message in &mut partial.messages {
            message.summary_only = true;
            message.payload.body = Default::default();
            message.payload.parts.clear();
        }
        self.db.execute(
            "INSERT INTO partial_searches(key,value)
             SELECT ?,? WHERE NOT EXISTS(
                 SELECT 1 FROM searches WHERE key=? AND json_extract(value,'$.threaded')=1
             )
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![key, serde_json::to_string(&partial)?, key],
        )?;
        Ok(())
    }

    fn partial_page(&self, key: &str) -> Result<Option<Page>> {
        let json: Option<String> = self
            .db
            .query_row(
                "SELECT value FROM partial_searches WHERE key=?",
                [key],
                |row| row.get(0),
            )
            .optional()?;
        let page: Option<Page> = json.map(|json| serde_json::from_str(&json)).transpose()?;
        Ok(page.filter(|page| page.threaded))
    }

    /// Exact snapshots win, including confirmed empty pages. A folder's first
    /// page can fall back to known local membership; arbitrary Gmail searches
    /// and remote pagination cannot be reconstructed safely offline.
    pub fn cached_page(&self, label: &str, query: &str, token: &str) -> Result<Option<Page>> {
        let base = self.cached_folder_page(label, query, token)?;
        let mut page = base.clone().unwrap_or_default();
        self.project_page(label, query, token, &mut page)?;
        if !label.is_empty()
            && token.is_empty()
            && matches!(query.trim(), "" | "is:unread")
            && base.as_ref().is_some_and(|base| {
                !base.messages.is_empty() && page.conversations().len() < base.conversations().len()
            })
            && page.conversations().len() < 25
        {
            // Pending archives may exhaust the saved first page while older
            // cached conversations remain. Fill locally so rapid triage keeps
            // moving even before Gmail acknowledges those actions.
            let mut candidates =
                self.indexed_folder_candidates(label, query, self.optimistic_candidate_limit()?)?;
            self.project_page(label, query, token, &mut candidates)?;
            if candidates.conversations().len() > page.conversations().len() {
                candidates.next.clear();
                page = candidates;
            }
        }
        Ok(if base.is_some() || !page.messages.is_empty() {
            Some(page)
        } else {
            None
        })
    }

    pub fn rate_limit_until(&self) -> Result<u64> {
        Ok(self.get("rate_limit_until")?.unwrap_or(0))
    }

    pub fn defer_requests(&self, until: u64) -> Result<()> {
        self.db.execute(
            "INSERT INTO kv(key,value) VALUES('rate_limit_until',?1)
             ON CONFLICT(key) DO UPDATE SET value=CAST(MAX(
                 CAST(kv.value AS INTEGER), CAST(excluded.value AS INTEGER)
             ) AS TEXT)",
            [until.to_string()],
        )?;
        Ok(())
    }
    pub fn switch_draft(
        &self,
        current: &Draft,
        pending_save: Option<i64>,
        target: Draft,
        resume: Option<usize>,
    ) -> Result<crate::model::DraftSlot> {
        let transaction = self.db.unchecked_transaction()?;
        let mut slots: Vec<crate::model::DraftSlot> =
            self.get("saved_local_drafts")?.unwrap_or_default();
        let index = resume.or_else(|| {
            slots.iter().position(|slot| {
                !target.in_reply_to.is_empty()
                    && slot.draft.in_reply_to == target.in_reply_to
                    && slot.draft.thread_id == target.thread_id
            })
        });
        let selected = if let Some(index) = index {
            anyhow::ensure!(
                index < slots.len(),
                "Saved draft changed; reopen the draft list"
            );
            if resume.is_some() {
                anyhow::ensure!(
                    slots[index].draft == target,
                    "Saved draft changed; reopen the draft list"
                );
            }
            slots.remove(index)
        } else {
            crate::model::DraftSlot {
                draft: target,
                pending_save: None,
            }
        };
        if current != &Draft::default() && current != &selected.draft {
            slots.push(crate::model::DraftSlot {
                draft: current.clone(),
                pending_save,
            });
        }
        self.put("saved_local_drafts", &slots)?;
        self.put("composer", &selected.draft)?;
        self.put("pending_draft_save", &selected.pending_save.unwrap_or(0))?;
        transaction.commit()?;
        Ok(selected)
    }

    pub fn draft(&self) -> Result<Draft> {
        Ok(self.get("composer")?.unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complete_thread_evicts_obsolete_members_from_all_cached_views() {
        let dir = tempfile::tempdir_in("/tmp").unwrap();
        let store = Store::open(Some(dir.path())).unwrap();
        let page = crate::model::demo_conversations().1;
        let key = crate::worker::key("INBOX", "", "");
        store.save_page(&key, &page).unwrap();
        store.save_partial_page(&key, &page).unwrap();
        for message in &page.messages {
            store.save_message(message).unwrap();
        }
        let thread = &page.messages[0].thread_id;
        let members: Vec<_> = page
            .messages
            .iter()
            .filter(|m| &m.thread_id == thread)
            .cloned()
            .collect();
        assert!(members.len() > 1);
        let removed = &members[1].id;
        store.reconcile_thread(thread, &members[..1]).unwrap();
        assert!(store.message(removed).unwrap().is_none());
        for cached in [
            store.page(&key).unwrap().unwrap(),
            store.cached_folder_page("INBOX", "", "").unwrap().unwrap(),
        ] {
            assert!(!cached.messages.iter().any(|m| &m.id == removed));
            assert_eq!(
                cached
                    .messages
                    .iter()
                    .filter(|m| &m.thread_id == thread)
                    .count(),
                1
            );
        }
        assert_eq!(
            store
                .db
                .query_row("SELECT count(*) FROM deleted_messages", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
        drop(store);
        let store = Store::open(Some(dir.path())).unwrap();
        assert!(
            !store
                .cached_folder_page("INBOX", "", "")
                .unwrap()
                .unwrap()
                .messages
                .iter()
                .any(|m| &m.id == removed)
        );
    }

    #[test]
    fn switching_replies_preserves_drafts_and_pending_receipts_across_reopen() {
        let dir = tempfile::tempdir_in("/tmp").unwrap();
        let first = Draft {
            body: "Unfinished original".into(),
            ..Default::default()
        };
        let reply = Draft {
            in_reply_to: "message-id".into(),
            thread_id: "thread".into(),
            body: "Reply text".into(),
            ..Default::default()
        };
        {
            let store = Store::open(Some(dir.path())).unwrap();
            assert_eq!(
                store
                    .switch_draft(&first, Some(42), reply.clone(), None)
                    .unwrap()
                    .draft,
                reply
            );
        }
        let store = Store::open(Some(dir.path())).unwrap();
        assert_eq!(store.draft().unwrap(), reply);
        let restored = store
            .switch_draft(&reply, Some(43), first.clone(), Some(0))
            .unwrap();
        assert_eq!(restored.draft, first);
        assert_eq!(restored.pending_save, Some(42));
        assert!(
            store
                .switch_draft(&first, Some(42), Draft::default(), Some(0))
                .is_err()
        );
        assert_eq!(store.draft().unwrap(), first);
        let restored = store
            .switch_draft(&first, Some(42), reply.clone(), None)
            .unwrap();
        assert_eq!(restored.draft, reply);
        assert_eq!(restored.pending_save, Some(43));
    }

    #[test]
    fn permanent_deletion_scrubs_every_cache_and_preserves_other_thread_members() {
        let dir = tempfile::tempdir_in("/tmp").unwrap();
        let mut page = crate::model::demo_conversations().1;
        page.next = "old-page-token".into();
        let deleted = vec!["demo-0".into(), "demo-2".into()];
        let full_key = crate::worker::key("INBOX", "", "");
        let partial_key = crate::worker::key("INBOX", "is:unread", "");
        let search_key = crate::worker::key("", "synthetic", "old-page-token");
        {
            let store = Store::open(Some(dir.path())).unwrap();
            for message in &page.messages {
                store.save_message(message).unwrap();
            }
            store.save_page(&full_key, &page).unwrap();
            store.save_page(&search_key, &page).unwrap();
            store.save_partial_page(&partial_key, &page).unwrap();
            for thread in page.conversations() {
                store
                    .put(
                        &format!("thread:{}", thread.message().thread_id),
                        &thread.messages,
                    )
                    .unwrap();
            }
            store.put("email", &"synthetic@example.test").unwrap();
            store.remove_messages(&deleted).unwrap();
        }
        let store = Store::open(Some(dir.path())).unwrap();
        for id in &deleted {
            assert!(store.message(id).unwrap().is_none());
        }
        assert!(store.message("demo-reply-0").unwrap().is_some());
        for cached in [
            store.page(&full_key).unwrap().unwrap(),
            store.page(&search_key).unwrap().unwrap(),
            store.partial_page(&partial_key).unwrap().unwrap(),
            store.cached_page("design", "", "").unwrap().unwrap(),
        ] {
            assert!(cached.next.is_empty());
            assert!(
                cached
                    .messages
                    .iter()
                    .all(|message| !deleted.contains(&message.id))
            );
            assert!(
                cached
                    .messages
                    .iter()
                    .any(|message| message.id == "demo-reply-0")
            );
        }
        let thread: Vec<Message> = store.get("thread:thread-0").unwrap().unwrap();
        assert_eq!(thread.len(), 2);
        assert!(thread.iter().all(|message| message.id != "demo-0"));
        assert!(
            store
                .get::<Vec<Message>>("thread:thread-2")
                .unwrap()
                .is_none()
        );
        assert_eq!(
            store.get::<String>("email").unwrap().unwrap(),
            "synthetic@example.test"
        );
    }

    #[test]
    fn old_sync_writers_cannot_resurrect_permanently_deleted_mail() {
        let dir = tempfile::tempdir_in("/tmp").unwrap();
        let store = Store::open(Some(dir.path())).unwrap();
        let old_writer = Store::open(Some(dir.path())).unwrap();
        let mut page = crate::model::demo().1;
        page.next = "stale-next".into();
        let deleted = &page.messages[0];
        let key = crate::worker::key("INBOX", "", "");
        store.remove_messages(&[deleted.id.clone()]).unwrap();
        // Raw SQL models the existing process's pre-tombstone cache methods.
        for _ in 0..2 {
            old_writer
                .db
                .execute(
                    "INSERT OR REPLACE INTO messages VALUES(?,?)",
                    params![deleted.id, serde_json::to_string(deleted).unwrap()],
                )
                .unwrap();
            for table in ["searches", "partial_searches"] {
                old_writer
                    .db
                    .execute(
                        &format!("INSERT OR REPLACE INTO {table} VALUES(?,?)"),
                        params![key, serde_json::to_string(&page).unwrap()],
                    )
                    .unwrap();
                old_writer
                    .db
                    .execute(
                        &format!("UPDATE {table} SET value=? WHERE key=?"),
                        params![serde_json::to_string(&page).unwrap(), key],
                    )
                    .unwrap();
            }
            old_writer.put("thread:stale", &page.messages).unwrap();
            old_writer
                .db
                .execute(
                    "UPDATE kv SET value=? WHERE key='thread:stale'",
                    [serde_json::to_string(&page.messages).unwrap()],
                )
                .unwrap();
        }
        assert!(store.message(&deleted.id).unwrap().is_none());
        for cached in [
            store.page(&key).unwrap().unwrap(),
            store.partial_page(&key).unwrap().unwrap(),
            store.cached_page("design", "", "").unwrap().unwrap(),
        ] {
            assert!(cached.next.is_empty());
            assert!(
                cached
                    .messages
                    .iter()
                    .all(|message| message.id != deleted.id)
            );
        }
        let thread: Vec<Message> = store.get("thread:stale").unwrap().unwrap();
        assert_eq!(thread.len(), page.messages.len() - 1);
        assert!(thread.iter().all(|message| message.id != deleted.id));
    }

    #[test]
    fn permanent_deletion_is_atomic_and_empty_input_keeps_pagination() {
        let store = Store::open(None).unwrap();
        let mut page = crate::model::demo().1;
        page.next = "valid-next".into();
        let key = crate::worker::key("INBOX", "", "");
        let message = &page.messages[0];
        store.save_message(message).unwrap();
        store.save_page(&key, &page).unwrap();
        store.remove_messages(&[]).unwrap();
        assert_eq!(store.page(&key).unwrap().unwrap().next, "valid-next");
        store
            .db
            .execute_batch(
                "CREATE TRIGGER synthetic_failure BEFORE DELETE ON messages
             BEGIN SELECT RAISE(ABORT,'synthetic cache failure'); END;",
            )
            .unwrap();
        assert!(store.remove_messages(&[message.id.clone()]).is_err());
        store
            .db
            .execute_batch("DROP TRIGGER synthetic_failure")
            .unwrap();
        assert!(store.message(&message.id).unwrap().is_some());
        assert_eq!(
            store.page(&key).unwrap().unwrap().messages.len(),
            page.messages.len()
        );
        assert_eq!(
            store
                .db
                .query_row("SELECT COUNT(*) FROM deleted_messages", [], |r| r
                    .get::<_, u64>(0))
                .unwrap(),
            0
        );
        let mut ids: Vec<String> = (0..1200).map(|id| format!("gone-{id}")).collect();
        ids.push(message.id.clone());
        store.remove_messages(&ids).unwrap();
        assert!(store.message(&message.id).unwrap().is_none());
        assert!(store.page(&key).unwrap().unwrap().next.is_empty());
    }

    #[test]
    fn emptied_partial_view_can_use_other_known_folder_messages() {
        let store = Store::open(None).unwrap();
        let mut partial = crate::model::demo().1;
        let other = partial.messages[1].clone();
        store.save_message(&other).unwrap();
        partial.messages.truncate(1);
        let removed = partial.messages[0].id.clone();
        store
            .save_partial_page(&crate::worker::key("INBOX", "", ""), &partial)
            .unwrap();
        store
            .apply_labels(&removed, &[], &["INBOX".into()])
            .unwrap();
        let cached = store.cached_page("INBOX", "", "").unwrap().unwrap();
        assert_eq!(cached.messages.len(), 1);
        assert_eq!(cached.messages[0].id, other.id);
    }

    #[test]
    fn partial_views_survive_restart_without_replacing_complete_pages() {
        let dir = tempfile::tempdir_in("/tmp").unwrap();
        let key = crate::worker::key("INBOX", "", "");
        let mut page = crate::model::demo().1;
        page.next = "valid-next-page".into();
        {
            let store = Store::open(Some(dir.path())).unwrap();
            store.save_partial_page(&key, &page).unwrap();
            let mut fewer = page.clone();
            fewer.messages.truncate(1);
            store.save_partial_page(&key, &fewer).unwrap();
        }
        let store = Store::open(Some(dir.path())).unwrap();
        assert!(store.page(&key).unwrap().is_none());
        let cached = store.cached_page("INBOX", "", "").unwrap().unwrap();
        assert_eq!(cached.messages.len(), 5);
        assert!(cached.next.is_empty());
        assert!(cached.messages.iter().all(|message| {
            message.summary_only
                && message.payload.body.data.is_empty()
                && message.payload.parts.is_empty()
        }));
        let mut complete = page.clone();
        complete.messages.truncate(2);
        store.save_page(&key, &complete).unwrap();
        store.save_partial_page(&key, &page).unwrap();
        let cached = store.cached_page("INBOX", "", "").unwrap().unwrap();
        assert_eq!(cached.messages.len(), 2);
        assert_eq!(cached.next, "valid-next-page");
        let empty = Page {
            threaded: true,
            ..Default::default()
        };
        store.save_page(&key, &empty).unwrap();
        store.save_message(&page.messages[0]).unwrap();
        assert!(
            store
                .cached_page("INBOX", "", "")
                .unwrap()
                .unwrap()
                .messages
                .is_empty()
        );
    }

    #[test]
    fn legacy_exact_views_keep_messages_but_discard_message_cursors() {
        let store = Store::open(None).unwrap();
        let key = crate::worker::key("", "from:synthetic@example.test", "");
        let mut legacy = crate::model::demo_conversations().1;
        legacy.threaded = false;
        legacy.next = "obsolete-message-page-token".into();
        store.save_page(&key, &legacy).unwrap();
        assert!(store.page(&key).unwrap().is_none());
        let cached = store
            .cached_page("", "from:synthetic@example.test", "")
            .unwrap()
            .unwrap();
        assert!(cached.threaded && cached.next.is_empty());
        assert_eq!(cached.messages.len(), legacy.messages.len());
        assert_eq!(cached.conversations().len(), 3);
        let mut smaller = legacy.clone();
        smaller.threaded = true;
        smaller.messages.truncate(1);
        store.save_partial_page(&key, &smaller).unwrap();
        assert_eq!(
            store
                .cached_page("", "from:synthetic@example.test", "")
                .unwrap()
                .unwrap()
                .conversations()
                .len(),
            3
        );
    }

    #[test]
    fn folder_fallback_is_bounded_sorted_and_never_guesses_search_or_pagination() {
        let store = Store::open(None).unwrap();
        let source = crate::model::demo().1.messages.remove(0);
        for index in 0..32 {
            let mut message = source.clone();
            message.id = format!("message-{index}");
            message.thread_id = format!("thread-{index}");
            message.internal_date = index.to_string();
            message.label_ids = vec!["target".into()];
            if index % 2 == 0 {
                message.label_ids.push("UNREAD".into());
            }
            if index == 30 {
                message.label_ids.push("TRASH".into());
            }
            if index == 31 {
                message.label_ids.push("SPAM".into());
            }
            if index == 29 {
                store.put("thread:thread-29", &vec![message]).unwrap();
            } else if index == 28 {
                store
                    .save_page(
                        &crate::worker::key("other", "", ""),
                        &Page {
                            messages: vec![message],
                            threaded: true,
                            next: "other-cursor".into(),
                        },
                    )
                    .unwrap();
            } else {
                store.save_message(&message).unwrap();
            }
        }
        let cached = store.cached_page("target", "", "").unwrap().unwrap();
        assert_eq!(cached.conversations().len(), 25);
        assert_eq!(cached.messages[0].id, "message-29");
        assert_eq!(cached.messages.last().unwrap().id, "message-5");
        assert!(cached.next.is_empty());
        assert!(
            cached
                .messages
                .iter()
                .all(|message| message.summary_only && message.payload.body.data.is_empty())
        );
        assert_eq!(
            store
                .cached_page("target", "is:unread", "")
                .unwrap()
                .unwrap()
                .conversations()
                .len(),
            15
        );
        assert!(
            store
                .cached_page("target", "from:synthetic@example.test", "")
                .unwrap()
                .is_none()
        );
        assert!(
            store
                .cached_page("target", "", "remote-next")
                .unwrap()
                .is_none()
        );
        assert!(store.cached_page("", "", "").unwrap().is_none());
    }

    #[test]
    fn fallback_keeps_known_sent_members_and_partial_views_follow_actions() {
        let store = Store::open(None).unwrap();
        let page = crate::model::demo_conversations().1;
        for message in &page.messages {
            store.save_message(message).unwrap();
        }
        let cached = store.cached_page("INBOX", "", "").unwrap().unwrap();
        assert!(
            cached
                .conversations()
                .iter()
                .any(|thread| thread.messages.len() == 3)
        );
        let key = crate::worker::key("INBOX", "is:unread", "");
        store.save_partial_page(&key, &page).unwrap();
        for message in &page.messages {
            store
                .apply_labels(&message.id, &[], &["UNREAD".into()])
                .unwrap();
        }
        assert!(
            store
                .cached_page("INBOX", "is:unread", "")
                .unwrap()
                .unwrap()
                .messages
                .is_empty()
        );
        let key = crate::worker::key("INBOX", "", "");
        store.save_partial_page(&key, &page).unwrap();
        for id in ["demo-0", "demo-received-reply", "demo-reply-0"] {
            store.apply_labels(id, &[], &["INBOX".into()]).unwrap();
        }
        let cached = store.cached_page("INBOX", "", "").unwrap().unwrap();
        assert!(
            cached
                .messages
                .iter()
                .all(|message| message.thread_id != "thread-0")
        );
        store.invalidate_mail_views().unwrap();
        assert!(store.cached_page("INBOX", "", "").unwrap().is_none());
    }

    #[test]
    fn rate_limit_deadline_is_shared_and_can_only_move_forward() {
        let dir = tempfile::tempdir_in("/tmp").unwrap();
        let first = Store::open(Some(dir.path())).unwrap();
        let second = Store::open(Some(dir.path())).unwrap();
        assert_eq!(first.rate_limit_until().unwrap(), 0);
        first.defer_requests(100).unwrap();
        second.defer_requests(200).unwrap();
        first.defer_requests(150).unwrap();
        assert_eq!(first.rate_limit_until().unwrap(), 200);
        assert_eq!(second.rate_limit_until().unwrap(), 200);
    }

    #[test]
    fn database_lock_probe() {
        let Some(path) = std::env::var_os("GMAILTJIK_TEST_LOCK_DB") else {
            return;
        };
        let db = Connection::open(path).unwrap();
        db.busy_timeout(std::time::Duration::ZERO).unwrap();
        let error = db
            .execute_batch("BEGIN EXCLUSIVE")
            .expect_err("another process must still hold its read lock");
        assert_eq!(
            error.sqlite_error_code(),
            Some(rusqlite::ErrorCode::DatabaseBusy)
        );
    }

    #[test]
    fn opening_another_store_does_not_release_existing_sqlite_locks() {
        let dir = tempfile::tempdir_in("/tmp").unwrap();
        let first = Store::open(Some(dir.path())).unwrap();
        first
            .db
            .execute_batch("PRAGMA journal_mode=DELETE; BEGIN; SELECT * FROM kv;")
            .unwrap();
        // Journal-mode switching is expected to fail while the first connection
        // reads. Even this failure path must retain its OS-level lock.
        let _second = Store::open(Some(dir.path()));
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "store::tests::database_lock_probe",
                "--nocapture",
            ])
            .env("GMAILTJIK_TEST_LOCK_DB", dir.path().join("mail.sqlite"))
            .status()
            .unwrap();
        assert!(status.success());
        first.db.execute_batch("ROLLBACK").unwrap();
    }

    #[test]
    fn conversation_cache_keeps_sent_members_until_the_thread_leaves_inbox() {
        let store = Store::open(None).unwrap();
        let page = crate::model::demo_conversations().1;
        let key = crate::worker::key("INBOX", "", "");
        store.save_page(&key, &page).unwrap();
        store
            .apply_labels("demo-0", &[], &["UNREAD".into()])
            .unwrap();
        assert_eq!(
            store.page(&key).unwrap().unwrap().conversations()[0]
                .messages
                .len(),
            3
        );
        for id in ["demo-0", "demo-received-reply", "demo-reply-0"] {
            store.apply_labels(id, &[], &["INBOX".into()]).unwrap();
        }
        let cached = store.page(&key).unwrap().unwrap();
        assert_eq!(cached.conversations().len(), 2);
        assert!(cached.messages.iter().all(|m| m.thread_id != "thread-0"));
    }

    #[test]
    fn old_message_pages_are_not_reused_as_thread_pages() {
        let store = Store::open(None).unwrap();
        let mut page = crate::model::demo().1;
        page.threaded = false;
        page.next = "message-cursor".into();
        store.save_page("old", &page).unwrap();
        assert!(store.page("old").unwrap().is_none());
    }

    #[test]
    fn cached_page_and_draft_survive_restart() {
        let dir = tempfile::tempdir_in("/tmp").unwrap();
        let page = crate::model::demo().1;
        let draft = Draft {
            body: "Unsaved thoughts".into(),
            ..Draft::default()
        };
        {
            let store = Store::open(Some(dir.path())).unwrap();
            store.save_page("INBOX", &page).unwrap();
            store.put("composer", &draft).unwrap();
        }
        let store = Store::open(Some(dir.path())).unwrap();
        assert_eq!(store.page("INBOX").unwrap().unwrap().messages.len(), 5);
        assert_eq!(store.draft().unwrap(), draft);
        assert_eq!(
            fs::metadata(dir.path().join("mail.sqlite"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    #[test]
    fn private_storage_rejects_repository() {
        let dir = tempfile::tempdir_in("/tmp").unwrap();
        fs::create_dir(dir.path().join(".git")).unwrap();
        fs::write(dir.path().join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
        assert!(private_dir(&dir.path().join("mail")).is_err());
        assert!(!dir.path().join("mail").exists());
    }
    #[test]
    fn trashed_mail_leaves_cached_views_and_searches() {
        let store = Store::open(None).unwrap();
        let page = crate::model::demo().1;
        for label in ["INBOX", "", "STARRED", "TRASH"] {
            store
                .save_page(&crate::worker::key(label, "", ""), &page)
                .unwrap();
        }
        let search = crate::worker::key("", "synthetic", "");
        store.save_page(&search, &page).unwrap();
        store
            .apply_labels("demo-0", &["TRASH".into()], &["INBOX".into()])
            .unwrap();
        for label in ["INBOX", "", "STARRED"] {
            let page = store
                .page(&crate::worker::key(label, "", ""))
                .unwrap()
                .unwrap();
            assert!(page.messages.iter().all(|message| message.id != "demo-0"));
        }
        assert!(store.page(&search).unwrap().is_none());
        let trash = store
            .page(&crate::worker::key("TRASH", "", ""))
            .unwrap()
            .unwrap();
        assert_eq!(trash.messages.len(), 1);
        assert_eq!(trash.messages[0].id, "demo-0");
    }
    #[test]
    fn archived_mail_does_not_reappear_from_a_cached_inbox() {
        let store = Store::open(None).unwrap();
        let key = crate::worker::key("INBOX", "", "");
        store.save_page(&key, &crate::model::demo().1).unwrap();
        store
            .apply_labels("demo-0", &[], &["INBOX".into()])
            .unwrap();
        assert_eq!(store.page(&key).unwrap().unwrap().messages.len(), 4);
    }
}
