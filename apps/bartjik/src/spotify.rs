use crate::util::*;
use anyhow::{ensure, Context, Result};
use hmac::{Hmac, Mac};
use serde_json::{json, Value};
use sha1::Sha1;
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
};
pub fn path() -> std::path::PathBuf {
    xdg("XDG_CONFIG_HOME", ".config").join("bartjik/spotify-sp-dc")
}
pub fn valid_cookie(v: &str) -> bool {
    regex::Regex::new(r"^[A-Za-z0-9_%+./=~-]{1,4096}$")
        .unwrap()
        .is_match(v)
}
pub fn cookie() -> String {
    let read = || -> Result<String> {
        let f = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path())?;
        let m = f.metadata()?;
        ensure!(
            m.is_file() && m.uid() == unsafe { libc::getuid() } && m.mode() & 0o077 == 0,
            "private credential required"
        );
        let mut raw = String::new();
        f.take(4097).read_to_string(&mut raw)?;
        let v = raw.trim();
        ensure!(valid_cookie(v), "invalid credential");
        Ok(v.into())
    };
    read().unwrap_or_default()
}
pub fn configure(action: &str) -> Result<Value> {
    match action {
        "login" => {
            use std::os::fd::AsRawFd;
            let mut tty = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open("/dev/tty")
                .context("Run spotify login in an interactive terminal")?;
            let fd = tty.as_raw_fd();
            let mut original = unsafe { std::mem::zeroed::<libc::termios>() };
            ensure!(
                unsafe { libc::tcgetattr(fd, &mut original) } == 0,
                "Interactive terminal required"
            );
            struct Restore(i32, libc::termios);
            impl Drop for Restore {
                fn drop(&mut self) {
                    unsafe {
                        libc::tcsetattr(self.0, libc::TCSAFLUSH, &self.1);
                    }
                }
            }
            let _restore = Restore(fd, original);
            let mut hidden = original;
            hidden.c_lflag &= !(libc::ECHO | libc::ISIG);
            ensure!(
                unsafe { libc::tcsetattr(fd, libc::TCSAFLUSH, &hidden) } == 0,
                "Cannot hide terminal input"
            );
            tty.write_all(b"Spotify sp_dc cookie (hidden): ")?;
            let mut bytes = vec![];
            let mut c = [0];
            while tty.read(&mut c)? == 1 {
                if c[0] == b'\n' || c[0] == b'\r' {
                    break;
                }
                ensure!(
                    c[0] != 3 && c[0] != 4 && bytes.len() < 4097,
                    "Login cancelled"
                );
                bytes.push(c[0]);
            }
            tty.write_all(b"\n")?;
            let value = std::str::from_utf8(&bytes)?.trim();
            ensure!(valid_cookie(value), "Enter only the sp_dc cookie value");
            let p = path();
            std::fs::create_dir_all(p.parent().unwrap())?;
            let mut f = tempfile::NamedTempFile::new_in(p.parent().unwrap())?;
            writeln!(f, "{value}")?;
            f.persist(p)?;
        }
        "logout" => match std::fs::remove_file(path()) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        },
        "status" => {}
        _ => anyhow::bail!("unknown spotify command"),
    };
    Ok(json!({"ok":true,"configured":!cookie().is_empty(),"path":path()}))
}
pub fn totp(dictionary: &Value, timestamp: f64) -> Result<(String, String)> {
    let versions: Vec<u32> = obj(dictionary)
        .keys()
        .filter(|k| !k.is_empty() && k.len() <= 6 && k.bytes().all(|b| b.is_ascii_digit()))
        .filter_map(|k| k.parse().ok())
        .collect();
    let version = versions
        .iter()
        .max()
        .context("Missing Spotify authentication version")?
        .to_string();
    let values = arr(&dictionary[&version]);
    ensure!(
        !values.is_empty()
            && values.len() <= 256
            && values.iter().all(|v| v.as_u64().is_some_and(|n| n <= 255)),
        "Invalid Spotify authentication data"
    );
    let key = values
        .iter()
        .enumerate()
        .map(|(i, v)| (v.as_u64().unwrap() ^ ((i % 33 + 9) as u64)).to_string())
        .collect::<String>();
    let mut mac = <Hmac<Sha1> as Mac>::new_from_slice(key.as_bytes())?;
    mac.update(&((timestamp as u64) / 30).to_be_bytes());
    let digest = mac.finalize().into_bytes();
    let off = (digest[19] & 15) as usize;
    let number = u32::from_be_bytes(digest[off..off + 4].try_into().unwrap()) & 0x7fffffff;
    Ok((version, format!("{:06}", number % 1000000)))
}
async fn request(url: &str, headers: &[(&str, String)]) -> Result<Value> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let mut r = client
        .get(url)
        .header("User-Agent", "Mozilla/5.0")
        .header("Accept", "application/json");
    for (k, v) in headers {
        r = r.header(*k, v);
    }
    let mut response = r
        .send()
        .await
        .map_err(|_| anyhow::anyhow!("Spotify lookup unavailable"))?;
    if response.status() == 429 {
        let delay = response
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(60.)
            .clamp(60., 3600.);
        anyhow::bail!("HTTP 429; retry {delay}");
    }
    ensure!(
        response.status().is_success(),
        "HTTP {}",
        response.status().as_u16()
    );
    let mut raw = vec![];
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| anyhow::anyhow!("Spotify lookup unavailable"))?
    {
        ensure!(
            raw.len() + chunk.len() <= 512 * 1024,
            "Spotify response exceeds lookup limit"
        );
        raw.extend_from_slice(&chunk);
    }
    Ok(serde_json::from_slice(&raw)?)
}
pub fn parse(data: &Value) -> Result<Option<Value>> {
    let lyrics = &data["lyrics"];
    ensure!(
        lyrics.is_object() && lyrics["lines"].is_array(),
        "Invalid Spotify lyrics response"
    );
    let mut texts = vec![];
    let mut timed = std::collections::BTreeMap::<i64, Vec<String>>::new();
    let mut synced = lyrics["syncType"] == "LINE_SYNCED";
    for row in arr(&lyrics["lines"]) {
        let text = row["words"]
            .as_str()
            .context("Invalid Spotify lyrics line")?
            .trim();
        texts.push(text);
        let stamp = st(&row["startTimeMs"]).parse::<f64>().unwrap_or(f64::NAN) / 1000.;
        if stamp.is_finite() && (0. ..=86400.).contains(&stamp) {
            let lines = timed.entry((stamp * 1000.).round() as i64).or_default();
            if !text.is_empty() && !lines.iter().any(|t| t == text) {
                lines.push(text.into());
            }
        } else {
            synced = false;
        }
    }
    let text = texts.join("\n").trim().to_owned();
    if text.is_empty() {
        return Ok(None);
    }
    let mut result = json!({"status":"ready","text":text,"source":"Spotify"});
    if synced && !timed.is_empty() {
        result["lines"] = json!(timed
            .iter()
            .map(|(t, v)| json!({"time":*t as f64/1000.,"text":v.join("\n")}))
            .collect::<Vec<_>>());
    }
    Ok(Some(result))
}
#[derive(Default)]
pub struct Spotify {
    credential: String,
    token: String,
    expires: f64,
    secrets: Value,
    secrets_until: f64,
    retry: f64,
}
impl Spotify {
    async fn access(&mut self, cookie: &str) -> Result<String> {
        if !self.token.is_empty() && now() < self.expires {
            return Ok(self.token.clone());
        }
        if !truth(&self.secrets) || now() >= self.secrets_until {
            self.secrets=request("https://code.thetadev.de/ThetaDev/spotify-secrets/raw/branch/main/secrets/secretDict.json",&[]).await?;
            self.secrets_until = now() + 86400.;
        }
        let clock = request("https://open.spotify.com/api/server-time", &[]).await?;
        let timestamp = num(&clock["serverTime"]);
        ensure!(timestamp > 0., "Invalid Spotify server time");
        let (version, code) = totp(&self.secrets, timestamp)?;
        let url=format!("https://open.spotify.com/api/token?reason=init&productType=web-player&totp={code}&totpServer={code}&totpVer={version}");
        let data = request(&url, &[("Cookie", format!("sp_dc={cookie}"))]).await?;
        ensure!(data["isAnonymous"] == false, "Spotify login expired");
        let token = s(&data["accessToken"]);
        ensure!(
            regex::Regex::new(r"^[A-Za-z0-9._~-]{1,8192}$")?.is_match(token),
            "Invalid Spotify access token"
        );
        let expiry = num(&data["accessTokenExpirationTimestampMs"]) / 1000.;
        ensure!(expiry > now(), "Expired Spotify access token");
        self.token = token.into();
        self.expires = now() + ((expiry - now()).min(3600.) - 30.).max(0.);
        Ok(self.token.clone())
    }
    pub async fn get(&mut self, id: &str, cookie: &str) -> Result<Option<Value>> {
        if cookie.is_empty() || !regex::Regex::new(r"^[A-Za-z0-9]{22}$")?.is_match(id) {
            return Ok(None);
        }
        let fingerprint = format!("{:x}", Sha256::digest(cookie.as_bytes()));
        if fingerprint != self.credential {
            self.credential = fingerprint;
            self.token.clear();
            self.retry = 0.;
        }
        ensure!(now() >= self.retry, "Spotify lookup cooling down");
        let result=tokio::time::timeout(std::time::Duration::from_secs(8),async{for attempt in 0..2{let token=self.access(cookie).await?;match request(&format!("https://spclient.wg.spotify.com/color-lyrics/v2/track/{id}?format=json&market=from_token"),&[("Authorization",format!("Bearer {token}")),("App-Platform","WebPlayer".into())]).await{Ok(v)=>return parse(&v),Err(e)if e.to_string()=="HTTP 404"=>return Ok(None),Err(e)if e.to_string()=="HTTP 401"&&attempt==0=>self.token.clear(),Err(e)=>return Err(e)}}Ok(None)}).await.unwrap_or_else(|_|Err(anyhow::anyhow!("Spotify lookup timed out")));
        if let Err(ref e) = result {
            let text = e.to_string();
            self.retry = now()
                + if text == "HTTP 401" || text == "HTTP 403" {
                    300.
                } else {
                    text.strip_prefix("HTTP 429; retry ")
                        .and_then(|s| s.parse::<f64>().ok())
                        .unwrap_or(30.)
                };
            self.secrets_until = 0.;
        }
        result
    }
}
