use super::*;
use axum::{body::Body, http::Request};
use tower::ServiceExt;
fn fixture() -> (tempfile::TempDir, Router) {
    let dir = tempfile::tempdir().unwrap();
    let database = dir.path().join("notifications.sqlite3");
    store::connect(&database).unwrap();
    let app = Arc::new(App {
        database,
        cfg: Config {
            socket: dir.path().join("api.sock"),
            directory: dir.path().into(),
            origin: "https://si.example".into(),
            allowed_users: ["allowed@example.test".into()].into(),
            local_users: [("1000".into(), "allowed@example.test".into())].into(),
            firebase_credentials: None,
            android_config: None,
        },
    });
    (dir, router(app))
}
#[tokio::test]
async fn auth_trusts_only_protected_proxy_or_mapped_uid() {
    let (_dir, router) = fixture();
    for (uid, login, status) in [
        (0, Some("allowed@example.test"), 200),
        (0, Some("other@example.test"), 403),
        (0, None, 403),
        (1000, None, 200),
        (2000, Some("allowed@example.test"), 403),
    ] {
        let mut request = Request::builder().uri("/api/v1/health");
        if let Some(login) = login {
            request = request.header("tailscale-user-login", login);
        }
        let mut request = request.body(Body::empty()).unwrap();
        request.extensions_mut().insert(ConnectInfo(Peer(uid)));
        assert_eq!(
            router
                .clone()
                .oneshot(request)
                .await
                .unwrap()
                .status()
                .as_u16(),
            status
        );
    }
}
#[tokio::test]
async fn cross_origin_and_missing_intent_are_rejected() {
    let (_dir, router) = fixture();
    for (origin, intent, status) in [
        ("https://evil.example", true, 403),
        ("https://si.example", false, 403),
        ("https://si.example", true, 400),
    ] {
        let mut request = Request::builder()
            .method("POST")
            .uri("/api/v1/request")
            .header("content-type", "application/json")
            .header("origin", origin);
        if intent {
            request = request.header("x-notjik-request", "1");
        }
        let mut request = request.body(Body::from("{}")).unwrap();
        request.extensions_mut().insert(ConnectInfo(Peer(1000)));
        assert_eq!(
            router
                .clone()
                .oneshot(request)
                .await
                .unwrap()
                .status()
                .as_u16(),
            status
        );
    }
}
