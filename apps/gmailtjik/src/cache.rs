//! Indexed, body-free folder snapshots. SQLite triggers keep this projection
//! current even while an older background sync binary is still running.
use crate::{
    model::{Message, Page},
    store::Store,
};
use anyhow::Result;
use rusqlite::{OptionalExtension, params};

const REVISION: &str = "UPDATE mail_cache_meta SET revision=revision+1 WHERE singleton=1;";

fn summaries(source: &str) -> String {
    // Labels are mutable; a previously started reader (including an older sync
    // binary) must not undo a newer authoritative observation after its action
    // overlay has retired. Message bodies themselves are immutable in Gmail.
    let source = format!(
        "SELECT json_set(input.value,'$.labelIds',json(COALESCE(observed.labels,
           json_extract(input.value,'$.labelIds'),'[]'))) AS value
         FROM ({source}) AS input LEFT JOIN message_observations AS observed
           ON observed.id=json_extract(input.value,'$.id')"
    );
    format!(
        "INSERT INTO mail_summaries(id,thread_key,received,summary,trashed,spam,snoozed,unread)
         SELECT json_extract(source.value,'$.id'),
           COALESCE(NULLIF(json_extract(source.value,'$.threadId'),''),json_extract(source.value,'$.id')),
           COALESCE(CAST(json_extract(source.value,'$.internalDate') AS INTEGER),0),
           json_set(json_remove(source.value,'$.payload.body','$.payload.parts'),'$.summaryOnly',json('true')),
           EXISTS(SELECT 1 FROM json_each(source.value,'$.labelIds') WHERE value='TRASH'),
           EXISTS(SELECT 1 FROM json_each(source.value,'$.labelIds') WHERE value='SPAM'),
           EXISTS(SELECT 1 FROM json_each(source.value,'$.labelIds') WHERE value='SNOOZED'),
           EXISTS(SELECT 1 FROM json_each(source.value,'$.labelIds') WHERE value='UNREAD')
         FROM ({source}) AS source
         WHERE COALESCE(json_extract(source.value,'$.id'),'')!=''
           AND NOT EXISTS(SELECT 1 FROM deleted_messages WHERE id=json_extract(source.value,'$.id'))
         ON CONFLICT(id) DO UPDATE SET thread_key=excluded.thread_key,received=excluded.received,
           summary=excluded.summary,trashed=excluded.trashed,spam=excluded.spam,
           snoozed=excluded.snoozed,unread=excluded.unread;"
    )
}

fn view_snapshot(source: &str, key: &str, value: &str) -> String {
    format!(
        "INSERT INTO mail_views(source,key,next,complete,refreshed_at)
         VALUES('{source}',{key},COALESCE(json_extract({value},'$.next'),''),
           COALESCE(json_extract({value},'$.threaded'),0),0)
         ON CONFLICT(source,key) DO UPDATE SET next=excluded.next,
           complete=excluded.complete,refreshed_at=0;
         DELETE FROM mail_view_messages WHERE source='{source}' AND key={key};
         INSERT OR IGNORE INTO mail_view_messages(source,key,id,position)
         SELECT '{source}',{key},json_extract(member.value,'$.id'),CAST(member.key AS INTEGER)
         FROM json_each({value},'$.messages') AS member
         WHERE COALESCE(json_extract(member.value,'$.id'),'')!=''
           AND NOT EXISTS(SELECT 1 FROM deleted_messages WHERE id=json_extract(member.value,'$.id'));
         {REVISION}"
    )
}

