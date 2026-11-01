use crate::store::write_private;
use anyhow::{Context, Result, bail};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Deserialize, Serialize)]
pub struct Credential {
    pub client_id: String,
    pub client_secret: String,
    pub refresh_token: String,
    pub access_token: String,
    pub expires_at: u64,
    #[serde(default)]
    pub scope: String,
}
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn http() -> Result<reqwest::blocking::Client> {
    Ok(reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(35))
        .build()?)
}

pub struct Auth {
    path: PathBuf,
    credential: Credential,
}
fn credential_lock(path: &Path) -> Result<fs::File> {
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .open(path.with_extension("lock"))?;
    lock.lock()?;
    Ok(lock)
}
impl Auth {
    pub fn open(dir: &Path) -> Result<Self> {
        let path = dir.join("account.json");
        let credential = serde_json::from_slice(
            &fs::read(&path)
                .context("Connect Gmail first: gmailtjik --connect /private/path/client.json")?,
        )?;
        Ok(Self { path, credential })
    }
    pub fn token(&mut self) -> Result<String> {
        let _lock = credential_lock(&self.path)?;
        // A GUI reconnect or the sync service may have refreshed these credentials.
        self.credential = serde_json::from_slice(&fs::read(&self.path)?)?;
        if self.credential.expires_at < now() + 90 {
            let response = http()?
                .post("https://oauth2.googleapis.com/token")
                .form(&[
                    ("grant_type", "refresh_token"),
                    ("refresh_token", &self.credential.refresh_token),
                    ("client_id", &self.credential.client_id),
                    ("client_secret", &self.credential.client_secret),
                ])
                .send()?;
            if !response.status().is_success() {
                bail!("Google authorization expired or was revoked. Reconnect Gmail.");
            }
            let token: serde_json::Value = response.json()?;
            self.credential.access_token = token["access_token"]
                .as_str()
                .context("Missing access token")?
                .into();
            self.credential.expires_at = now() + token["expires_in"].as_u64().unwrap_or(3600);
            write_private(&self.path, &serde_json::to_vec(&self.credential)?)?;
        }
        Ok(self.credential.access_token.clone())
    }
    pub fn contacts(&self) -> bool {
        self.credential.scope.contains("contacts.readonly")
    }
}

pub fn connect(dir: &Path, client_path: &Path, contacts: bool) -> Result<()> {
    let client_path = client_path.canonicalize()?;
    if crate::store::in_repository(&client_path) {
        bail!("Keep the Google client JSON outside the repository");
    }
    let client: serde_json::Value = serde_json::from_slice(&fs::read(client_path)?)?;
    let client = client
        .get("installed")
        .context("Use a Google Desktop OAuth client JSON")?;
    let client_id = client["client_id"].as_str().context("Missing client ID")?;
    let client_secret = client["client_secret"].as_str().unwrap_or_default();
    let listener = TcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(true)?;
    let redirect = format!("http://127.0.0.1:{}/", listener.local_addr()?.port());
    let verifier = URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>());
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let state = URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>());
    let scope = if contacts {
        "https://www.googleapis.com/auth/gmail.modify https://www.googleapis.com/auth/contacts.readonly https://www.googleapis.com/auth/contacts.other.readonly"
    } else {
        "https://www.googleapis.com/auth/gmail.modify"
    };
    let mut url = url::Url::parse("https://accounts.google.com/o/oauth2/v2/auth")?;
    url.query_pairs_mut().extend_pairs([
        ("client_id", client_id),
        ("redirect_uri", &redirect),
        ("response_type", "code"),
        ("scope", scope),
        ("access_type", "offline"),
        ("prompt", "consent"),
        ("state", &state),
        ("code_challenge", &challenge),
        ("code_challenge_method", "S256"),
    ]);
    webbrowser::open(url.as_str()).context("Could not open the Google authorization browser")?;
    println!("Complete Gmail authorization in your browser. Waiting up to 10 minutes.");
    let deadline = std::time::Instant::now() + Duration::from_secs(600);
    let code = loop {
        if std::time::Instant::now() > deadline {
            bail!("Google authorization timed out");
        }
        let (mut socket, _) = match listener.accept() {
            Ok(pair) => pair,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }
            Err(error) => return Err(error.into()),
        };
        socket.set_read_timeout(Some(Duration::from_secs(3)))?;
        let mut bytes = [0_u8; 8192];
        let n = socket.read(&mut bytes)?;
        let request = String::from_utf8_lossy(&bytes[..n]);
        let target = request
            .lines()
            .next()
            .unwrap_or_default()
            .split_whitespace()
            .nth(1)
            .unwrap_or("/");
        let parsed = url::Url::parse(&format!("{redirect}{}", target.trim_start_matches('/')))?;
        let params: std::collections::HashMap<_, _> = parsed.query_pairs().into_owned().collect();
        if params.get("state") != Some(&state) {
            socket.write_all(b"HTTP/1.1 400 Bad Request\r\nConnection: close\r\n\r\nInvalid authorization state.")?;
            continue;
        }
        if params.contains_key("error") {
            bail!("Google authorization was declined");
        }
        let code = params
            .get("code")
            .context("Google callback omitted authorization code")?
            .clone();
        socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\nGmailtjik authorization received. You can close this tab.")?;
        break code;
    };
    let response = http()?
        .post("https://oauth2.googleapis.com/token")
        .form(&[
            ("grant_type", "authorization_code"),
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("code", &code),
            ("code_verifier", &verifier),
            ("redirect_uri", &redirect),
        ])
        .send()?;
    if !response.status().is_success() {
        bail!("Google rejected the authorization exchange");
    }
    let token: serde_json::Value = response.json()?;
    let credential = Credential {
        client_id: client_id.into(),
        client_secret: client_secret.into(),
        refresh_token: token["refresh_token"]
            .as_str()
            .context("No refresh token returned")?
            .into(),
        access_token: token["access_token"]
            .as_str()
            .context("No access token returned")?
            .into(),
        expires_at: now() + token["expires_in"].as_u64().unwrap_or(3600),
        scope: token["scope"].as_str().unwrap_or(scope).into(),
    };
    let profile = http()?
        .get("https://gmail.googleapis.com/gmail/v1/users/me/profile")
        .bearer_auth(&credential.access_token)
        .send()?
        .error_for_status()?
        .json::<serde_json::Value>()?;
    let email = profile["emailAddress"]
        .as_str()
        .context("Google did not return the account email")?;
    let store = crate::store::Store::open(Some(dir))?;
    if store
        .get::<String>("email")?
        .is_some_and(|old| old != email)
    {
        bail!(
            "This data directory belongs to a different Gmail account. Set GMAILTJIK_DATA_DIR to a separate private directory and connect again"
        );
    }
    let _lock = credential_lock(&dir.join("account.json"))?;
    write_private(&dir.join("account.json"), &serde_json::to_vec(&credential)?)?;
    store.put("email", &email)?;
    println!("Connected. Run gmailtjik to open your inbox.");
    Ok(())
}
