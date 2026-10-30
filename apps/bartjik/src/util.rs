use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::{path::PathBuf, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
};
pub fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default()
}
pub fn root() -> PathBuf {
    std::env::var_os("BARTJIK_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".local/lib/bartjik"))
}
pub fn xdg(env: &str, default: &str) -> PathBuf {
    std::env::var_os(env)
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(default))
}
pub fn expand(s: &str) -> PathBuf {
    if s == "~" {
        home()
    } else if let Some(s) = s.strip_prefix("~/") {
        home().join(s)
    } else {
        PathBuf::from(s)
    }
}
pub fn s(v: &Value) -> &str {
    v.as_str().unwrap_or("")
}
pub fn st(v: &Value) -> String {
    v.as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| v.to_string())
}
pub fn num(v: &Value) -> f64 {
    v.as_f64().unwrap_or(0.)
}
pub fn arr(v: &Value) -> &[Value] {
    v.as_array().map(Vec::as_slice).unwrap_or(&[])
}
pub fn obj(v: &Value) -> serde_json::Map<String, Value> {
    v.as_object().cloned().unwrap_or_default()
}
pub fn truth(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::String(s) => !s.is_empty(),
        Value::Number(_) => num(v) != 0.,
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}
pub fn now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}
pub fn read(p: impl AsRef<std::path::Path>) -> Result<Value> {
    Ok(serde_json::from_slice(&std::fs::read(p)?)?)
}
pub fn save(p: &std::path::Path, v: &Value) -> Result<()> {
    use std::io::Write;
    let dir = p.parent().context("no parent")?;
    std::fs::create_dir_all(dir)?;
    let mut f = tempfile::NamedTempFile::new_in(dir)?;
    f.write_all(serde_json::to_string(v)?.as_bytes())?;
    f.as_file().sync_all()?;
    f.persist(p)?;
    Ok(())
}
struct Group(tokio::process::Child);
impl Drop for Group {
    fn drop(&mut self) {
        if let Some(pid) = self.0.id() {
            unsafe {
                libc::kill(-(pid as i32), libc::SIGKILL);
            }
        }
    }
}
pub async fn command(
    args: &[String],
    timeout: f64,
    limit: usize,
    env: &[(String, String)],
    input: Option<&str>,
) -> Result<String> {
    ensure!(!args.is_empty(), "empty command");
    let mut cmd = Command::new(&args[0]);
    cmd.args(&args[1..])
        .envs(env.iter().cloned())
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .process_group(0);
    let mut p = Group(cmd.spawn().context("executable unavailable")?);
    let mut output = p.0.stdout.take().unwrap();
    let input = input.map(str::to_owned);
    let mut stdin = p.0.stdin.take();
    let run = async {
        let writer = async {
            if let (Some(ref mut pipe), Some(ref data)) = (&mut stdin, &input) {
                pipe.write_all(data.as_bytes()).await?;
                pipe.shutdown().await?;
            }
            Ok::<(), anyhow::Error>(())
        };
        let reader = async {
            let mut data = Vec::new();
            (&mut output)
                .take(limit as u64 + 1)
                .read_to_end(&mut data)
                .await?;
            ensure!(data.len() <= limit, "provider output limit exceeded");
            Ok::<_, anyhow::Error>(data)
        };
        let (_, data) = tokio::try_join!(writer, reader)?;
        let status = p.0.wait().await?;
        ensure!(
            status.success(),
            "{} failed (exit {})",
            args[0],
            status.code().unwrap_or(-1)
        );
        Ok(String::from_utf8_lossy(&data).trim().to_owned())
    };
    tokio::time::timeout(Duration::from_secs_f64(timeout.clamp(0.1, 120.)), run)
        .await
        .context("command timed out")?
}
pub async fn cmd(args: &[&str]) -> Result<String> {
    command(
        &args.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        5.,
        262144,
        &[],
        None,
    )
    .await
}
pub async fn jc(args: &[&str]) -> Result<Value> {
    Ok(serde_json::from_str(&cmd(args).await?)?)
}
pub async fn http(url: &str, headers: &[(String, String)], limit: usize) -> Result<Vec<u8>> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .redirect(
            if headers.iter().any(|(k, _)| {
                k.eq_ignore_ascii_case("authorization")
                    || k.eq_ignore_ascii_case("x-cmc_pro_api_key")
            }) {
                reqwest::redirect::Policy::none()
            } else {
                reqwest::redirect::Policy::limited(3)
            },
        )
        .build()?;
    let mut r = client.get(url);
    for (k, v) in headers {
        r = r.header(k, v);
    }
    let mut r = r
        .send()
        .await
        .map_err(|_| anyhow::anyhow!("API unavailable"))?;
    if r.status() == 429 {
        let delay = r
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(60.)
            .clamp(1., 3600.);
        anyhow::bail!("HTTP 429; retry {delay}");
    }
    ensure!(r.status().is_success(), "HTTP {}", r.status().as_u16());
    let mut data = Vec::new();
    while let Some(chunk) = r
        .chunk()
        .await
        .map_err(|_| anyhow::anyhow!("API read failed"))?
    {
        ensure!(
            data.len() + chunk.len() <= limit,
            "API output limit exceeded"
        );
        data.extend_from_slice(&chunk);
    }
    Ok(data)
}
pub async fn hj(url: &str, headers: &[(String, String)]) -> Result<Value> {
    Ok(serde_json::from_slice(&http(url, headers, 1048576).await?)?)
}
pub fn item(
    id: &str,
    text: Option<&str>,
    icon: Option<&str>,
    kind: &str,
    state: Value,
    tooltip: &str,
    actions: Value,
) -> Value {
    let mut v = json!({"id":id,"type":if text.is_some()&&icon.is_some(){"icon_text"}else if icon.is_some(){"icon"}else{"text"},"state":state,"tooltip":tooltip,"visible":true,"enabled":true,"actions":actions});
    if let Some(t) = text {
        v["text"] = json!({"value":t,"style":{},"actions":{}})
    }
    if let Some(i) = icon {
        v["icon"] = json!({"kind":kind,"value":i,"style":{},"actions":{}})
    }
    v
}
pub fn icon(name: &str) -> String {
    format!("{}/qml/icons/{name}.svg", root().display())
}
pub fn field(label: &str, value: Value, detail: &str) -> Value {
    json!({"label":label,"value":st(&value),"detail":detail})
}
pub fn panel(title: &str, rows: Value, subtitle: &str) -> Value {
    json!({"title":title,"rows":rows,"subtitle":subtitle})
}
