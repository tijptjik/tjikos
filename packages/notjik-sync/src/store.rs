//! The sole authority for routing. Every transition is serialized in SQLite.
use crate::*;
use anyhow::{bail, Context};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde_json::json;
use std::path::Path;
pub fn connect(path: &Path) -> Result<Connection> {
    let c = Connection::open(path)?;
    c.busy_timeout(std::time::Duration::from_secs(10))?;
    c.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;
    CREATE TABLE IF NOT EXISTS devices(id TEXT PRIMARY KEY, owner TEXT NOT NULL, name TEXT NOT NULL, kind TEXT NOT NULL, secret TEXT NOT NULL, seen INTEGER DEFAULT 0, active INTEGER DEFAULT 0, paused INTEGER DEFAULT 0, ready INTEGER DEFAULT 0, push TEXT DEFAULT '', UNIQUE(owner,name));
    CREATE TABLE IF NOT EXISTS notices(id TEXT PRIMARY KEY, owner TEXT NOT NULL, source TEXT NOT NULL, app TEXT NOT NULL, run TEXT NOT NULL, body TEXT NOT NULL, history INTEGER NOT NULL, delivery TEXT NOT NULL, created INTEGER NOT NULL, updated INTEGER NOT NULL, revision INTEGER NOT NULL DEFAULT 1, dismissed INTEGER DEFAULT 0, is_read INTEGER DEFAULT 0, target TEXT DEFAULT '', lease INTEGER DEFAULT 0, delivered INTEGER DEFAULT 0, pushed INTEGER DEFAULT 0);
    CREATE INDEX IF NOT EXISTS notice_history ON notices(owner,updated);
    CREATE TABLE IF NOT EXISTS requests(id TEXT PRIMARY KEY, owner TEXT NOT NULL, hash TEXT NOT NULL, result TEXT NOT NULL);
    CREATE TABLE IF NOT EXISTS jobs(id TEXT PRIMARY KEY, owner TEXT NOT NULL, notification TEXT NOT NULL, action TEXT NOT NULL, target TEXT NOT NULL, state TEXT NOT NULL, result TEXT DEFAULT '', created INTEGER NOT NULL);")?;
    let columns = {
        let mut s = c.prepare("PRAGMA table_info(notices)")?;
        let values = s
            .query_map([], |r| r.get::<_, String>(1))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        values
    };
    if !columns.iter().any(|v| v == "push_attempts") {
        c.execute(
            "ALTER TABLE notices ADD COLUMN push_attempts INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
    }
    let columns = {
        let mut s = c.prepare("PRAGMA table_info(jobs)")?;
        let values = s
            .query_map([], |r| r.get::<_, String>(1))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        values
    };
    if !columns.iter().any(|v| v == "claimed_at") {
        c.execute(
            "ALTER TABLE jobs ADD COLUMN claimed_at INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
    }
    c.execute_batch(
        "CREATE INDEX IF NOT EXISTS jobs_notification ON jobs(owner,notification,created);",
    )?;
    Ok(c)
}
fn candidate(c: &Connection, owner: &str, time: i64) -> Result<Option<String>> {
    Ok(c.query_row("SELECT id FROM devices WHERE owner=?1 AND paused=0 AND ((seen>?2 AND ready=1 AND active>?3 AND kind IN ('desktop','android')) OR (kind='android' AND push!='')) ORDER BY CASE WHEN seen>?2 AND ready=1 AND active>?3 THEN 0 ELSE 1 END, active DESC, name ASC LIMIT 1",params![owner,time-35,time-120],|r|r.get(0)).optional()?)
}
pub fn route(c: &Connection, owner: &str, time: i64) -> Result<()> {
    if let Some(target) = candidate(c, owner, time)? {
        c.execute("UPDATE notices SET pushed=CASE WHEN target=?1 THEN pushed ELSE 0 END,target=?1,lease=?2 WHERE owner=?3 AND delivery='active' AND dismissed=0 AND delivered=0 AND (target='' OR lease<?4)",params![target,time+45,owner,time])?;
    }
    c.execute(
        "UPDATE jobs SET state='unknown' WHERE state='started' AND claimed_at<?1",
        [time - 600],
    )?;
    // Non-retained envelopes are transport state only, purged after settlement.
    c.execute(
        "DELETE FROM notices WHERE history=0 AND updated<?1 AND (dismissed=1 OR delivered=1)",
        [time - 86400],
    )?;
    Ok(())
}
pub fn request(path: &Path, owner: &str, device: &str, secret: &str, v: Value) -> Result<Value> {
    let mut c = connect(path)?;
    let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let result = apply(&tx, owner, device, secret, &v, now())?;
    tx.commit()?;
    Ok(result)
}
fn apply(
    c: &Connection,
    owner: &str,
    device: &str,
    secret: &str,
    v: &Value,
    time: i64,
) -> Result<Value> {
    let op = text(v, "op");
    if op == "enroll" {
        ensure!(
            uuid::Uuid::parse_str(device).is_ok() && secret.len() >= 32 && secret.len() <= 128,
            "Invalid device enrollment"
        );
        let name = text(v, "name");
        let kind = text(v, "kind");
        ensure!(
            !name.is_empty()
                && name.len() <= 63
                && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'),
            "Invalid device name"
        );
        ensure!(
            ["desktop", "android", "server"].contains(&kind),
            "Invalid device kind"
        );
        c.execute(
            "INSERT OR IGNORE INTO devices(id,owner,name,kind,secret) VALUES(?1,?2,?3,?4,?5)",
            params![device, owner, name, kind, hash(secret)],
        )?;
    }
    let authorized: bool = c.query_row(
        "SELECT EXISTS(SELECT 1 FROM devices WHERE id=?1 AND owner=?2 AND secret=?3)",
        params![device, owner, hash(secret)],
        |r| r.get(0),
    )?;
    ensure!(
        authorized,
        "Device authentication failed; enroll this device"
    );
    if op == "enroll" {
        return Ok(json!({"enrolled":true}));
    }
    let request_id = text(v, "request_id");
    let mutation = ["publish", "dismiss", "read", "invoke", "job-finish"].contains(&op);
    let digest = hash(&format!("{device}:{v}"));
    if mutation {
        ensure!(
            uuid::Uuid::parse_str(request_id).is_ok(),
            "UUID request_id required"
        );
        if let Some((who, h, result)) = c
            .query_row(
                "SELECT owner,hash,result FROM requests WHERE id=?1",
                [request_id],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()?
        {
            ensure!(
                who == owner && h == digest,
                "Request ID reused with different payload"
            );
            return Ok(serde_json::from_str(&result)?);
        }
    }
    let result = match op {
        "presence" => {
            let active = v["active"].as_bool().unwrap_or(false);
            // A client supplies idle age, never a wall-clock timestamp.
            let age = v["idle_seconds"].as_i64().unwrap_or(120).clamp(0, 86400);
            let ready = v["ready"] == true;
            let paused = v["paused"] == true;
            c.execute("UPDATE devices SET seen=?1,active=CASE WHEN ?2 THEN ?3 ELSE active END,ready=?4,paused=?5 WHERE id=?6",params![time,active&&ready&&!paused,time-age,ready,paused,device])?;
            if let Some(token) = v["push_token"].as_str() {
                ensure!(token.len() < 4096, "Invalid push token");
                c.execute(
                    "UPDATE devices SET push=?1 WHERE id=?2 AND kind='android'",
                    params![token, device],
                )?;
            }
            route(c, owner, time)?;
            json!({"live":candidate(c,owner,time)?})
        }
        "publish" => {
            let n = &v["notice"];
            validate(n)?;
            let existing = c
                .query_row(
                    "SELECT source,owner FROM notices WHERE id=?1",
                    [text(n, "id")],
                    |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
                )
                .optional()?;
            if let Some((source, who)) = existing {
                ensure!(
                    source == device && who == owner,
                    "Notification belongs to another device"
                );
            }
            let shown = n["shown"] == true || n["delivery"] == "local";
            c.execute("INSERT INTO notices(id,owner,source,app,run,body,history,delivery,created,updated,delivered) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?9,?10) ON CONFLICT(id) DO UPDATE SET body=excluded.body,history=excluded.history,delivery=excluded.delivery,updated=excluded.updated,revision=notices.revision+1,delivered=excluded.delivered,target='',lease=0,pushed=0,push_attempts=0",params![text(n,"id"),owner,device,text(n,"app"),text(n,"run"),n.to_string(),n["history"]==true,text(n,"delivery"),time,shown])?;
            route(c, owner, time)?;
            json!({"id":n["id"],"stored":true})
        }
        "poll" => {
            route(c, owner, time)?;
            let mut stmt=c.prepare("SELECT body,revision FROM notices WHERE owner=?1 AND target=?2 AND dismissed=0 AND delivered=0 AND ?3 ORDER BY created LIMIT 50")?;
            let notices = stmt
                .query_map(
                    params![
                        owner,
                        device,
                        candidate(c, owner, time)?.as_deref() == Some(device)
                    ],
                    |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)),
                )?
                .collect::<rusqlite::Result<Vec<_>>>()?
                .into_iter()
                .map(|(s, r)| {
                    let mut v: Value = serde_json::from_str(&s).unwrap();
                    v["revision"] = json!(r);
                    v
                })
                .collect::<Vec<_>>();
            let mut stmt=c.prepare("SELECT id,notification,action FROM jobs WHERE owner=?1 AND target=?2 AND state='pending' ORDER BY created LIMIT 10")?;
            let jobs=stmt.query_map(params![owner,device],|r|Ok(json!({"id":r.get::<_,String>(0)?,"notification":r.get::<_,String>(1)?,"action":r.get::<_,String>(2)?})))?.collect::<rusqlite::Result<Vec<_>>>()?;
            let mut dismissals = Vec::new();
            if let Some(ids) = v["visible"].as_array() {
                ensure!(ids.len() <= 1000, "Too many visible IDs");
                for id in ids {
                    if c.query_row(
                        "SELECT dismissed FROM notices WHERE owner=?1 AND id=?2",
                        params![owner, id.as_str().unwrap_or("")],
                        |r| r.get::<_, bool>(0),
                    )
                    .optional()?
                    .unwrap_or(false)
                    {
                        dismissals.push(id.clone());
                    }
                }
            }
            json!({"notices":notices,"jobs":jobs,"dismissed":dismissals,"live":candidate(c,owner,time)?})
        }
        "ack" => {
            c.execute("UPDATE notices SET delivered=1 WHERE owner=?1 AND id=?2 AND target=?3 AND revision=?4",params![owner,text(v,"id"),device,v["revision"].as_i64().unwrap_or(0)])?;
            json!({"ok":true})
        }
        "dismiss" | "read" => {
            let sql = if op == "dismiss" {
                "UPDATE notices SET dismissed=1,is_read=1 WHERE owner=?1 AND id=?2"
            } else {
                "UPDATE notices SET is_read=1 WHERE owner=?1 AND id=?2"
            };
            ensure!(
                c.execute(sql, params![owner, text(v, "id")])? == 1,
                "Unknown notification"
            );
            json!({"ok":true})
        }
        "history" => {
            let offset = v["offset"].as_i64().unwrap_or(0).max(0);
            let q = text(v, "query");
            ensure!(q.len() <= 256, "Search too long");
            let mut stmt=c.prepare("SELECT n.body,n.revision,n.dismissed,n.is_read,n.created,n.updated,d.name FROM notices n JOIN devices d ON n.source=d.id WHERE n.owner=?1 AND n.history=1 AND (?2='' OR instr(lower(n.body||' '||d.name),lower(?2))>0) ORDER BY n.updated DESC,n.id LIMIT 100 OFFSET ?3")?;
            let rows = stmt
                .query_map(params![owner, q, offset], |r| {
                    let mut n: Value = serde_json::from_str(&r.get::<_, String>(0)?).unwrap();
                    n["revision"] = json!(r.get::<_, i64>(1)?);
                    n["dismissed"] = json!(r.get::<_, bool>(2)?);
                    n["read"] = json!(r.get::<_, bool>(3)?);
                    n["created"] = json!(r.get::<_, i64>(4)?);
                    n["updated"] = json!(r.get::<_, i64>(5)?);
                    n["device"] = json!(r.get::<_, String>(6)?);
                    Ok(n)
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let mut rows = rows;
            for n in &mut rows {
                let mut s=c.prepare("SELECT action,state FROM jobs WHERE owner=?1 AND notification=?2 ORDER BY created DESC LIMIT 16")?;
                let jobs = s
                    .query_map(params![owner, text(n, "id")], |r| {
                        Ok(json!({"action":r.get::<_,String>(0)?,"state":r.get::<_,String>(1)?}))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                n["actionJobs"] = json!(jobs);
            }
            json!({"notices":rows,"next":if rows.len()==100 {Some(offset+100)}else{None}})
        }
        "invoke" => {
            let (body, target): (String, String) = c
                .query_row(
                    "SELECT body,source FROM notices WHERE owner=?1 AND id=?2",
                    params![owner, text(v, "id")],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .context("Unknown notification")?;
            let n: Value = serde_json::from_str(&body)?;
            ensure!(
                n["actions"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|a| a["id"] == v["action"] && a["url"].is_null()),
                "Unknown action"
            );
            let existing=c.query_row("SELECT id FROM jobs WHERE owner=?1 AND notification=?2 AND action=?3 AND state IN ('pending','started') ORDER BY created DESC LIMIT 1",params![owner,text(v,"id"),text(v,"action")],|r|r.get::<_,String>(0)).optional()?;
            let job = existing.clone().unwrap_or_else(id);
            if existing.is_none() {
                c.execute("INSERT INTO jobs(id,owner,notification,action,target,state,created) VALUES(?1,?2,?3,?4,?5,'pending',?6)",params![job,owner,text(v,"id"),text(v,"action"),target,time])?;
            }
            json!({"job":job,"state":"pending"})
        }
        "job-claim" => {
            let claimed=c.execute("UPDATE jobs SET state='started',claimed_at=?4 WHERE id=?1 AND owner=?2 AND target=?3 AND state='pending'",params![text(v,"id"),owner,device,time])?==1;
            json!({"claimed":claimed})
        }
        "job-finish" => {
            ensure!(
                ["succeeded", "failed", "unknown"].contains(&text(v, "state")),
                "Invalid job outcome"
            );
            c.execute("UPDATE jobs SET state=?1,result=?2 WHERE id=?3 AND owner=?4 AND target=?5 AND state IN ('started','unknown')",params![text(v,"state"),v["exit_code"].to_string(),text(v,"id"),owner,device])?;
            json!({"ok":true})
        }
        "devices" => {
            let mut s=c.prepare("SELECT id,name,kind,seen,active,paused,ready FROM devices WHERE owner=?1 ORDER BY name")?;
            let devices=s.query_map([owner],|r|Ok(json!({"id":r.get::<_,String>(0)?,"name":r.get::<_,String>(1)?,"kind":r.get::<_,String>(2)?,"seen":r.get::<_,i64>(3)?,"active":r.get::<_,i64>(4)?,"paused":r.get::<_,bool>(5)?,"ready":r.get::<_,bool>(6)?})))?.collect::<rusqlite::Result<Vec<_>>>()?;
            json!({"devices":devices,"live":candidate(c,owner,time)?})
        }
        "jobs" => {
            let mut s=c.prepare("SELECT id,notification,action,state,result FROM jobs WHERE owner=?1 ORDER BY created DESC LIMIT 100")?;
            let jobs=s.query_map([owner],|r|Ok(json!({"id":r.get::<_,String>(0)?,"notification":r.get::<_,String>(1)?,"action":r.get::<_,String>(2)?,"state":r.get::<_,String>(3)?,"result":r.get::<_,String>(4)?})))?.collect::<rusqlite::Result<Vec<_>>>()?;
            json!({"jobs":jobs})
        }
        _ => bail!("Unknown operation"),
    };
    if mutation {
        c.execute(
            "INSERT INTO requests VALUES(?1,?2,?3,?4)",
            params![request_id, owner, digest, result.to_string()],
        )?;
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    const KEY: &str = "test-device-credential-only-000000000000";
    struct Fixture {
        _dir: tempfile::TempDir,
        c: Connection,
        fi: String,
        pi: String,
        si: String,
    }
    impl Fixture {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let c = connect(&dir.path().join("test.sqlite")).unwrap();
            let f = Self {
                _dir: dir,
                c,
                fi: id(),
                pi: id(),
                si: id(),
            };
            for (device, name, kind) in [
                (&f.fi, "fi", "desktop"),
                (&f.pi, "pi", "android"),
                (&f.si, "si", "server"),
            ] {
                apply(
                    &f.c,
                    "owner",
                    device,
                    KEY,
                    &json!({"op":"enroll","name":name,"kind":kind}),
                    1000,
                )
                .unwrap();
            }
            f
        }
        fn req(&self, d: &str, mut v: Value, t: i64) -> Value {
            if v["request_id"].is_null() {
                v["request_id"] = json!(id());
            }
            apply(&self.c, "owner", d, KEY, &v, t).unwrap()
        }
        fn notice(&self) -> Value {
            json!({"id":id(),"app":"tests","run":"example","title":"Test result","body":"Synthetic only","history":true,"delivery":"active","actions":[]})
        }
        fn presence(&self, d: &str, t: i64, active: bool, paused: bool) {
            self.req(d,json!({"op":"presence","ready":true,"active":active,"paused":paused,"idle_seconds":0,"push_token":if d==self.pi{"synthetic-token"}else{""}}),t);
        }
    }
    #[test]
    fn routes_to_recent_desktop_then_phone() {
        let f = Fixture::new();
        f.presence(&f.pi, 1000, false, false);
        f.presence(&f.fi, 1000, true, false);
        assert_eq!(candidate(&f.c, "owner", 1001).unwrap(), Some(f.fi.clone()));
        assert_eq!(candidate(&f.c, "owner", 1040).unwrap(), Some(f.pi));
    }
    #[test]
    fn phone_foreground_can_win_but_background_does_not() {
        let f = Fixture::new();
        f.presence(&f.fi, 1000, true, false);
        f.presence(&f.pi, 1001, false, false);
        assert_eq!(candidate(&f.c, "owner", 1001).unwrap(), Some(f.fi.clone()));
        f.presence(&f.pi, 1002, true, false);
        assert_eq!(candidate(&f.c, "owner", 1002).unwrap(), Some(f.pi));
    }
    #[test]
    fn idle_and_dnd_hold_delivery() {
        let f = Fixture::new();
        f.presence(&f.fi, 1000, false, false);
        assert!(candidate(&f.c, "owner", 1000).unwrap().is_none());
        f.presence(&f.fi, 1001, true, true);
        assert!(candidate(&f.c, "owner", 1001).unwrap().is_none());
        let n = f.notice();
        f.req(&f.si, json!({"op":"publish","notice":n}), 1001);
        assert_eq!(
            f.req(&f.fi, json!({"op":"poll"}), 1002)["notices"],
            json!([])
        );
    }
    #[test]
    fn local_history_never_delivers() {
        let f = Fixture::new();
        f.presence(&f.fi, 1000, true, false);
        let mut n = f.notice();
        n["delivery"] = json!("local");
        f.req(&f.si, json!({"op":"publish","notice":n}), 1000);
        assert_eq!(
            f.req(&f.fi, json!({"op":"poll"}), 1000)["notices"],
            json!([])
        );
        assert_eq!(
            f.req(&f.fi, json!({"op":"history"}), 1000)["notices"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }
    #[test]
    fn nonretained_delivers_but_is_not_in_history() {
        let f = Fixture::new();
        f.presence(&f.fi, 1000, true, false);
        let mut n = f.notice();
        n["history"] = json!(false);
        f.req(&f.si, json!({"op":"publish","notice":n}), 1000);
        assert_eq!(
            f.req(&f.fi, json!({"op":"poll"}), 1000)["notices"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            f.req(&f.fi, json!({"op":"history"}), 1000)["notices"],
            json!([])
        );
    }
    #[test]
    fn replay_is_idempotent_and_conflicting_request_rejected() {
        let f = Fixture::new();
        let mut r = json!({"op":"publish","request_id":id(),"notice":f.notice()});
        f.req(&f.si, r.clone(), 1000);
        f.req(&f.si, r.clone(), 1001);
        let n = f.req(&f.fi, json!({"op":"history"}), 1002);
        assert_eq!(n["notices"][0]["revision"], 1);
        r["notice"]["title"] = json!("changed");
        assert!(apply(&f.c, "owner", &f.si, KEY, &r, 1003).is_err());
    }
    #[test]
    fn shown_offline_is_history_only() {
        let f = Fixture::new();
        f.presence(&f.fi, 1000, true, false);
        let mut n = f.notice();
        n["shown"] = json!(true);
        f.req(&f.si, json!({"op":"publish","notice":n}), 1000);
        assert_eq!(
            f.req(&f.fi, json!({"op":"poll"}), 1000)["notices"],
            json!([])
        );
    }
    #[test]
    fn read_and_dismiss_are_distinct() {
        let f = Fixture::new();
        let n = f.notice();
        f.req(&f.si, json!({"op":"publish","notice":n}), 1000);
        f.req(&f.pi, json!({"op":"read","id":n["id"]}), 1001);
        let h = f.req(&f.fi, json!({"op":"history"}), 1001);
        assert_eq!(h["notices"][0]["read"], true);
        assert_eq!(h["notices"][0]["dismissed"], false);
        f.req(&f.pi, json!({"op":"dismiss","id":n["id"]}), 1002);
        assert_eq!(
            f.req(&f.fi, json!({"op":"poll","visible":[n["id"]]}), 1003)["dismissed"],
            json!([n["id"]])
        );
    }
    #[test]
    fn another_device_cannot_overwrite_or_ack() {
        let f = Fixture::new();
        f.presence(&f.fi, 1000, true, false);
        let n = f.notice();
        f.req(&f.si, json!({"op":"publish","notice":n}), 1000);
        assert!(apply(
            &f.c,
            "owner",
            &f.pi,
            KEY,
            &json!({"op":"publish","request_id":id(),"notice":n}),
            1001
        )
        .is_err());
        f.req(&f.pi, json!({"op":"ack","id":n["id"],"revision":1}), 1001);
        assert_eq!(
            f.req(&f.fi, json!({"op":"poll"}), 1001)["notices"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }
    #[test]
    fn action_claim_is_once_and_target_only() {
        let f = Fixture::new();
        let mut n = f.notice();
        let action = id();
        n["actions"] = json!([{"id":action,"label":"Synthetic action"}]);
        f.req(&f.si, json!({"op":"publish","notice":n}), 1000);
        let j = f.req(
            &f.pi,
            json!({"op":"invoke","id":n["id"],"action":action}),
            1001,
        );
        let r = json!({"op":"job-claim","id":j["job"]});
        assert_eq!(f.req(&f.fi, r.clone(), 1002)["claimed"], false);
        assert_eq!(f.req(&f.si, r.clone(), 1002)["claimed"], true);
        assert_eq!(f.req(&f.si, r, 1003)["claimed"], false);
    }
    #[test]
    fn commands_and_unsafe_links_never_cross_protocol() {
        let f = Fixture::new();
        for action in [
            json!({"label":"Bad","command":["sh","-c","echo bad"]}),
            json!({"label":"Bad","url":"file:///etc/passwd"}),
            json!({"label":"Bad","url":"javascript:alert(1)"}),
        ] {
            let mut n = f.notice();
            n["actions"] = json!([action]);
            assert!(validate(&n).is_err());
        }
    }
    #[test]
    fn device_key_and_owner_are_required() {
        let f = Fixture::new();
        assert!(apply(
            &f.c,
            "owner",
            &f.fi,
            "wrong",
            &json!({"op":"history"}),
            1000
        )
        .is_err());
        assert!(apply(&f.c, "other", &f.fi, KEY, &json!({"op":"history"}), 1000).is_err());
    }
}
