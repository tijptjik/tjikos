use crate::{
    discovery::{folded, Entry},
    util::*,
};
use anyhow::{bail, Result};
use serde_json::{json, Value};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    os::unix::{fs::MetadataExt, net::UnixStream},
    path::PathBuf,
    time::Duration,
};
pub fn directory() -> Result<PathBuf> {
    let root = PathBuf::from(std::env::var("XDG_RUNTIME_DIR")?);
    let meta = root.metadata()?;
    if meta.uid() != unsafe { libc::getuid() } || meta.mode() & 0o077 != 0 {
        bail!("XDG_RUNTIME_DIR must be owned by you and private");
    }
    Ok(root.join("runtjik-browser"))
}
pub fn request(path: &std::path::Path, message: &Value) -> Result<Value> {
    let mut client = UnixStream::connect(path)?;
    client.set_read_timeout(Some(Duration::from_secs(12)))?;
    client.set_write_timeout(Some(Duration::from_secs(12)))?;
    writeln!(client, "{message}")?;
    let mut data = Vec::new();
    BufReader::new(client)
        .take(4 * 1024 * 1024 + 1)
        .read_until(b'\n', &mut data)?;
    if data.len() > 4 * 1024 * 1024 {
        bail!("Browser response too large");
    }
    let value: Value = serde_json::from_slice(&data)?;
    if let Some(e) = value.get("error") {
        bail!("{}", s(e));
    }
    Ok(value)
}
#[derive(Default)]
pub struct Browser {
    pub scope: Option<(String, Value, String)>,
}
impl Browser {
    pub fn query(&self, q: &str, demo: bool) -> Result<Vec<Entry>> {
        let (command, mut term) = q.split_once(' ').unwrap_or((q, ""));
        let tabs_mode = !["/browser", "/zen"].contains(&command);
        if !q.starts_with('/') {
            term = q;
        }
        let sessions = if demo {
            vec![(
                "demo".into(),
                json!({"windows":[{"id":1,"title":"Research","tabs":[{"id":11,"title":"Firefox extension documentation","url":"https://example.org/firefox"},{"id":12,"title":"Hyprland window management","url":"https://example.org/hyprland"}]},{"id":2,"title":"Reading","tabs":[{"id":21,"title":"A synthetic reading list","url":"https://example.org/reading"}]}]}),
            )]
        } else {
            let mut paths: Vec<_> = match fs::read_dir(directory()?) {
                Ok(entries) => entries
                    .filter_map(|e| e.ok())
                    .map(|e| e.path())
                    .filter(|p| p.extension().is_some_and(|x| x == "sock"))
                    .collect(),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => vec![],
                Err(e) => return Err(e.into()),
            };
            paths.sort();
            let mut sessions = vec![];
            for path in paths {
                match request(&path, &json!({"op":"list"})) {
                    Ok(v) => {
                        sessions.push((path.file_name().unwrap().to_string_lossy().to_string(), v))
                    }
                    Err(e)
                        if e.downcast_ref::<std::io::Error>().is_some_and(|e| {
                            matches!(
                                e.kind(),
                                std::io::ErrorKind::ConnectionRefused
                                    | std::io::ErrorKind::NotFound
                            )
                        }) => {}
                    Err(e) => return Err(e),
                }
            }
            if sessions.is_empty() {
                bail!("Connect the Runtjik Browser extension in Zen or Firefox; see browser-extension/README.md");
            }
            sessions
        };
        let mut rows = vec![];
        for (session, state) in sessions {
            for window in state["windows"]
                .as_array()
                .ok_or_else(|| anyhow::anyhow!("Invalid browser windows"))?
            {
                if tabs_mode
                    && self
                        .scope
                        .as_ref()
                        .is_some_and(|(s, id, _)| *s != session || *id != window["id"])
                {
                    continue;
                }
                let title = if s(&window["title"]).is_empty() {
                    "Browser window"
                } else {
                    s(&window["title"])
                };
                let tabs: Vec<_> = window["tabs"]
                    .as_array()
                    .ok_or_else(|| anyhow::anyhow!("Invalid browser tabs"))?
                    .iter()
                    .filter(|t| t["pinned"] != true)
                    .collect();
                let items = if tabs_mode {
                    tabs.clone()
                } else {
                    vec![window]
                };
                for item in items {
                    let searchable = if tabs_mode {
                        s(&item["title"]).to_owned()
                    } else {
                        std::iter::once(title)
                            .chain(tabs.iter().map(|t| s(&t["title"])))
                            .collect::<Vec<_>>()
                            .join(" ")
                    };
                    if !folded(&searchable).contains(folded(term).trim()) {
                        continue;
                    }
                    let url = if tabs_mode { s(&item["url"]) } else { "" };
                    let domain = url::Url::parse(url)
                        .ok()
                        .and_then(|u| u.host_str().map(str::to_owned))
                        .unwrap_or_default();
                    let mut status = vec![];
                    if !domain.is_empty() {
                        status.push(domain);
                    }
                    if item["muted"] == true {
                        status.push("Muted".into());
                    } else if item["audible"] == true {
                        status.push("Playing audio".into());
                    }
                    if item["discarded"] == true {
                        status.push("Unloaded".into());
                    }
                    let mut favicon = s(&item["favicon"]).to_owned();
                    if favicon.starts_with("moz-remote-image://") {
                        favicon = url::Url::parse(&favicon)
                            .ok()
                            .and_then(|u| {
                                u.query_pairs()
                                    .find(|(k, _)| k == "url")
                                    .map(|(_, v)| v.to_string())
                            })
                            .unwrap_or_default();
                    }
                    if !(favicon.starts_with("https://") || favicon.starts_with("data:image/"))
                        || favicon.len() > 131072
                    {
                        favicon.clear();
                    }
                    rows.push(Entry{id:format!("Browser:{session}:{}:{}",if tabs_mode{"tab"}else{"window"},item["id"]),name:if s(&item["title"]).is_empty(){"Untitled".into()}else{s(&item["title"]).into()},subtitle:if tabs_mode{status.join(" · ")}else{format!("{} tabs",tabs.len())},icon:"zen-browser".into(),provider:"Browser".into(),keywords:String::new(),data:json!({"session":session,"window":window["id"],"url":url,"favicon":favicon,"keep_open":true,"tab":if tabs_mode{item["id"].clone()}else{Value::Null}})});
                }
            }
        }
        Ok(rows)
    }
    pub fn activate(&mut self, e: &Entry, mode: &str, demo: bool) -> Result<Value> {
        if mode == "switch" {
            if e.data["tab"].is_null() {
                self.scope = Some((
                    s(&e.data["session"]).into(),
                    e.data["window"].clone(),
                    e.name.clone(),
                ));
                return Ok(json!({"kind":"edit","text":"/tabs "}));
            }
            return Ok(
                json!({"kind":"message","message":"Read later is moving to Tasktjik; integration is not connected yet. Nothing was closed."}),
            );
        }
        let session = s(&e.data["session"]);
        if !demo
            && (std::path::Path::new(session)
                .file_name()
                .and_then(|s| s.to_str())
                != Some(session)
                || !session.ends_with(".sock"))
        {
            bail!("Invalid browser session");
        }
        let op = if mode == "close" { "close" } else { "focus" };
        let response = if demo {
            json!({})
        } else {
            let mut msg = json!({"op":op,"window":e.data["window"],"tab":e.data["tab"]});
            if mode == "close" {
                msg["url"] = e.data["url"].clone();
            }
            request(&directory()?.join(session), &msg)?
        };
        if mode == "close" {
            return Ok(
                json!({"refresh_query":if e.data["tab"].is_null(){"/browser"}else{"/tabs"},"message":if e.data["tab"].is_null(){response["message"].as_str().unwrap_or("Hyprland window close requested")}else{"Tab closed"}}),
            );
        }
        Ok(json!({"kind":"activated","demo":demo}))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn demo_scope_and_filter() {
        let mut b = Browser::default();
        let windows = b.query("/browser", true).unwrap();
        assert_eq!(windows.len(), 2);
        assert_eq!(b.query("/tabs", true).unwrap().len(), 3);
        b.activate(&windows[0], "switch", true).unwrap();
        assert_eq!(b.query("/tabs", true).unwrap().len(), 2);
        assert_eq!(
            b.query("/tabs Hyprland", true).unwrap()[0].name,
            "Hyprland window management"
        );
    }
}
