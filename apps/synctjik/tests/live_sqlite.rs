use rusqlite::Connection;
use serde_json::json;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};
use synctjik::{fs as tree, util::hash};

#[test]
fn fingerprint_and_stage_capture_live_sqlite_commits() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("source");
    std::fs::create_dir(&source).unwrap();
    let db = source.join("store.db");
    let connection = Connection::open(&db).unwrap();
    connection.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE pairs(a INTEGER, b INTEGER); INSERT INTO pairs VALUES(0,0);").unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let commits = Arc::new(AtomicUsize::new(0));
    let stop_writer = stop.clone();
    let count = commits.clone();
    let writer = std::thread::spawn(move || {
        connection
            .busy_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        while !stop_writer.load(Ordering::Relaxed) {
            connection
                .execute_batch("UPDATE pairs SET a=a+1,b=b+1; PRAGMA wal_checkpoint(PASSIVE);")
                .unwrap();
            count.fetch_add(1, Ordering::Relaxed);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    });
    while commits.load(Ordering::Relaxed) == 0 {
        std::thread::yield_now();
    }
    let result = (|| -> anyhow::Result<()> {
        let policy = json!({"sqlite":["store.db"]});
        for i in 0..5 {
            let scratch = tmp.path().join(format!("scratch-{i}"));
            std::fs::create_dir(&scratch)?;
            tree::fingerprint(&source, &policy, &tmp.path().join("cache.json"), &scratch)?;
            let destination = tmp.path().join(format!("staged-{i}"));
            let (entries, dbs) = tree::stage(&source, &destination, &policy)?;
            assert_eq!(dbs, ["store.db"]);
            assert_eq!(
                entries["store.db"]["sha256"],
                hash(&destination.join("store.db"))?
            );
            let restored = Connection::open(destination.join("store.db"))?;
            let (a, b): (i64, i64) =
                restored.query_row("SELECT a,b FROM pairs", [], |r| Ok((r.get(0)?, r.get(1)?)))?;
            assert_eq!(a, b);
            assert!(a > 0);
            let check: String = restored.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
            assert_eq!(check, "ok");
        }
        Ok(())
    })();
    stop.store(true, Ordering::Relaxed);
    writer.join().unwrap();
    result.unwrap();
}
