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
    db: Connection,
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
            CREATE TABLE IF NOT EXISTS searches(key TEXT PRIMARY KEY, value TEXT NOT NULL);",
        )?;
        Ok(Self { db })
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
        json.map(|s| serde_json::from_str(&s).map_err(Into::into))
            .transpose()
    }
    pub fn save_message(&self, message: &Message) -> Result<()> {
        self.db.execute(
            "INSERT OR REPLACE INTO messages VALUES(?,?)",
            params![message.id, serde_json::to_string(message)?],
        )?;
        Ok(())
    }
    pub fn invalidate(&self, id: &str) -> Result<()> {
        self.db.execute("DELETE FROM messages WHERE id=?", [id])?;
        Ok(())
    }
    pub fn invalidate_all(&self) -> Result<()> {
        self.db.execute("DELETE FROM messages", [])?;
        Ok(())
    }
    pub fn apply_labels(&self, id: &str, add: &[String], remove: &[String]) -> Result<()> {
        let mut stmt = self.db.prepare("SELECT key,value FROM searches")?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        for (key, value) in rows {
            let (label, query, _): (String, String, String) = serde_json::from_str(&key)?;
            if !query.is_empty() {
                self.db.execute("DELETE FROM searches WHERE key=?", [key])?;
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
            if !label.is_empty() {
                page.messages.retain(|m| m.label_ids.contains(&label));
            }
            self.save_page(&key, &page)?;
        }
        self.invalidate(id)
    }
    pub fn page(&self, key: &str) -> Result<Option<Page>> {
        let json: Option<String> = self
            .db
            .query_row("SELECT value FROM searches WHERE key=?", [key], |r| {
                r.get(0)
            })
            .optional()?;
        json.map(|s| serde_json::from_str(&s).map_err(Into::into))
            .transpose()
    }
    pub fn save_page(&self, key: &str, page: &Page) -> Result<()> {
        self.db.execute(
            "INSERT OR REPLACE INTO searches VALUES(?,?)",
            params![key, serde_json::to_string(page)?],
        )?;
        Ok(())
    }
    pub fn draft(&self) -> Result<Draft> {
        Ok(self.get("composer")?.unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
