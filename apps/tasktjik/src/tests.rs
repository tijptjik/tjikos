use super::*;
use axum::{body::Body, http::Request};
use http_body_util::BodyExt;
use tower::ServiceExt;
use uuid::Uuid;
fn app(tmp: &tempfile::TempDir) -> Arc<App> {
    let database = tmp.path().join("tasks.sqlite3");
    db::connect(&database).unwrap();
    Arc::new(App {
        cfg: Config {
            socket: tmp.path().join("api.sock"),
            directory: tmp.path().into(),
            origin: "https://si.example.ts.net".into(),
            allowed_users: HashSet::from(["owner@example.test".into()]),
            local_users: HashMap::new(),
        },
        database,
    })
}
fn mutation(action: &str) -> Value {
    json!({"action":action,"request_id":Uuid::new_v4().to_string(),"actor":"owner@example.test"})
}
fn create(path: &std::path::Path) -> Value {
    let mut r = mutation("create");
    r["fields"] = json!({"title":"Synthetic task","due":model::today().to_string()});
    db::request(path, r, "owner@example.test", "").unwrap()
}
async fn invoke(
    a: Arc<App>,
    uid: u32,
    login: Option<&str>,
    path: &str,
    body: Option<Value>,
    origin: Option<&str>,
    custom: bool,
) -> (StatusCode, Value) {
    let mut r = Request::builder()
        .uri(path)
        .method(if body.is_some() { "POST" } else { "GET" })
        .extension(ConnectInfo(Peer(uid)));
    if let Some(login) = login {
        r = r.header("tailscale-user-login", login);
    }
    if let Some(origin) = origin {
        r = r.header("origin", origin);
    }
    if custom {
        r = r
            .header("x-tasktjik-request", "1")
            .header("content-type", "application/json");
    }
    let response = router(a)
        .oneshot(
            r.body(
                body.map(|b| Body::from(b.to_string()))
                    .unwrap_or(Body::empty()),
            )
            .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}
#[tokio::test]
async fn only_root_proxy_and_explicit_identity_are_trusted() {
    let tmp = tempfile::tempdir().unwrap();
    let a = app(&tmp);
    for (uid, login) in [
        (1000, Some("owner@example.test")),
        (0, None),
        (0, Some("other@example.test")),
    ] {
        assert_eq!(
            invoke(a.clone(), uid, login, "/api/v1/tasks", None, None, false)
                .await
                .0,
            StatusCode::FORBIDDEN
        );
    }
    assert_eq!(
        invoke(
            a,
            0,
            Some("owner@example.test"),
            "/api/v1/health",
            None,
            None,
            false
        )
        .await
        .0,
        StatusCode::OK
    );
}
#[tokio::test]
async fn reject_cross_origin_and_simple_posts() {
    let tmp = tempfile::tempdir().unwrap();
    let a = app(&tmp);
    let r = mutation("create");
    assert_eq!(
        invoke(
            a.clone(),
            0,
            Some("owner@example.test"),
            "/api/v1/request",
            Some(r.clone()),
            Some("https://evil.example"),
            true
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        invoke(
            a,
            0,
            Some("owner@example.test"),
            "/api/v1/request",
            Some(r),
            None,
            false
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
}
#[tokio::test]
async fn attributed_create_and_idempotency() {
    let tmp = tempfile::tempdir().unwrap();
    let a = app(&tmp);
    let mut r = mutation("create");
    r["actor"] = json!("forged");
    r["fields"] = json!({"title":"Synthetic task"});
    let first = invoke(
        a.clone(),
        0,
        Some("owner@example.test"),
        "/tasktjik/api/v1/request",
        Some(r.clone()),
        None,
        true,
    )
    .await;
    assert_eq!(first.0, StatusCode::OK);
    assert_eq!(
        first.1["result"]["events"][0]["actor"],
        "owner@example.test"
    );
    assert_eq!(
        first,
        invoke(
            a.clone(),
            0,
            Some("owner@example.test"),
            "/api/v1/request",
            Some(r.clone()),
            None,
            true
        )
        .await
    );
    r["fields"]["title"] = json!("Different");
    assert_eq!(
        invoke(
            a,
            0,
            Some("owner@example.test"),
            "/api/v1/request",
            Some(r),
            None,
            true
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
}
#[test]
fn stale_edits_do_not_lose_data() {
    let tmp = tempfile::tempdir().unwrap();
    let a = app(&tmp);
    let t = create(&a.database);
    let mut edit = mutation("edit");
    edit["id"] = t["id"].clone();
    edit["revision"] = t["revision"].clone();
    edit["fields"] = json!({"title":"Updated"});
    db::request(&a.database, edit.clone(), "owner@example.test", "").unwrap();
    edit["request_id"] = json!(Uuid::new_v4().to_string());
    assert!(db::request(&a.database, edit, "owner@example.test", "")
        .unwrap_err()
        .to_string()
        .contains("changed"));
    assert_eq!(
        db::tasks(&db::connect(&a.database).unwrap()).unwrap()[0]["title"],
        "Updated"
    );
}
#[test]
fn concurrent_claims_only_one_wins_and_delivery_requires_ownership() {
    let tmp = tempfile::tempdir().unwrap();
    let a = app(&tmp);
    let t = create(&a.database);
    let device = Uuid::new_v4().to_string();
    db::devices(
        &a.database,
        "owner@example.test",
        Some(json!({"id":device,"name":"Fixture","notifications":true})),
    )
    .unwrap();
    let jobs: Vec<_> = (0..16)
        .map(|_| {
            let p = a.database.clone();
            let id = t["id"].clone();
            let device = device.clone();
            std::thread::spawn(move || {
                let mut r = mutation("claim");
                r["id"] = id;
                r["device"] = json!(device);
                db::request(&p, r, "owner@example.test", &device).unwrap()
            })
        })
        .collect();
    let replies: Vec<_> = jobs.into_iter().map(|j| j.join().unwrap()).collect();
    assert_eq!(replies.iter().filter(|r| r.is_object()).count(), 1);
    let winner = replies.iter().find(|r| r.is_object()).unwrap();
    let (key, claim) = winner["notifications"]
        .as_object()
        .unwrap()
        .iter()
        .next()
        .unwrap();
    let mut ack = mutation("ack");
    ack["id"] = t["id"].clone();
    ack["notification_key"] = json!(key);
    ack["token"] = claim["token"].clone();
    ack["device"] = json!("wrong");
    assert!(db::request(&a.database, ack.clone(), "owner@example.test", &device).is_err());
    ack["device"] = json!(device);
    let r = db::request(&a.database, ack, "owner@example.test", &device).unwrap();
    assert_eq!(r["notifications"][key]["state"], "delivered");
}
#[test]
fn expired_claim_can_be_reclaimed_and_disabled_device_cannot_claim() {
    let tmp = tempfile::tempdir().unwrap();
    let a = app(&tmp);
    let mut t = create(&a.database);
    let device = Uuid::new_v4().to_string();
    let key = format!("{}:{}", model::today(), model::today());
    t["notifications"][&key] =
        json!({"state":"pending","expires":"2000-01-01T00:00:00Z","token":"old","device":"old"});
    db::connect(&a.database)
        .unwrap()
        .execute(
            "UPDATE tasks SET body=? WHERE id=?",
            rusqlite::params![t.to_string(), t["id"].as_str()],
        )
        .unwrap();
    let mut r = mutation("claim");
    r["id"] = t["id"].clone();
    r["device"] = json!(device);
    assert!(db::request(&a.database, r.clone(), "owner@example.test", &device).is_err());
    db::devices(
        &a.database,
        "owner@example.test",
        Some(json!({"id":device,"name":"Fixture","notifications":true})),
    )
    .unwrap();
    assert!(db::request(&a.database, r, "owner@example.test", &device)
        .unwrap()
        .is_object());
}
#[test]
fn recurrence_boundaries_and_verified_snapshot() {
    let t = json!({"due":"2024-01-31","repeat":{"unit":"months","value":1}});
    assert_eq!(
        model::period(&t, chrono::NaiveDate::from_ymd_opt(2024, 2, 29).unwrap()).unwrap(),
        "2024-02-29"
    );
    assert_eq!(
        model::period(&t, chrono::NaiveDate::from_ymd_opt(2024, 3, 30).unwrap()).unwrap(),
        "2024-02-29"
    );
    let tmp = tempfile::tempdir().unwrap();
    let a = app(&tmp);
    create(&a.database);
    let snapshot = tmp.path().join("snapshot.sqlite3");
    db::snapshot(&a.database, &snapshot).unwrap();
    assert_eq!(
        db::tasks(&db::connect(&snapshot).unwrap()).unwrap().len(),
        1
    );
    assert!(db::snapshot(&a.database, &snapshot).is_err());
}
#[test]
fn occurrence_completion_history_is_preserved() {
    let tmp = tempfile::tempdir().unwrap();
    let a = app(&tmp);
    let t = create(&a.database);
    for p in ["2026-10-28", "2026-10-29"] {
        let mut r = mutation("done");
        r["id"] = t["id"].clone();
        r["period"] = json!(p);
        db::request(&a.database, r, "owner@example.test", "").unwrap();
    }
    let t = db::tasks(&db::connect(&a.database).unwrap())
        .unwrap()
        .remove(0);
    assert_eq!(t["completions"].as_object().unwrap().len(), 2);
}

#[test]
fn import_failure_is_atomic_and_snapshot_requires_existing_source() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("tasks.sqlite3");
    let task = create(&path);
    let source = tmp.path().join("import");
    std::fs::create_dir(&source).unwrap();
    std::fs::write(source.join("good.json"), task.to_string()).unwrap();
    std::fs::write(source.join("bad.json"), "42").unwrap();
    let target = tmp.path().join("imported.sqlite3");
    assert!(db::import(&target, &source).is_err());
    assert!(db::tasks(&db::connect(&target).unwrap())
        .unwrap()
        .is_empty());
    assert!(db::snapshot(&tmp.path().join("missing"), &tmp.path().join("snapshot")).is_err());
}

#[tokio::test]
async fn local_client_identity_comes_from_peer_credentials() {
    let tmp = tempfile::tempdir().unwrap();
    let original = app(&tmp);
    let mut cfg = original.cfg.clone();
    cfg.local_users
        .insert("1000".into(), "owner@example.test".into());
    let a = Arc::new(App {
        cfg,
        database: original.database.clone(),
    });
    let (status, identity) = invoke(
        a.clone(),
        1000,
        Some("forged@example.test"),
        "/api/v1/identity",
        None,
        None,
        false,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(identity["login"], "owner@example.test");
    assert_eq!(
        invoke(
            a,
            1001,
            Some("owner@example.test"),
            "/api/v1/identity",
            None,
            None,
            false
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
}
