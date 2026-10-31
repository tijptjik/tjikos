//! Firebase sees only an opaque event reference. All content stays on the tailnet.
use super::*;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use std::{
    io::Write,
    process::{Command, Stdio},
};
struct Firebase {
    credentials: Value,
    token: String,
    until: i64,
    http: reqwest::blocking::Client,
}
impl Firebase {
    fn new(path: &std::path::Path) -> Result<Self> {
        anyhow::ensure!(
            path.metadata()?.permissions().mode() & 0o077 == 0,
            "Firebase credentials must be private"
        );
        Ok(Self {
            credentials: serde_json::from_slice(&std::fs::read(path)?)?,
            token: String::new(),
            until: 0,
            http: reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(15))
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
        })
    }
    fn token(&mut self) -> Result<()> {
        let now = notjik_sync::now();
        if now < self.until {
            return Ok(());
        }
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"RS256","typ":"JWT"}"#);
        let payload=URL_SAFE_NO_PAD.encode(serde_json::to_vec(&json!({"iss":self.credentials["client_email"],"scope":"https://www.googleapis.com/auth/firebase.messaging","aud":"https://oauth2.googleapis.com/token","iat":now,"exp":now+3600}))?);
        let signing = format!("{header}.{payload}");
        let mut key = tempfile::NamedTempFile::new()?;
        key.write_all(
            self.credentials["private_key"]
                .as_str()
                .context("Missing Firebase signing key")?
                .as_bytes(),
        )?;
        key.flush()?;
        let mut child = Command::new("openssl")
            .args(["dgst", "-sha256", "-sign"])
            .arg(key.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        child.stdin.take().unwrap().write_all(signing.as_bytes())?;
        let out = child.wait_with_output()?;
        anyhow::ensure!(out.status.success(), "Firebase signing failed");
        let assertion = format!("{signing}.{}", URL_SAFE_NO_PAD.encode(out.stdout));
        let response = self
            .http
            .post("https://oauth2.googleapis.com/token")
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
                ("assertion", &assertion),
            ])
            .send()
            .map_err(|_| anyhow::anyhow!("Firebase authentication transport failed"))?;
        anyhow::ensure!(
            response.status().is_success(),
            "Firebase authentication rejected"
        );
        let body: Value = response.json()?;
        self.token = body["access_token"]
            .as_str()
            .context("Missing access token")?
            .into();
        self.until = now + 3300;
        Ok(())
    }
    fn send(&mut self, token: &str, id: &str) -> Result<()> {
        self.token()?;
        let project = self.credentials["project_id"]
            .as_str()
            .context("Missing Firebase project")?;
        anyhow::ensure!(
            project
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-'),
            "Invalid Firebase project"
        );
        let response=self.http.post(format!("https://fcm.googleapis.com/v1/projects/{project}/messages:send")).bearer_auth(&self.token)
            .json(&json!({"message":{"token":token,"data":{"reference":id},"android":{"priority":"HIGH","ttl":"60s","collapse_key":"notjik"}}})).send().map_err(|_|anyhow::anyhow!("Firebase delivery transport failed"))?;
        anyhow::ensure!(
            response.status().is_success(),
            "Firebase rejected delivery (HTTP {})",
            response.status()
        );
        Ok(())
    }
}
pub fn run(app: Arc<App>) {
    let mut firebase = None;
    loop {
        let result = (|| -> Result<()> {
            let mut c = store::connect(&app.database)?;
            {
                let tx = c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
                for owner in &app.cfg.allowed_users {
                    store::route(&tx, owner, notjik_sync::now())?;
                }
                tx.commit()?;
            }
            if firebase.is_none() {
                if let Some(p) = &app.cfg.firebase_credentials {
                    if p.exists() {
                        firebase = Some(Firebase::new(p)?);
                    }
                }
            }
            if let Some(f) = firebase.as_mut() {
                let mut s=c.prepare("SELECT n.id,n.revision,d.push FROM notices n JOIN devices d ON n.target=d.id WHERE n.dismissed=0 AND n.delivered=0 AND d.kind='android' AND d.paused=0 AND d.push!='' AND n.pushed + CASE WHEN n.push_attempts<2 THEN 60 WHEN n.push_attempts<5 THEN 300 ELSE 3600 END <?1 LIMIT 20")?;
                let pending = s
                    .query_map([notjik_sync::now()], |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            r.get::<_, i64>(1)?,
                            r.get::<_, String>(2)?,
                        ))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                for (id, revision, token) in pending {
                    f.send(&token, &id)?;
                    c.execute(
                        "UPDATE notices SET pushed=?1,push_attempts=push_attempts+1 WHERE id=?2 AND revision=?3",
                        rusqlite::params![notjik_sync::now(), id, revision],
                    )?;
                }
            }
            Ok(())
        })();
        if let Err(e) = result {
            eprintln!("Notjik delivery: {e}");
        }
        std::thread::sleep(Duration::from_secs(5));
    }
}
