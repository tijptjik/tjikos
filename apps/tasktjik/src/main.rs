mod db;
mod model;

use anyhow::{Context, Result};
use axum::{
    extract::{ConnectInfo, DefaultBodyLimit, Request, State},
    http::{header, HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Extension, Json, Router,
};
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
            Json(json!({"error":"This Tailscale identity is not allowed to access Tasktjik"})),
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
            .get("x-tasktjik-request")
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
            Json(json!({"error":"JSON and X-Tasktjik-Request: 1 are required"})),
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
async fn health(State(app): State<Arc<App>>) -> Result<Json<Value>, Failure> {
    blocking(move || {
        let c = db::connect(&app.database)?;
        c.query_row("SELECT 1", [], |r| r.get::<_, i64>(0))?;
        Ok(())
    })
    .await?;
    Ok(Json(
        json!({"status":"ok","storage":"sqlite","protocol":1,"server":"axum"}),
    ))
}
async fn identity(Extension(user): Extension<Identity>) -> Json<Value> {
    Json(json!({"login":user.0}))
}
async fn list(State(app): State<Arc<App>>) -> Result<Json<Value>, Failure> {
    let tasks = blocking(move || {
        let c = db::connect(&app.database)?;
        let mut tasks = db::tasks(&c)?;
        for t in &mut tasks {
            let p = model::period(t, model::today())?;
            t["done"] = json!(t["completions"].get(&p).is_some());
            t["period"] = json!(p);
        }
        Ok(tasks)
    })
    .await?;
    let mut groups = vec![
        "All".to_string(),
        "Quick".into(),
        "General".into(),
        "Work".into(),
        "Socials".into(),
        "Domestic".into(),
        "Finances".into(),
        "Tech".into(),
    ];
    for t in &tasks {
        if let Some(g) = t["group"].as_str() {
            if !groups.iter().any(|x| x == g) {
                groups.push(g.into());
            }
        }
    }
    Ok(Json(
        json!({"tasks":tasks,"groups":groups,"today":model::today().to_string()}),
    ))
}
async fn request(
    State(app): State<Arc<App>>,
    Extension(user): Extension<Identity>,
    headers: HeaderMap,
    Json(mut data): Json<Value>,
) -> Result<Json<Value>, Failure> {
    if !data.is_object() {
        return Err(Failure(anyhow::anyhow!("Request must be an object")));
    }
    data["actor"] = json!(user.0);
    let device = headers
        .get("x-tasktjik-device")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned();
    if ["claim", "ack", "release"].contains(&data["action"].as_str().unwrap_or("")) {
        data["device"] = json!(device);
    }
    let result = blocking(move || db::request(&app.database, data, &user.0, &device)).await?;
    Ok(Json(json!({"result":result})))
}
async fn devices(
    State(app): State<Arc<App>>,
    Extension(user): Extension<Identity>,
) -> Result<Json<Value>, Failure> {
    Ok(Json(
        blocking(move || db::devices(&app.database, &user.0, None)).await?,
    ))
}
async fn enroll(
    State(app): State<Arc<App>>,
    Extension(user): Extension<Identity>,
    Json(data): Json<Value>,
) -> Result<Json<Value>, Failure> {
    Ok(Json(
        blocking(move || db::devices(&app.database, &user.0, Some(data))).await?,
    ))
}
fn asset(mime: &'static str, bytes: &'static [u8]) -> Response {
    ([(header::CONTENT_TYPE, mime)], bytes).into_response()
}
fn router(app: Arc<App>) -> Router {
    let routes = Router::new()
        .route(
            "/",
            get(|| async {
                asset(
                    "text/html; charset=utf-8",
                    include_bytes!("../web/index.html"),
                )
            }),
        )
        .route(
            "/app.js",
            get(|| async {
                asset(
                    "text/javascript; charset=utf-8",
                    include_bytes!("../web/app.js"),
                )
            }),
        )
        .route(
            "/style.css",
            get(|| async {
                asset(
                    "text/css; charset=utf-8",
                    include_bytes!("../web/style.css"),
                )
            }),
        )
        .route("/api/v1/health", get(health))
        .route("/api/v1/identity", get(identity))
        .route("/api/v1/tasks", get(list))
        .route("/api/v1/request", post(request))
        .route("/api/v1/devices", get(devices).post(enroll));
    // Serve strips the mount prefix; accepting both shapes also permits direct
    // root-owned Unix-socket diagnostics without broadening authentication.
    Router::new()
        .nest("/tasktjik", routes.clone())
        .merge(routes)
        .layer(DefaultBodyLimit::max(262144))
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
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() == 3 && (args[0] == "snapshot" || args[0] == "import-json") {
        let path = PathBuf::from(&args[1]);
        let target = PathBuf::from(&args[2]);
        if args[0] == "snapshot" {
            db::snapshot(&path, &target)?;
            std::fs::set_permissions(target, std::fs::Permissions::from_mode(0o600))?;
            println!("Snapshot verified");
        } else {
            println!("Imported {} records", db::import(&path, &target)?);
        }
        return Ok(());
    }
    anyhow::ensure!(args.len()==2 && args[0]=="--config","Usage: tasktjik-api --config FILE | snapshot DATABASE DESTINATION | import-json DATABASE DIRECTORY");
    let cfg: Config = toml::from_str::<FileConfig>(&std::fs::read_to_string(&args[1])?)?.server;
    anyhow::ensure!(
        cfg.origin.starts_with("https://")
            && !cfg.origin.trim_start_matches("https://").contains('/'),
        "origin must be an HTTPS origin without a path"
    );
    anyhow::ensure!(
        !cfg.allowed_users.is_empty() && !cfg.allowed_users.contains("*"),
        "Explicit allowed_users required"
    );
    std::fs::create_dir_all(&cfg.directory)?;
    std::fs::set_permissions(&cfg.directory, std::fs::Permissions::from_mode(0o700))?;
    let database = cfg.directory.join("tasks.sqlite3");
    db::connect(&database)?;
    std::fs::set_permissions(&database, std::fs::Permissions::from_mode(0o600))?;
    if cfg.socket.exists() {
        anyhow::ensure!(
            cfg.socket.symlink_metadata()?.file_type().is_socket(),
            "Refusing to replace a non-socket"
        );
        std::fs::remove_file(&cfg.socket)?;
    }
    let listener = UnixListener::bind(&cfg.socket).context("Bind protected API socket")?;
    std::fs::set_permissions(
        &cfg.socket,
        std::fs::Permissions::from_mode(if cfg.local_users.is_empty() {
            0o600
        } else {
            0o660
        }),
    )?;
    let app = Arc::new(App { cfg, database });
    axum::serve(
        listener,
        router(app).into_make_service_with_connect_info::<Peer>(),
    )
    .with_graceful_shutdown(async {
        let mut signal = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("signal handler");
        tokio::select! {_=tokio::signal::ctrl_c()=>{},_=signal.recv()=>{}}
    })
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests;
