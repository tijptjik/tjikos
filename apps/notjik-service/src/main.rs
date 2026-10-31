mod push;
use anyhow::{Context, Result};
use axum::{
    extract::{ConnectInfo, DefaultBodyLimit, Request, State},
    http::{header, HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Extension, Json, Router,
};
use notjik_sync::store;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    os::unix::fs::{FileTypeExt, PermissionsExt},
    path::PathBuf,
    sync::Arc,
    time::Duration,
};
use tokio::net::UnixListener;
use tower::limit::ConcurrencyLimitLayer;

#[derive(Clone, Deserialize)]
struct Config {
    socket: PathBuf,
    directory: PathBuf,
    origin: String,
    allowed_users: HashSet<String>,
    #[serde(default)]
    local_users: HashMap<String, String>,
    #[serde(default)]
    firebase_credentials: Option<PathBuf>,
    #[serde(default)]
    android_config: Option<PathBuf>,
}
#[derive(Deserialize)]
struct FileConfig {
    server: Config,
}
#[derive(Clone)]
struct App {
    cfg: Config,
    database: PathBuf,
}
#[derive(Clone)]
struct Identity(String);
#[derive(Clone)]
struct Peer(u32);
impl axum::extract::connect_info::Connected<axum::serve::IncomingStream<'_, UnixListener>>
    for Peer
{
    fn connect_info(stream: axum::serve::IncomingStream<'_, UnixListener>) -> Self {
        Self(stream.io().peer_cred().map(|c| c.uid()).unwrap_or(u32::MAX))
    }
}
struct Failure(anyhow::Error);
impl IntoResponse for Failure {
    fn into_response(self) -> Response {
        let message = self.0.to_string();
        let status = if message.contains("changed") || message.contains("Request ID") {
            StatusCode::CONFLICT
        } else if message.contains("Enroll this device") || message.contains("another identity") {
            StatusCode::FORBIDDEN
        } else if self.0.downcast_ref::<rusqlite::Error>().is_some() {
            StatusCode::SERVICE_UNAVAILABLE
        } else {
            StatusCode::BAD_REQUEST
        };
        let message = if status == StatusCode::SERVICE_UNAVAILABLE {
            "Authority storage unavailable".to_owned()
        } else {
            message
        };
        (status, Json(json!({"error":message}))).into_response()
    }
}
impl From<anyhow::Error> for Failure {
    fn from(e: anyhow::Error) -> Self {
        Self(e)
    }
}
async fn auth(State(app): State<Arc<App>>, mut request: Request, next: Next) -> Response {
    let peer = request
        .extensions()
        .get::<ConnectInfo<Peer>>()
        .map(|p| p.0 .0);
    let headers = request.headers();
    let login = if let Some(login) = peer
        .filter(|uid| *uid != 0)
        .and_then(|uid| app.cfg.local_users.get(&uid.to_string()))
    {
        Some(login.clone())
    } else if peer == Some(0)
        && !headers.contains_key("tailscale-user-login")
        && app.cfg.local_users.contains_key("0")
    {
        app.cfg.local_users.get("0").cloned()
    } else if peer == Some(0) && headers.get_all("tailscale-user-login").iter().count() == 1 {
        headers
            .get("tailscale-user-login")
            .and_then(|s| s.to_str().ok())
            .and_then(|s| percent_encoding::percent_decode_str(s).decode_utf8().ok())
            .map(|s| s.into_owned())
    } else {
        None
    };
    let approved = login
        .as_ref()
        .is_some_and(|s| app.cfg.allowed_users.contains(s));
    if !approved {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({"error":"This Tailscale identity is not allowed to access Notjik"})),
        )
            .into_response();
    }
    if headers
        .get(header::ORIGIN)
        .is_some_and(|o| o.to_str().ok() != Some(app.cfg.origin.as_str()))
    {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({"error":"Cross-origin requests are not allowed"})),
        )
            .into_response();
    }
    if request.method() != axum::http::Method::GET
        && (headers
            .get("x-notjik-request")
            .and_then(|v| v.to_str().ok())
            != Some("1")
            || headers
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .map(|s| s.split(';').next().unwrap_or("").trim())
                != Some("application/json"))
    {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({"error":"JSON and X-Notjik-Request: 1 are required"})),
        )
            .into_response();
    }
    request.extensions_mut().insert(Identity(login.unwrap()));
    let mut response = next.run(request).await;
    let h = response.headers_mut();
    for (k,v) in [("cache-control","no-store"),("x-content-type-options","nosniff"),("referrer-policy","no-referrer"),("content-security-policy","default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'self'; img-src 'self'; base-uri 'none'; frame-ancestors 'none'; form-action 'self'")] {h.insert(axum::http::HeaderName::from_static(k),axum::http::HeaderValue::from_static(v));}
    response
}
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T> + Send + 'static,
) -> Result<T, Failure> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|_| Failure(anyhow::anyhow!("Authority worker unavailable")))?
        .map_err(Failure)
}
async fn health() -> Json<Value> {
    Json(json!({"status":"ok","protocol":1,"server":"axum"}))
}
async fn request(
    State(app): State<Arc<App>>,
    Extension(user): Extension<Identity>,
    headers: HeaderMap,
    Json(data): Json<Value>,
) -> Result<Json<Value>, Failure> {
    let device = headers
        .get("x-notjik-device")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned();
    let secret = headers
        .get("x-notjik-key")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned();
    if data["op"] == "android-config" {
        let app2 = app.clone();
        let owner = user.0.clone();
        let d = device.clone();
        let key = secret.clone();
        blocking(move || store::request(&app2.database, &owner, &d, &key, json!({"op":"devices"})))
            .await?;
        let path = app
            .cfg
            .android_config
            .as_ref()
            .ok_or_else(|| Failure(anyhow::anyhow!("Firebase configuration pending")))?;
        return Ok(Json(
            serde_json::from_slice(&std::fs::read(path).map_err(anyhow::Error::from)?)
                .map_err(anyhow::Error::from)?,
        ));
    }
    Ok(Json(
        blocking(move || store::request(&app.database, &user.0, &device, &secret, data)).await?,
    ))
}
fn router(app: Arc<App>) -> Router {
    let routes = Router::new()
        .route("/api/v1/health", get(health))
        .route("/api/v1/request", post(request));
    Router::new()
        .nest("/notjik", routes.clone())
        .merge(routes)
        .layer(DefaultBodyLimit::max(131072))
        .layer(middleware::from_fn_with_state(app.clone(), auth))
        .layer(ConcurrencyLimitLayer::new(32))
        .layer(tower_http::timeout::TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(15),
        ))
        .with_state(app)
}
#[tokio::main]
async fn main() -> Result<()> {
    unsafe {
        libc::umask(0o077);
    }
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() == 3 && args[0] == "snapshot" {
        let c = store::connect(std::path::Path::new(&args[1]))?;
        anyhow::ensure!(
            !std::path::Path::new(&args[2]).exists(),
            "Snapshot destination exists"
        );
        c.backup(rusqlite::DatabaseName::Main, &args[2], None)?;
        let copy = rusqlite::Connection::open(&args[2])?;
        anyhow::ensure!(
            copy.query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0))? == "ok",
            "Snapshot failed integrity check"
        );
        println!("Snapshot verified");
        return Ok(());
    }
    anyhow::ensure!(
        args.len() == 2 && args[0] == "--config",
        "notjik-api --config FILE | snapshot DATABASE NEW_FILE"
    );
    let cfg = toml::from_str::<FileConfig>(&std::fs::read_to_string(&args[1])?)?.server;
    anyhow::ensure!(
        cfg.origin.starts_with("https://")
            && !cfg.origin.trim_start_matches("https://").contains('/'),
        "Invalid HTTPS origin"
    );
    anyhow::ensure!(
        !cfg.allowed_users.is_empty() && !cfg.allowed_users.contains("*"),
        "Explicit identity allowlist required"
    );
    std::fs::create_dir_all(&cfg.directory)?;
    std::fs::set_permissions(&cfg.directory, std::fs::Permissions::from_mode(0o700))?;
    let database = cfg.directory.join("notifications.sqlite3");
    store::connect(&database)?;
    if cfg.socket.exists() {
        anyhow::ensure!(
            cfg.socket.symlink_metadata()?.file_type().is_socket(),
            "Socket path occupied"
        );
        std::fs::remove_file(&cfg.socket)?;
    }
    let listener = UnixListener::bind(&cfg.socket)?;
    std::fs::set_permissions(&cfg.socket, std::fs::Permissions::from_mode(0o660))?;
    let app = Arc::new(App { cfg, database });
    let worker = app.clone();
    std::thread::spawn(move || push::run(worker));
    axum::serve(
        listener,
        router(app).into_make_service_with_connect_info::<Peer>(),
    )
    .with_graceful_shutdown(async {
        let mut sig =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).unwrap();
        tokio::select! {_=tokio::signal::ctrl_c()=>{},_=sig.recv()=>{}}
    })
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests;