impl Store {
    pub(crate) fn init_cache(&self) -> Result<()> {
        self.db.execute_batch(
            "CREATE TABLE IF NOT EXISTS mail_cache_meta(singleton INTEGER PRIMARY KEY CHECK(singleton=1),revision INTEGER NOT NULL);
             INSERT OR IGNORE INTO mail_cache_meta VALUES(1,0);
             CREATE TABLE IF NOT EXISTS mail_summaries(
               id TEXT PRIMARY KEY,thread_key TEXT NOT NULL,received INTEGER NOT NULL,summary TEXT NOT NULL,
               trashed INTEGER NOT NULL,spam INTEGER NOT NULL,snoozed INTEGER NOT NULL,unread INTEGER NOT NULL);
             CREATE INDEX IF NOT EXISTS mail_summaries_thread ON mail_summaries(thread_key,received,id);
             CREATE TABLE IF NOT EXISTS mail_labels(label TEXT NOT NULL,id TEXT NOT NULL,thread_key TEXT NOT NULL,newest INTEGER NOT NULL,
               PRIMARY KEY(label,id));
             CREATE INDEX IF NOT EXISTS mail_labels_newest ON mail_labels(label,newest DESC,thread_key,id);
             CREATE INDEX IF NOT EXISTS mail_labels_message ON mail_labels(id);
             CREATE INDEX IF NOT EXISTS mail_labels_thread ON mail_labels(thread_key);
             CREATE TABLE IF NOT EXISTS mail_views(source TEXT NOT NULL,key TEXT NOT NULL,next TEXT NOT NULL,complete INTEGER NOT NULL,
               refreshed_at INTEGER NOT NULL DEFAULT 0,PRIMARY KEY(source,key));
             CREATE TABLE IF NOT EXISTS mail_view_messages(source TEXT NOT NULL,key TEXT NOT NULL,id TEXT NOT NULL,position INTEGER NOT NULL,
               PRIMARY KEY(source,key,id));
             CREATE INDEX IF NOT EXISTS mail_view_order ON mail_view_messages(source,key,position);
             CREATE INDEX IF NOT EXISTS mail_view_message ON mail_view_messages(id);",
        )?;
        self.db.execute_batch(
            "CREATE TABLE IF NOT EXISTS mail_folder_receipts(
            label TEXT NOT NULL,query TEXT NOT NULL,id TEXT NOT NULL,observed_at INTEGER NOT NULL,
            PRIMARY KEY(label,query,id));",
        )?;
        self.db.execute_batch(
            "CREATE TABLE IF NOT EXISTS mail_thread_freshness(id TEXT PRIMARY KEY,refreshed_at INTEGER NOT NULL DEFAULT 0);"
        )?;
        for operation in ["INSERT", "UPDATE"] {
            self.db.execute_batch(&format!(
                "CREATE TRIGGER IF NOT EXISTS mail_observed_labels_{operation}
                 AFTER {operation} ON message_observations BEGIN
                   UPDATE mail_summaries SET summary=json_set(summary,'$.labelIds',json(NEW.labels)),
                     trashed=EXISTS(SELECT 1 FROM json_each(NEW.labels) WHERE value='TRASH'),
                     spam=EXISTS(SELECT 1 FROM json_each(NEW.labels) WHERE value='SPAM'),
                     snoozed=EXISTS(SELECT 1 FROM json_each(NEW.labels) WHERE value='SNOOZED'),
                     unread=EXISTS(SELECT 1 FROM json_each(NEW.labels) WHERE value='UNREAD')
                   WHERE id=NEW.id;
                 END;"
            ))?;
            self.db.execute_batch(&format!(
                "CREATE TRIGGER IF NOT EXISTS mail_summary_labels_{operation}
                 AFTER {operation} ON mail_summaries BEGIN
                   DELETE FROM mail_labels WHERE id=NEW.id;
                   INSERT OR IGNORE INTO mail_labels(label,id,thread_key,newest)
                   SELECT DISTINCT value,NEW.id,NEW.thread_key,
                     (SELECT MAX(received) FROM mail_summaries WHERE thread_key=NEW.thread_key)
                   FROM json_each(NEW.summary,'$.labelIds');
                   UPDATE mail_labels SET newest=(SELECT MAX(received) FROM mail_summaries WHERE thread_key=NEW.thread_key)
                     WHERE thread_key=NEW.thread_key;
                   {REVISION}
                 END;"
            ))?;
            self.db.execute_batch(&format!(
                "CREATE TRIGGER IF NOT EXISTS mail_index_message_{operation}
                 AFTER {operation} ON messages BEGIN {} END;",
                summaries("SELECT NEW.value AS value")
            ))?;
            self.db.execute_batch(&format!(
                "CREATE TRIGGER IF NOT EXISTS mail_index_thread_{operation}
                 AFTER {operation} ON kv WHEN NEW.key LIKE 'thread:%' BEGIN {}
                   INSERT INTO mail_thread_freshness(id,refreshed_at) VALUES(substr(NEW.key,8),0)
                   ON CONFLICT(id) DO UPDATE SET refreshed_at=0;
                 END;",
                summaries("SELECT value FROM json_each(NEW.value)")
            ))?;
            for table in ["searches", "partial_searches"] {
                self.db.execute_batch(&format!(
                    "CREATE TRIGGER IF NOT EXISTS mail_index_{table}_{operation}
                     AFTER {operation} ON {table} BEGIN {} {} END;",
                    summaries("SELECT value FROM json_each(NEW.value,'$.messages')"),
                    view_snapshot(table, "NEW.key", "NEW.value"),
                ))?;
            }
        }
        self.db.execute_batch(&format!(
            "CREATE TRIGGER IF NOT EXISTS mail_summary_deleted AFTER DELETE ON mail_summaries BEGIN
               DELETE FROM mail_labels WHERE id=OLD.id;
               DELETE FROM mail_view_messages WHERE id=OLD.id;
               UPDATE mail_labels SET newest=COALESCE((SELECT MAX(received) FROM mail_summaries WHERE thread_key=OLD.thread_key),0)
                 WHERE thread_key=OLD.thread_key;
               {REVISION}
             END;
             CREATE TRIGGER IF NOT EXISTS mail_index_tombstone AFTER INSERT ON deleted_messages BEGIN
               DELETE FROM mail_summaries WHERE id=NEW.id;
               DELETE FROM mail_folder_receipts WHERE id=NEW.id;
               UPDATE mail_views SET next='',refreshed_at=0;
               {REVISION}
             END;
             CREATE TRIGGER IF NOT EXISTS mail_body_invalidated AFTER DELETE ON messages BEGIN
               UPDATE mail_views SET refreshed_at=0 WHERE EXISTS(
                 SELECT 1 FROM mail_view_messages AS member
                 WHERE member.source=mail_views.source AND member.key=mail_views.key AND member.id=OLD.id);
               UPDATE mail_thread_freshness SET refreshed_at=0 WHERE id=(
                 SELECT thread_key FROM mail_summaries WHERE id=OLD.id);
               {REVISION}
             END;
             CREATE TRIGGER IF NOT EXISTS mail_thread_deleted AFTER DELETE ON kv WHEN OLD.key LIKE 'thread:%' BEGIN
               DELETE FROM mail_thread_freshness WHERE id=substr(OLD.key,8);
               {REVISION}
             END;"
        ))?;
        for table in ["searches", "partial_searches"] {
            self.db.execute_batch(&format!(
                "CREATE TRIGGER IF NOT EXISTS mail_index_{table}_DELETE
                 AFTER DELETE ON {table} BEGIN
                   DELETE FROM mail_views WHERE source='{table}' AND key=OLD.key;
                   DELETE FROM mail_view_messages WHERE source='{table}' AND key=OLD.key;
                   {REVISION}
                 END;"
            ))?;
        }
        for operation in ["INSERT", "UPDATE", "DELETE"] {
            self.db.execute_batch(&format!(
                "CREATE TRIGGER IF NOT EXISTS mail_actions_revision_{operation}
                 AFTER {operation} ON mailbox_actions BEGIN {REVISION} END;"
            ))?;
            self.db.execute_batch(&format!(
                "CREATE TRIGGER IF NOT EXISTS mail_receipts_revision_{operation}
                 AFTER {operation} ON mail_folder_receipts BEGIN {REVISION} END;"
            ))?;
        }
        // Migrate once, before entering the UI loop. Later folder visits touch
        // only indexed metadata for their 25 conversations.
        let initialized: bool = self.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM kv WHERE key='mail_summary_index_v1')",
            [],
            |r| r.get(0),
        )?;
        if !initialized {
            let tx = self.db.unchecked_transaction()?;
            // Prefer the same source precedence as the former fallback.
            for table in ["partial_searches", "searches"] {
                tx.execute_batch(&summaries(&format!(
                    "SELECT member.value FROM {table},json_each({table}.value,'$.messages') AS member"
                )))?;
                let rows = {
                    let mut statement = tx.prepare(&format!("SELECT key,value FROM {table}"))?;
                    statement
                        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
                        .collect::<std::result::Result<Vec<_>, _>>()?
                };
                for (key, value) in rows {
                    // Parameters keep arbitrary Gmail search text out of SQL.
                    tx.execute("INSERT INTO mail_views(source,key,next,complete,refreshed_at)
                        VALUES(?1,?2,COALESCE(json_extract(?3,'$.next'),''),COALESCE(json_extract(?3,'$.threaded'),0),0)
                        ON CONFLICT(source,key) DO NOTHING", params![table,key,value])?;
                    tx.execute("INSERT OR IGNORE INTO mail_view_messages(source,key,id,position)
                        SELECT ?1,?2,json_extract(value,'$.id'),CAST(key AS INTEGER) FROM json_each(?3,'$.messages')
                        WHERE COALESCE(json_extract(value,'$.id'),'')!=''
                        AND NOT EXISTS(SELECT 1 FROM deleted_messages WHERE id=json_extract(value,'$.id'))",params![table,key,value])?;
                }
            }
            tx.execute_batch(&summaries(
                "SELECT member.value FROM kv,json_each(CASE WHEN kv.key LIKE 'thread:%' THEN kv.value ELSE '[]' END) AS member"
            ))?;
            tx.execute_batch(&summaries("SELECT value FROM messages"))?;
            tx.execute(
                "INSERT OR REPLACE INTO kv VALUES('mail_summary_index_v1','true')",
                [],
            )?;
            tx.commit()?;
        }
        Ok(())
    }

    pub fn cache_revision(&self) -> Result<i64> {
        Ok(self.db.query_row(
            "SELECT revision FROM mail_cache_meta WHERE singleton=1",
            [],
            |r| r.get(0),
        )?)
    }

    pub(crate) fn mark_page_fresh(&self, key: &str) -> Result<()> {
        self.db.execute("UPDATE mail_views SET refreshed_at=?1 WHERE source='searches' AND key=?2 AND complete=1",
            params![crate::auth::now(), key])?;
        Ok(())
    }

    pub fn invalidate_page_freshness(&self) -> Result<()> {
        self.db
            .execute("UPDATE mail_views SET refreshed_at=0", [])?;
        self.db
            .execute("UPDATE mail_thread_freshness SET refreshed_at=0", [])?;
        Ok(())
    }

    pub(crate) fn invalidate_cached_message(&self, id: &str) -> Result<()> {
        self.db.execute(
            "UPDATE mail_views SET refreshed_at=0 WHERE EXISTS(
            SELECT 1 FROM mail_view_messages AS member WHERE member.source=mail_views.source
            AND member.key=mail_views.key AND member.id=?1)",
            [id],
        )?;
        self.db.execute(
            "UPDATE mail_thread_freshness SET refreshed_at=0 WHERE id=(
            SELECT thread_key FROM mail_summaries WHERE id=?1)",
            [id],
        )?;
        Ok(())
    }

    pub(crate) fn update_summary_labels(
        &self,
        id: &str,
        add: &[String],
        remove: &[String],
    ) -> Result<()> {
        let value: Option<String> = self
            .db
            .query_row("SELECT summary FROM mail_summaries WHERE id=?", [id], |r| {
                r.get(0)
            })
            .optional()?;
        if let Some(value) = value {
            let mut message: Message = serde_json::from_str(&value)?;
            message.label_ids.retain(|label| !remove.contains(label));
            for label in add {
                if !message.label_ids.contains(label) {
                    message.label_ids.push(label.clone());
                }
            }
            self.db.execute(
                &summaries("SELECT ?1 AS value"),
                [serde_json::to_string(&message)?],
            )?;
        }
        Ok(())
    }

    pub fn mark_thread_fresh(&self, id: &str) -> Result<()> {
        self.db.execute(
            "INSERT INTO mail_thread_freshness VALUES(?1,?2)
            ON CONFLICT(id) DO UPDATE SET refreshed_at=excluded.refreshed_at",
            params![id, crate::auth::now()],
        )?;
        Ok(())
    }

    pub fn thread_is_fresh(&self, id: &str, max_age_secs: u64) -> Result<bool> {
        let refreshed: Option<u64> = self
            .db
            .query_row(
                "SELECT refreshed_at FROM mail_thread_freshness WHERE id=?",
                [id],
                |r| r.get(0),
            )
            .optional()?;
        let now = crate::auth::now();
        Ok(refreshed.is_some_and(|time| time > 0 && time <= now && now - time < max_age_secs))
    }

    pub fn page_is_fresh(
        &self,
        label: &str,
        query: &str,
        token: &str,
        max_age_secs: u64,
    ) -> Result<bool> {
        let key = crate::worker::key(label, query, token);
        let refreshed: Option<u64> = self.db.query_row(
            "SELECT refreshed_at FROM mail_views WHERE source='searches' AND key=? AND complete=1",
            [key], |r| r.get(0),
        ).optional()?;
        let now = crate::auth::now();
        Ok(refreshed.is_some_and(|time| time > 0 && time <= now && now - time < max_age_secs))
    }

    /// Only labels changed by a confirmed action can add membership to an older
    /// snapshot. Unrelated known mail must not refill authoritative empty views.
    pub(crate) fn apply_receipt_membership(
        &self,
        ids: &[String],
        add: &[String],
        _remove: &[String],
    ) -> Result<()> {
        for id in ids {
            let value: Option<(String,u64)> = self.db.query_row(
                "SELECT summary,read_started FROM mail_summaries JOIN message_observations USING(id) WHERE id=?",
                [id],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
            let Some((value, observed_at)) = value else {
                continue;
            };
            let message: Message = serde_json::from_str(&value)?;
            for label in &message.label_ids {
                for query in ["", "is:unread"] {
                    if (query == "is:unread" && !message.unread())
                        || !(add.contains(label)
                            || (query == "is:unread" && add.iter().any(|l| l == "UNREAD")))
                    {
                        continue;
                    }
                    self.db.execute("INSERT INTO mail_folder_receipts VALUES(?1,?2,?3,?4)
                        ON CONFLICT(label,query,id) DO UPDATE SET observed_at=MAX(observed_at,excluded.observed_at)",
                        params![label,query,id,observed_at])?;
                }
            }
        }
        Ok(())
    }

    /// A complete first-page read begun after a receipt owns folder membership,
    /// including a confirmed empty page. Retain confirmed additions while they
    /// remain in that snapshot, so a late old writer cannot drop them at handoff.
    pub fn confirm_folder_snapshot(&self, key: &str, read_started_ms: u64) -> Result<()> {
        let (label, query, token): (String, String, String) = serde_json::from_str(key)?;
        if token.is_empty() && !label.is_empty() && matches!(query.as_str(), "" | "is:unread") {
            let transaction = self.db.unchecked_transaction()?;
            self.db.execute(
                "UPDATE mail_folder_receipts SET observed_at=?3
                 WHERE label=?1 AND query=?2 AND observed_at<?3 AND EXISTS(
                   SELECT 1 FROM mail_view_messages AS member
                   WHERE member.source='searches' AND member.key=?4 AND member.id=mail_folder_receipts.id)",
                params![label, query, read_started_ms, key],
            )?;
            self.db.execute(
                "DELETE FROM mail_folder_receipts WHERE label=?1 AND query=?2 AND observed_at<?3",
                params![label, query, read_started_ms],
            )?;
            transaction.commit()?;
        }
        Ok(())
    }

    fn include_folder_receipts(
        &self,
        label: &str,
        query: &str,
        token: &str,
        page: &mut Page,
    ) -> Result<()> {
        if !token.is_empty() || label.is_empty() || !matches!(query, "" | "is:unread") {
            return Ok(());
        }
        let mut statement=self.db.prepare("WITH chosen AS (
            SELECT DISTINCT labels.thread_key,labels.newest FROM mail_labels AS labels
            JOIN mail_folder_receipts AS receipt ON receipt.id=labels.id AND receipt.label=labels.label
            JOIN mail_summaries AS message ON message.id=labels.id
            WHERE receipt.label=?1 AND receipt.query=?2
              AND (?1='TRASH' OR message.trashed=0) AND (?1='SPAM' OR message.spam=0)
              AND (?1!='INBOX' OR message.snoozed=0) AND (?2!='is:unread' OR message.unread=1)
            ORDER BY labels.newest DESC,labels.thread_key LIMIT 25
          ) SELECT message.summary FROM chosen JOIN mail_summaries AS message USING(thread_key)
            ORDER BY chosen.newest DESC,chosen.thread_key,message.received,message.id")?;
        let extras = statement
            .query_map(params![label, query], |r| r.get::<_, String>(0))?
            .map(|value| Ok(serde_json::from_str::<Message>(&value?)?))
            .collect::<Result<Vec<_>>>()?;
        if extras.is_empty() {
            return Ok(());
        }
        let mut seen: std::collections::HashSet<_> =
            page.messages.iter().map(|m| m.id.clone()).collect();
        let mut added = false;
        for message in extras {
            if seen.insert(message.id.clone()) {
                page.messages.push(message);
                added = true;
            }
        }
        if added {
            page.next.clear();
            let mut conversations = page.conversations();
            conversations.sort_by_key(|row| {
                std::cmp::Reverse(row.latest().internal_date.parse::<i64>().unwrap_or(0))
            });
            page.messages = conversations
                .into_iter()
                .take(25)
                .flat_map(|row| row.messages.into_iter().cloned())
                .collect();
        }
        Ok(())
    }

    fn indexed_view(&self, source: &str, key: &str) -> Result<Option<(Page, bool)>> {
        let meta: Option<(String, bool)> = self
            .db
            .query_row(
                "SELECT next,complete FROM mail_views WHERE source=?1 AND key=?2",
                params![source, key],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let Some((next, complete)) = meta else {
            return Ok(None);
        };
        let mut statement = self.db.prepare(
            "SELECT summary FROM mail_view_messages AS member JOIN mail_summaries AS message ON message.id=member.id
             WHERE member.source=?1 AND member.key=?2 ORDER BY member.position"
        )?;
        let messages = statement
            .query_map(params![source, key], |r| r.get::<_, String>(0))?
            .map(|value| Ok(serde_json::from_str::<Message>(&value?)?))
            .collect::<Result<Vec<_>>>()?;
        Ok(Some((
            Page {
                messages,
                next: if complete && source == "searches" {
                    next
                } else {
                    String::new()
                },
                threaded: true,
            },
            complete,
        )))
    }

    /// Raw confirmed snapshots. The action layer projects pending changes once,
    /// after this lookup, so retries and rollback share the same baseline.
    pub fn cached_folder_page(
        &self,
        label: &str,
        query: &str,
        token: &str,
    ) -> Result<Option<Page>> {
        let key = crate::worker::key(label, query, token);
        let exact = self.indexed_view("searches", &key)?;
        if let Some((page, true)) = exact.as_ref() {
            let mut page = page.clone();
            self.include_folder_receipts(label, query, token, &mut page)?;
            return Ok(Some(page));
        }
        let partial = self.indexed_view("partial_searches", &key)?;
        let mut fallback = exact
            .into_iter()
            .chain(partial)
            .map(|(page, _)| page)
            .max_by_key(|page| page.conversations().len());
        if fallback
            .as_ref()
            .is_some_and(|page| !page.messages.is_empty())
        {
            self.include_folder_receipts(label, query, token, fallback.as_mut().unwrap())?;
            return Ok(fallback);
        }
        if label.is_empty() || !token.is_empty() || !matches!(query, "" | "is:unread") {
            return Ok(fallback);
        }
        let page = self.indexed_folder_candidates(label, query, 25)?;
        if !page.messages.is_empty() {
            fallback = Some(page);
        }
        Ok(fallback)
    }

    /// Extra indexed candidates let pending archives replenish the visible 25
    /// rows without a Gmail request. Callers apply the action projection once.
    pub(crate) fn indexed_folder_candidates(
        &self,
        label: &str,
        query: &str,
        limit: usize,
    ) -> Result<Page> {
        let mut page = Page {
            threaded: true,
            ..Page::default()
        };
        if label.is_empty() || !matches!(query, "" | "is:unread") || limit == 0 {
            return Ok(page);
        }
        // The label/newest index supplies only the requested matching threads;
        // the thread index then restores their known members, including Sent.
        let mut statement = self.db.prepare(
            "WITH selected AS (
               SELECT DISTINCT labels.thread_key,labels.newest
               FROM mail_labels AS labels INDEXED BY mail_labels_newest
               JOIN mail_summaries AS message ON message.id=labels.id
               WHERE labels.label=?1 AND (?1='TRASH' OR message.trashed=0)
                 AND (?1='SPAM' OR message.spam=0) AND (?1!='INBOX' OR message.snoozed=0)
                 AND (?2!='is:unread' OR message.unread=1)
               ORDER BY labels.newest DESC,labels.thread_key LIMIT ?3
             ) SELECT message.summary FROM selected
               JOIN mail_summaries AS message ON message.thread_key=selected.thread_key
               ORDER BY selected.newest DESC,selected.thread_key,message.received,message.id",
        )?;
        page.messages = statement
            .query_map(
                params![label, query, limit.min(i64::MAX as usize) as i64],
                |r| r.get::<_, String>(0),
            )?
            .map(|value| Ok(serde_json::from_str::<Message>(&value?)?))
            .collect::<Result<Vec<_>>>()?;
        Ok(page)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::Action;

    fn sample(index: usize, label: &str) -> Message {
        let mut message = crate::model::demo().1.messages.remove(0);
        message.id = format!("cache-message-{index}");
        message.thread_id = format!("cache-thread-{index}");
        message.internal_date = (1_793_491_200_000_u64 + index as u64 * 1000).to_string();
        message.label_ids = vec![label.into(), "UNREAD".into()];
        message
    }

    #[test]
    fn exact_snapshot_has_body_free_order_cursor_and_real_freshness() {
        let store = Store::open(None).unwrap();
        let key = crate::worker::key("INBOX", "", "");
        let page = Page {
            messages: vec![sample(2, "INBOX"), sample(1, "INBOX")],
            next: "next-thread-page".into(),
            threaded: true,
        };
        store.save_page(&key, &page).unwrap();
        let cached = store.cached_folder_page("INBOX", "", "").unwrap().unwrap();
        assert_eq!(cached.messages[0].id, page.messages[0].id);
        assert_eq!(cached.next, page.next);
        assert!(cached.messages.iter().all(|message| message.summary_only
            && message.payload.body.data.is_empty()
            && message.payload.parts.is_empty()));
        assert!(store.page_is_fresh("INBOX", "", "", 60).unwrap());
        store
            .save_partial_page(&crate::worker::key("INBOX", "is:unread", ""), &page)
            .unwrap();
        assert!(!store.page_is_fresh("INBOX", "is:unread", "", 60).unwrap());
        store
            .invalidate_cached_message(&page.messages[0].id)
            .unwrap();
        assert!(!store.page_is_fresh("INBOX", "", "", 60).unwrap());
        store
            .save_page(
                &key,
                &Page {
                    threaded: true,
                    ..Page::default()
                },
            )
            .unwrap();
        assert!(
            store
                .cached_folder_page("INBOX", "", "")
                .unwrap()
                .unwrap()
                .messages
                .is_empty()
        );
        assert!(store.page_is_fresh("INBOX", "", "", 60).unwrap());
    }

    #[test]
    fn confirmed_destination_membership_survives_old_empty_writer_until_fresh_snapshot() {
        let dir = tempfile::tempdir_in("/tmp").unwrap();
        let message = sample(1, "INBOX");
        let key = crate::worker::key("TRASH", "", "");
        let empty = Page {
            threaded: true,
            ..Default::default()
        };
        let store = Store::open(Some(dir.path())).unwrap();
        store.save_message(&message).unwrap();
        store.save_page(&key, &empty).unwrap();
        let action = store
            .enqueue_action(
                Action::Trash {
                    id: message.id.clone(),
                    thread: message.thread_id.clone(),
                },
                std::slice::from_ref(&message),
            )
            .unwrap();
        store
            .complete_action(action, std::slice::from_ref(&message.id))
            .unwrap();
        assert!(store.actions().unwrap().is_empty());
        assert_eq!(
            store
                .cached_page("TRASH", "", "")
                .unwrap()
                .unwrap()
                .messages[0]
                .id,
            message.id
        );
        // No request timestamp accompanies an older sync binary's SQL writes.
        store
            .db
            .execute(
                "INSERT OR REPLACE INTO searches VALUES(?1,?2)",
                params![key, serde_json::to_string(&empty).unwrap()],
            )
            .unwrap();
        store
            .db
            .execute(
                "INSERT OR REPLACE INTO messages VALUES(?1,?2)",
                params![message.id, serde_json::to_string(&message).unwrap()],
            )
            .unwrap();
        assert_eq!(
            store
                .cached_page("TRASH", "", "")
                .unwrap()
                .unwrap()
                .messages[0]
                .id,
            message.id
        );
        let receipt_at: u64 = store
            .db
            .query_row(
                "SELECT MAX(observed_at) FROM mail_folder_receipts",
                [],
                |r| r.get(0),
            )
            .unwrap();
        drop(store);
        let store = Store::open(Some(dir.path())).unwrap();
        assert_eq!(
            store
                .cached_page("TRASH", "", "")
                .unwrap()
                .unwrap()
                .messages[0]
                .id,
            message.id
        );
        // Confirmation hands membership to the refreshed snapshot without
        // exposing a window in which an older sync writer can drop the row.
        let confirmed = store.cached_page("TRASH", "", "").unwrap().unwrap();
        store.save_page(&key, &confirmed).unwrap();
        store.confirm_folder_snapshot(&key, receipt_at + 1).unwrap();
        store
            .db
            .execute(
                "INSERT OR REPLACE INTO searches VALUES(?1,?2)",
                params![key, serde_json::to_string(&empty).unwrap()],
            )
            .unwrap();
        assert_eq!(
            store
                .cached_page("TRASH", "", "")
                .unwrap()
                .unwrap()
                .messages
                .len(),
            1
        );
        // A search result or a later page does not own the first-page fence.
        for other_key in [
            crate::worker::key("TRASH", "", "next-page"),
            crate::worker::key("TRASH", "subject:missing", ""),
            crate::worker::key("TRASH", "is:unread", ""),
            crate::worker::key("INBOX", "", ""),
        ] {
            store
                .confirm_folder_snapshot(&other_key, receipt_at + 2)
                .unwrap();
        }
        assert_eq!(
            store
                .cached_page("TRASH", "", "")
                .unwrap()
                .unwrap()
                .messages
                .len(),
            1
        );
        store.save_page(&key, &empty).unwrap();
        store.confirm_folder_snapshot(&key, receipt_at + 2).unwrap();
        assert!(
            store
                .cached_page("TRASH", "", "")
                .unwrap()
                .unwrap()
                .messages
                .is_empty()
        );
    }

    #[test]
    fn receipt_only_adds_changed_membership_not_unrelated_stale_folder_rows() {
        let store = Store::open(None).unwrap();
        let message = sample(1, "INBOX");
        store.save_message(&message).unwrap();
        let empty = Page {
            threaded: true,
            ..Default::default()
        };
        for label in ["INBOX", "STARRED"] {
            store
                .save_page(&crate::worker::key(label, "", ""), &empty)
                .unwrap();
        }
        let action = store
            .enqueue_action(
                Action::Labels {
                    id: message.id.clone(),
                    thread: message.thread_id.clone(),
                    add: vec!["STARRED".into()],
                    remove: vec![],
                },
                std::slice::from_ref(&message),
            )
            .unwrap();
        store
            .complete_action(action, std::slice::from_ref(&message.id))
            .unwrap();
        assert_eq!(
            store
                .cached_page("STARRED", "", "")
                .unwrap()
                .unwrap()
                .messages
                .len(),
            1
        );
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
    fn indexed_fallback_keeps_sent_members_and_skips_spam_trash_snoozed() {
        let store = Store::open(None).unwrap();
        for index in 0..40 {
            store.save_message(&sample(index, "INBOX")).unwrap();
        }
        let mut sent = sample(100, "SENT");
        sent.thread_id = "cache-thread-0".into();
        store.save_message(&sent).unwrap();
        for (index, excluded) in [(97, "SPAM"), (98, "TRASH"), (99, "SNOOZED")] {
            let mut message = sample(index, "INBOX");
            message.label_ids.push(excluded.into());
            store.save_message(&message).unwrap();
        }
        let page = store.cached_folder_page("INBOX", "", "").unwrap().unwrap();
        assert_eq!(page.conversations().len(), 25);
        assert_eq!(page.messages.len(), 26);
        assert_eq!(page.messages[0].thread_id, "cache-thread-0");
        assert!(page.messages.iter().any(|message| message.id == sent.id));
        assert!(!page.messages.iter().any(|message| matches!(
            message.id.as_str(),
            "cache-message-97" | "cache-message-98" | "cache-message-99"
        )));
        assert!(
            store
                .cached_folder_page("INBOX", "from:someone", "")
                .unwrap()
                .is_none()
        );
        assert!(
            store
                .cached_folder_page("INBOX", "", "page-2")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn old_writers_tombstones_and_action_changes_update_index_revision() {
        let store = Store::open(None).unwrap();
        let original = sample(1, "INBOX");
        let mut changed = original.clone();
        changed.label_ids = vec!["STARRED".into()];
        let before = store.cache_revision().unwrap();
        // Direct SQL simulates the already-running sync process's existing API.
        store
            .db
            .execute(
                "INSERT OR REPLACE INTO messages VALUES(?1,?2)",
                params![original.id, serde_json::to_string(&original).unwrap()],
            )
            .unwrap();
        assert!(store.cache_revision().unwrap() > before);
        assert_eq!(
            store
                .cached_folder_page("INBOX", "", "")
                .unwrap()
                .unwrap()
                .messages
                .len(),
            1
        );
        store
            .db
            .execute(
                "UPDATE messages SET value=?1 WHERE id=?2",
                params![serde_json::to_string(&changed).unwrap(), original.id],
            )
            .unwrap();
        assert!(store.cached_folder_page("INBOX", "", "").unwrap().is_none());
        assert_eq!(
            store
                .cached_folder_page("STARRED", "", "")
                .unwrap()
                .unwrap()
                .messages
                .len(),
            1
        );
        let before = store.cache_revision().unwrap();
        let action = store
            .enqueue_action(
                Action::Trash {
                    id: changed.id.clone(),
                    thread: changed.thread_id.clone(),
                },
                std::slice::from_ref(&changed),
            )
            .unwrap();
        assert!(store.cache_revision().unwrap() > before);
        let projected = store.cached_page("TRASH", "", "").unwrap().unwrap();
        assert_eq!(projected.messages[0].id, changed.id);
        let before = store.cache_revision().unwrap();
        store.fail_action(action, "synthetic rejection").unwrap();
        assert!(store.cache_revision().unwrap() > before);
        assert!(
            store
                .cached_page("TRASH", "", "")
                .unwrap()
                .is_none_or(|page| page.messages.is_empty())
        );
        store
            .remove_messages(std::slice::from_ref(&original.id))
            .unwrap();
        store
            .db
            .execute(
                "INSERT OR REPLACE INTO messages VALUES(?1,?2)",
                params![original.id, serde_json::to_string(&original).unwrap()],
            )
            .unwrap();
        assert!(store.cached_folder_page("INBOX", "", "").unwrap().is_none());
        assert!(
            store
                .cached_folder_page("STARRED", "", "")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn legacy_cache_migrates_once_and_thread_invalidation_keeps_readable_body() {
        let dir = tempfile::tempdir_in("/tmp").unwrap();
        let original = sample(1, "INBOX");
        let key = crate::worker::key("INBOX", "", "");
        {
            let db = rusqlite::Connection::open(dir.path().join("mail.sqlite")).unwrap();
            db.execute_batch(
                "CREATE TABLE kv(key TEXT PRIMARY KEY,value TEXT NOT NULL);
                CREATE TABLE messages(id TEXT PRIMARY KEY,value TEXT NOT NULL);
                CREATE TABLE searches(key TEXT PRIMARY KEY,value TEXT NOT NULL);
                CREATE TABLE partial_searches(key TEXT PRIMARY KEY,value TEXT NOT NULL);",
            )
            .unwrap();
            db.execute(
                "INSERT INTO messages VALUES(?1,?2)",
                params![original.id, serde_json::to_string(&original).unwrap()],
            )
            .unwrap();
            db.execute(
                "INSERT INTO searches VALUES(?1,?2)",
                params![
                    key,
                    serde_json::to_string(&Page {
                        messages: vec![original.clone()],
                        next: "legacy-message-cursor".into(),
                        threaded: false
                    })
                    .unwrap()
                ],
            )
            .unwrap();
        }
        let store = Store::open(Some(dir.path())).unwrap();
        let page = store.cached_folder_page("INBOX", "", "").unwrap().unwrap();
        assert_eq!(page.messages[0].id, original.id);
        assert!(page.next.is_empty());
        assert!(!store.page_is_fresh("INBOX", "", "", 60).unwrap());
        store
            .put(
                &format!("thread:{}", original.thread_id),
                &vec![original.clone()],
            )
            .unwrap();
        store.mark_thread_fresh(&original.thread_id).unwrap();
        assert!(store.thread_is_fresh(&original.thread_id, 60).unwrap());
        store.invalidate(&original.id).unwrap();
        assert!(!store.thread_is_fresh(&original.thread_id, 60).unwrap());
        assert_eq!(
            store.cached_thread(&original.thread_id).unwrap().unwrap()[0].text(),
            original.text()
        );
        let revision = store.cache_revision().unwrap();
        drop(store);
        let store = Store::open(Some(dir.path())).unwrap();
        assert_eq!(store.cache_revision().unwrap(), revision);
        assert_eq!(
            store
                .cached_folder_page("INBOX", "", "")
                .unwrap()
                .unwrap()
                .messages[0]
                .id,
            original.id
        );
    }

    #[test]
    fn repeated_folder_navigation_reads_only_bounded_summaries() {
        let store = Store::open(None).unwrap();
        let base = sample(0, "unused");
        let start = std::time::Instant::now();
        let tx = store.db.unchecked_transaction().unwrap();
        {
            let mut insert = tx.prepare("INSERT INTO messages VALUES(?1,?2)").unwrap();
            for index in 0..5000 {
                let mut message = base.clone();
                message.id = format!("bulk-{index}");
                message.thread_id = message.id.clone();
                message.internal_date = (1_793_491_200_000_u64 + index).to_string();
                message.label_ids = vec![format!("folder-{}", index % 40), "UNREAD".into()];
                // 20 MB of body data must never be decoded during navigation.
                message.payload.body.data = "a".repeat(4096);
                insert
                    .execute(params![
                        message.id,
                        serde_json::to_string(&message).unwrap()
                    ])
                    .unwrap();
            }
        }
        tx.commit().unwrap();
        let seed_elapsed = start.elapsed();
        let start = std::time::Instant::now();
        for index in 0..200 {
            let page = store
                .cached_folder_page(&format!("folder-{}", index % 40), "", "")
                .unwrap()
                .unwrap();
            assert_eq!(page.messages.len(), 25);
            assert!(
                page.messages
                    .iter()
                    .all(|message| message.payload.body.data.is_empty() && message.summary_only)
            );
        }
        let elapsed = start.elapsed();
        eprintln!(
            "folder index: 5000 messages seeded in {seed_elapsed:?}; 200 folder visits in {elapsed:?} ({:?}/visit)",
            elapsed / 200
        );
        assert!(
            elapsed < std::time::Duration::from_secs(5),
            "Indexed summary navigation took {elapsed:?}"
        );
        let summary_bytes: u64 = store
            .db
            .query_row("SELECT SUM(length(summary)) FROM mail_summaries", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert!(summary_bytes < 5_000_000);
    }
}
