//! Desktop storage uses the SDK's SQLite pool so auth and inbox share its writer.

use crate::{Claim, InboxMessage, Journal, Outbound, SendState, TextRequest};
use anyhow::{Context, Result, ensure};
use diesel::{connection::SimpleConnection, prelude::*, sql_types::Text};
use std::{
    fs::{File, OpenOptions},
    path::Path,
    sync::Arc,
};
use wacore::store::error::StoreError;
use whatsapp_rust_sqlite_storage::{
    SharedSqlite, SqliteDatabase, SqliteDatabaseConfig, SqliteStore, Synchronous,
};

pub const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS whatsover_inbox (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    chat TEXT NOT NULL, sender TEXT NOT NULL, message_id TEXT NOT NULL,
    data TEXT NOT NULL, UNIQUE(chat, sender, message_id)
);
CREATE TABLE IF NOT EXISTS whatsover_outbox (
    request_id TEXT PRIMARY KEY, state TEXT NOT NULL, data TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS whatsover_attachments (
    id TEXT PRIMARY KEY, data TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS whatsover_delivery_events (
    id TEXT PRIMARY KEY, data TEXT NOT NULL
);";

fn db_error(error: diesel::result::Error) -> StoreError {
    StoreError::Database(Box::new(error))
}

#[derive(QueryableByName)]
struct JsonRow {
    #[diesel(sql_type = Text)]
    data: String,
}

#[derive(Clone)]
pub struct SqliteJournal(pub SharedSqlite);

impl SqliteJournal {
    pub async fn delivery_events(&self, message_id: &str) -> Result<Vec<serde_json::Value>> {
        let id = message_id.to_owned();
        let rows = self.0.read(move |conn| diesel::sql_query(
            "SELECT data FROM whatsover_delivery_events WHERE json_extract(data, '$.data.id') = ? OR EXISTS (SELECT 1 FROM json_each(json_extract(data, '$.data.message_ids')) WHERE value = ?) ORDER BY rowid"
        ).bind::<Text, _>(&id).bind::<Text, _>(&id).load::<JsonRow>(conn).map_err(db_error)).await?;
        rows.into_iter()
            .map(|r| Ok(serde_json::from_str(&r.data)?))
            .collect()
    }

    pub async fn record_delivery_events(&self, events: &[serde_json::Value]) -> Result<()> {
        use sha2::{Digest, Sha256};
        let rows = events
            .iter()
            .map(|event| {
                let data = serde_json::to_string(event)?;
                Ok((hex::encode(Sha256::digest(data.as_bytes())), data))
            })
            .collect::<Result<Vec<_>>>()?;
        self.0.run(move |conn| conn.transaction::<_, diesel::result::Error, _>(|conn| {
            for (id, data) in rows {
                diesel::sql_query("INSERT OR IGNORE INTO whatsover_delivery_events (id, data) VALUES (?, ?)")
                    .bind::<Text, _>(id).bind::<Text, _>(data).execute(conn)?;
            }
            Ok(())
        }).map_err(db_error)).await?;
        Ok(())
    }

    /// Upload descriptors contain media keys and stay in the private account DB.
    pub async fn stage_attachment(&self, id: String, data: String) -> Result<()> {
        self.0.run(move |conn| diesel::sql_query("INSERT INTO whatsover_attachments (id, data) VALUES (?, ?) ON CONFLICT(id) DO UPDATE SET data = excluded.data")
            .bind::<Text, _>(id).bind::<Text, _>(data).execute(conn).map_err(db_error)).await?;
        Ok(())
    }

    pub async fn initialize(&self) -> Result<()> {
        self.0
            .run(|conn| conn.batch_execute(SCHEMA).map_err(db_error))
            .await?;
        Ok(())
    }

    /// Called once after acquiring the exclusive account lock, before connecting.
    pub async fn recover(&self) -> Result<()> {
        self.0.run(|conn| diesel::sql_query("UPDATE whatsover_outbox SET state = 'uncertain', data = json_set(data, '$.state', 'uncertain') WHERE state = 'pending'").execute(conn).map_err(db_error)).await?;
        Ok(())
    }

    pub async fn inbox(&self, after: i64, limit: i64) -> Result<Vec<serde_json::Value>> {
        ensure!(
            after >= 0 && (1..=500).contains(&limit),
            "invalid inbox cursor or limit"
        );
        let rows = self.0.read(move |conn| diesel::sql_query("SELECT json_set(data, '$.sequence', seq) AS data FROM whatsover_inbox WHERE seq > ? ORDER BY seq LIMIT ?")
            .bind::<diesel::sql_types::BigInt, _>(after).bind::<diesel::sql_types::BigInt, _>(limit).load::<JsonRow>(conn).map_err(db_error)).await?;
        rows.into_iter()
            .map(|r| Ok(serde_json::from_str(&r.data)?))
            .collect()
    }

    pub async fn outbound(&self, id: &str) -> Result<Option<Outbound>> {
        let id = id.to_owned();
        let row = self
            .0
            .read(move |conn| {
                diesel::sql_query("SELECT data FROM whatsover_outbox WHERE request_id = ?")
                    .bind::<Text, _>(id)
                    .get_result::<JsonRow>(conn)
                    .optional()
                    .map_err(db_error)
            })
            .await?;
        row.map(|r| serde_json::from_str(&r.data))
            .transpose()
            .map_err(Into::into)
    }
}

#[async_trait::async_trait]
impl Journal for SqliteJournal {
    async fn attachment(&self, id: &str) -> Result<Option<String>> {
        let id = id.to_owned();
        let row = self
            .0
            .read(move |conn| {
                diesel::sql_query("SELECT data FROM whatsover_attachments WHERE id = ?")
                    .bind::<Text, _>(id)
                    .get_result::<JsonRow>(conn)
                    .optional()
                    .map_err(db_error)
            })
            .await?;
        Ok(row.map(|row| row.data))
    }

    async fn lookup(
        &self,
        chat: &str,
        target: &crate::content::MessageTarget,
    ) -> Result<Option<InboxMessage>> {
        let (chat, sender, id) = (chat.to_owned(), target.sender.clone(), target.id.clone());
        let row = self
            .0
            .read(move |conn| {
                diesel::sql_query(
                    "SELECT data FROM whatsover_inbox WHERE chat=? AND sender=? AND message_id=?",
                )
                .bind::<Text, _>(chat)
                .bind::<Text, _>(sender)
                .bind::<Text, _>(id)
                .get_result::<JsonRow>(conn)
                .optional()
                .map_err(db_error)
            })
            .await?;
        row.map(|row| serde_json::from_str(&row.data))
            .transpose()
            .map_err(Into::into)
    }

    async fn receive(&self, messages: Vec<InboxMessage>) -> Result<()> {
        let rows = messages
            .into_iter()
            .map(|m| {
                Ok((
                    m.chat.clone(),
                    m.sender.clone(),
                    m.id.clone(),
                    serde_json::to_string(&m)?,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        self.0.run(move |conn| conn.transaction::<_, diesel::result::Error, _>(|conn| {
            for (chat, sender, id, data) in rows {
                diesel::sql_query("INSERT INTO whatsover_inbox (chat, sender, message_id, data) VALUES (?, ?, ?, ?) ON CONFLICT(chat, sender, message_id) DO NOTHING")
                    .bind::<Text, _>(chat).bind::<Text, _>(sender).bind::<Text, _>(id).bind::<Text, _>(data).execute(conn)?;
            }
            Ok(())
        }).map_err(db_error)).await?;
        Ok(())
    }

    async fn claim(&self, request: TextRequest) -> Result<Claim> {
        let request = request.validated()?;
        let outbound = Outbound {
            message_id: request.message_id(),
            request: request.clone(),
            state: SendState::Pending,
        };
        let data = serde_json::to_string(&outbound)?;
        let id = request.request_id.clone();
        let (inserted, row) = self.0.run(move |conn| conn.transaction::<_, diesel::result::Error, _>(|conn| {
            let inserted = diesel::sql_query("INSERT INTO whatsover_outbox (request_id, state, data) VALUES (?, 'pending', ?) ON CONFLICT(request_id) DO NOTHING")
                .bind::<Text, _>(&id).bind::<Text, _>(data).execute(conn)?;
            let row = diesel::sql_query("SELECT data FROM whatsover_outbox WHERE request_id = ?").bind::<Text, _>(id).get_result::<JsonRow>(conn)?;
            Ok((inserted == 1, row))
        }).map_err(db_error)).await?;
        let previous: Outbound = serde_json::from_str(&row.data)?;
        ensure!(
            previous.request == request,
            "requestId already belongs to a different message"
        );
        Ok(if inserted {
            Claim::New(previous)
        } else {
            Claim::Existing(previous)
        })
    }

    async fn finish(&self, outbound: Outbound) -> Result<()> {
        ensure!(
            outbound.state != SendState::Pending,
            "cannot finish with pending state"
        );
        let state = match outbound.state {
            SendState::Submitted => "submitted",
            _ => "uncertain",
        };
        let data = serde_json::to_string(&outbound)?;
        let updated = self.0.run(move |conn| diesel::sql_query("UPDATE whatsover_outbox SET state = ?, data = ? WHERE request_id = ? AND state = 'pending'")
            .bind::<Text, _>(state).bind::<Text, _>(data).bind::<Text, _>(outbound.request.request_id).execute(conn).map_err(db_error)).await?;
        ensure!(updated == 1, "send claim was lost or already completed");
        Ok(())
    }
}

pub struct NativeAccount {
    pub protocol: SqliteStore,
    pub journal: Arc<SqliteJournal>,
    // Keep the owner lock alive through all engine shutdown/persistence work.
    _lock: File,
}

impl NativeAccount {
    pub async fn open(directory: &Path) -> Result<Self> {
        use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(directory)?;
        let directory = directory.canonicalize()?;
        ensure!(
            !directory
                .ancestors()
                .any(|p| p.join(".git").is_file() || p.join(".git/HEAD").is_file()),
            "WhatsApp state must live outside a repository"
        );
        ensure!(
            std::fs::metadata(&directory)?.permissions().mode() & 0o077 == 0,
            "state directory must have permissions 0700"
        );
        for file in [
            "owner.lock",
            "session.sqlite",
            "session.sqlite-wal",
            "session.sqlite-shm",
        ] {
            if let Ok(meta) = std::fs::symlink_metadata(directory.join(file)) {
                ensure!(
                    !meta.file_type().is_symlink(),
                    "state files must not be symlinks"
                );
            }
        }
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(directory.join("owner.lock"))?;
        lock.try_lock()
            .context("another Whatsover process owns this account")?;
        let db = SqliteDatabase::open(
            directory
                .join("session.sqlite")
                .to_str()
                .context("state path is not UTF-8")?,
            SqliteDatabaseConfig {
                synchronous: Synchronous::Full,
                ..Default::default()
            },
        )
        .await?;
        let protocol = if db.device_exists(1).await? {
            db.store(1)
        } else {
            db.provision_device(1).await?
        };
        let journal = Arc::new(SqliteJournal(db.shared()));
        journal.initialize().await?;
        journal.recover().await?;
        Ok(Self {
            protocol,
            journal,
            _lock: lock,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn private_directory() -> Result<tempfile::TempDir> {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir_in("/tmp")?;
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700))?;
        Ok(dir)
    }

    fn request(text: &str) -> TextRequest {
        TextRequest {
            request_id: "synthetic-1".into(),
            to: "+15555550100".into(),
            text: text.into(),
            reply_to: None,
            reaction: None,
            attachment_id: None,
        }
    }

    #[tokio::test]
    async fn claims_are_atomic_conflicts_fail_and_restarts_do_not_resend() -> Result<()> {
        let dir = private_directory()?;
        let account = NativeAccount::open(dir.path()).await?;
        assert!(NativeAccount::open(dir.path()).await.is_err());
        let (a, b) = tokio::join!(
            account.journal.claim(request("test")),
            account.journal.claim(request("test"))
        );
        assert!(matches!(
            (&a, &b),
            (Ok(Claim::New(_)), Ok(Claim::Existing(_)))
                | (Ok(Claim::Existing(_)), Ok(Claim::New(_)))
        ));
        assert!(account.journal.claim(request("different")).await.is_err());
        drop(account);
        let account = NativeAccount::open(dir.path()).await?;
        let Claim::Existing(record) = account.journal.claim(request("test")).await? else {
            panic!("replayed send must not get a fresh claim")
        };
        assert_eq!(record.state, SendState::Uncertain);
        Ok(())
    }

    #[tokio::test]
    async fn inbox_deduplicates_by_chat_sender_and_id_and_survives_reopen() -> Result<()> {
        let dir = private_directory()?;
        let account = NativeAccount::open(dir.path()).await?;
        let message = InboxMessage {
            chat: "15555550100@s.whatsapp.net".into(),
            sender: "15555550100@s.whatsapp.net".into(),
            id: "reused-id".into(),
            timestamp: 1,
            from_me: false,
            text: Some("synthetic".into()),
            payload: "{}".into(),
        };
        let mut other = message.clone();
        other.chat = "15555550101@s.whatsapp.net".into();
        account
            .journal
            .receive(vec![message.clone(), message, other])
            .await?;
        drop(account);
        let account = NativeAccount::open(dir.path()).await?;
        let rows = account.journal.inbox(0, 100).await?;
        assert_eq!(rows.len(), 2);
        let cursor = rows[0]["sequence"].as_i64().unwrap();
        assert_eq!(account.journal.inbox(cursor, 100).await?.len(), 1);
        Ok(())
    }
}
