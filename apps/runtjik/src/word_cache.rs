//! Private, bounded dictionary history and successful source responses.
use crate::words::Row;
use anyhow::Result;
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use std::{fs, os::unix::fs::PermissionsExt, path::Path};

pub struct WordCache(Connection);
impl WordCache {
    pub fn open(path: Option<&Path>) -> Result<Self> {
        let db = if let Some(path) = path {
            crate::util::private_dir(path.parent().unwrap())?;
            let db = Connection::open(path)?;
            fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
            db
        } else {
            Connection::open_in_memory()?
        };
        db.busy_timeout(std::time::Duration::from_secs(2))?;
        db.execute_batch("PRAGMA journal_mode=WAL;
            CREATE TABLE IF NOT EXISTS word_cache(seq INTEGER PRIMARY KEY AUTOINCREMENT, key TEXT NOT NULL UNIQUE, data TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS word_history(seq INTEGER PRIMARY KEY AUTOINCREMENT, term TEXT NOT NULL UNIQUE);
            CREATE TABLE IF NOT EXISTS word_meta(key TEXT PRIMARY KEY);")?;
        let cache = Self(db);
        if let Some(path) = path {
            cache.import_command_history(&path.with_file_name("history.sqlite3"))?;
        }
        Ok(cache)
    }
    fn import_command_history(&self, path: &Path) -> Result<()> {
        let imported: bool = self.0.query_row(
            "SELECT EXISTS(SELECT 1 FROM word_meta WHERE key='command-history-v1')",
            [],
            |r| r.get(0),
        )?;
        if imported {
            return Ok(());
        }
        let mut terms = vec![];
        if path.exists() {
            let old = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
            let mut query = old.prepare("SELECT query FROM command_queries ORDER BY seq")?;
            for query in query.query_map([], |r| r.get::<_, String>(0))? {
                let query = query?;
                if let Some((command, term)) = query.trim().split_once(char::is_whitespace) {
                    if ["/define", "/synonym"].contains(&command) && !term.trim().is_empty() {
                        terms.push(term.trim().to_owned());
                    }
                }
            }
        }
        // The new history is more recent than the legacy command list.
        terms.extend(self.history()?.into_iter().rev());
        let tx = self.0.unchecked_transaction()?;
        for term in terms {
            tx.execute(
                "INSERT OR REPLACE INTO word_history(term) VALUES(?)",
                [term],
            )?;
        }
        tx.execute("DELETE FROM word_history WHERE seq NOT IN (SELECT seq FROM word_history ORDER BY seq DESC LIMIT 100)", [])?;
        tx.execute(
            "INSERT INTO word_meta(key) VALUES('command-history-v1')",
            [],
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn get(&self, key: &str) -> Result<Option<Vec<Row>>> {
        let data: Option<String> = self
            .0
            .query_row("SELECT data FROM word_cache WHERE key=?", [key], |r| {
                r.get(0)
            })
            .optional()?;
        data.map(|data| {
            let rows = serde_json::from_str(&data)?;
            self.put(key, &rows)?;
            Ok(rows)
        })
        .transpose()
    }
    pub fn put(&self, key: &str, rows: &Vec<Row>) -> Result<()> {
        let data = serde_json::to_string(rows)?;
        if data.len() > 2_000_000 {
            return Ok(());
        }
        self.0.execute(
            "INSERT OR REPLACE INTO word_cache(key,data) VALUES(?,?)",
            params![key, data],
        )?;
        // Keep at most 256 responses and 32 MiB of serialized payloads, most recent first.
        self.0.execute_batch(
            "DELETE FROM word_cache WHERE seq IN (
            SELECT seq FROM (SELECT seq, ROW_NUMBER() OVER (ORDER BY seq DESC) AS n,
            SUM(length(CAST(data AS BLOB))) OVER (ORDER BY seq DESC) AS bytes FROM word_cache)
            WHERE n>256 OR bytes>33554432)",
        )?;
        Ok(())
    }
    pub fn remember(&self, term: &str) -> Result<()> {
        self.0.execute(
            "INSERT OR REPLACE INTO word_history(term) VALUES(?)",
            [term],
        )?;
        self.0.execute("DELETE FROM word_history WHERE seq NOT IN (SELECT seq FROM word_history ORDER BY seq DESC LIMIT 100)", [])?;
        Ok(())
    }
    pub fn history(&self) -> Result<Vec<String>> {
        Ok(self
            .0
            .prepare("SELECT term FROM word_history ORDER BY seq DESC LIMIT 100")?
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn persists_results_and_deduplicates_recent_words() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("words.sqlite3");
        let rows = vec![(
            "A small light".into(),
            "noun".into(),
            "https://en.wiktionary.org/wiki/glimmer".into(),
            serde_json::json!({"examples":["A glimmer of hope"]}),
        )];
        {
            let db = WordCache::open(Some(&path)).unwrap();
            db.put("define:glimmer:wiki:English", &rows).unwrap();
            db.remember("glimmer").unwrap();
            db.remember("monster").unwrap();
            db.remember("glimmer").unwrap();
        }
        let db = WordCache::open(Some(&path)).unwrap();
        assert_eq!(db.get("define:glimmer:wiki:English").unwrap(), Some(rows));
        assert!(db.get("synonym:glimmer:wiki:English").unwrap().is_none());
        assert!(db.get("define:glimmer:wiki:French").unwrap().is_none());
        assert_eq!(db.history().unwrap(), ["glimmer", "monster"]);
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    #[test]
    fn imports_legacy_lookups_once_without_reordering_new_history() {
        let dir = tempfile::tempdir().unwrap();
        let old_path = dir.path().join("history.sqlite3");
        let old = Connection::open(&old_path).unwrap();
        old.execute_batch("CREATE TABLE command_queries(seq INTEGER PRIMARY KEY,query TEXT); INSERT INTO command_queries VALUES(1,'/define glimmer'),(2,'/synonym monster'),(3,'/define '),(4,'/run private-command');").unwrap();
        let path = dir.path().join("words.sqlite3");
        let existing = Connection::open(&path).unwrap();
        existing.execute_batch("CREATE TABLE word_history(seq INTEGER PRIMARY KEY AUTOINCREMENT, term TEXT NOT NULL UNIQUE); INSERT INTO word_history(term) VALUES('newest');").unwrap();
        drop(existing);
        let cache = WordCache::open(Some(&path)).unwrap();
        assert_eq!(cache.history().unwrap(), ["newest", "monster", "glimmer"]);
        drop(cache);
        old.execute("INSERT INTO command_queries VALUES(5,'/define later')", [])
            .unwrap();
        assert_eq!(
            WordCache::open(Some(&path)).unwrap().history().unwrap(),
            ["newest", "monster", "glimmer"]
        );
    }
    #[test]
    fn bounds_history_and_cache() {
        let db = WordCache::open(None).unwrap();
        for n in 0..270 {
            db.remember(&format!("word-{n}")).unwrap();
            db.put(&n.to_string(), &vec![]).unwrap();
        }
        assert_eq!(db.history().unwrap().len(), 100);
        assert!(db.get("0").unwrap().is_none());
        assert_eq!(db.get("269").unwrap(), Some(vec![]));
    }
}
