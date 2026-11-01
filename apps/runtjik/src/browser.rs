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
pub fn session_path(session: &str) -> Result<PathBuf> {
    anyhow::ensure!(
        std::path::Path::new(session)
            .file_name()
            .and_then(|s| s.to_str())
            == Some(session)
            && session.ends_with(".sock"),
        "Invalid browser session"
    );
    Ok(directory()?.join(session))
}
// Compile once per query, not once per tab. Configuration is reloaded on opening.
pub fn favicon_overrides(value: &Value) -> Result<Vec<(regex::Regex, String)>> {
    let rules = value
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("Browser.favicon_overrides must be an array"))?;
    rules.iter().enumerate().map(|(index, rule)| {
        let invalid = || anyhow::anyhow!("Invalid Browser.favicon_overrides rule {}: expected domain_regex and icon strings", index + 1);
        let fields = rule.as_object().ok_or_else(invalid)?;
        let pattern = rule["domain_regex"].as_str().filter(|s| !s.is_empty()).ok_or_else(invalid)?;
        let icon = rule["icon"].as_str().filter(|s| !s.is_empty()).ok_or_else(invalid)?;
        if fields.len() != 2 { return Err(invalid()); }
        let regex = regex::RegexBuilder::new(pattern).case_insensitive(true).build()
            .map_err(|e| anyhow::anyhow!("Invalid Browser.favicon_overrides rule {} regex: {e}", index + 1))?;
        let path = if let Some(name) = icon.strip_prefix("bundled:") {
            if !["overture.png", "hkgov.png"].contains(&name) {
                bail!("Unknown bundled favicon: {name}");
            }
            Some(crate::paths::root().join("qml/icons/sites").join(name))
        } else if let Some(rest) = icon.strip_prefix("~/") {
            Some(home().join(rest))
        } else if std::path::Path::new(icon).is_absolute() {
            Some(PathBuf::from(icon))
        } else {
            None
        };
        let source = if let Some(path) = path {
            url::Url::from_file_path(path).map_err(|_| anyhow::anyhow!("Invalid favicon path"))?.to_string()
        } else if url::Url::parse(icon).is_ok_and(|u| u.scheme() == "https" && u.host_str().is_some()) {
            icon.to_owned()
        } else {
            bail!("Browser.favicon_overrides icon must be bundled:, an absolute path, ~/ path, or HTTPS URL");
        };
        Ok((regex, source))
    }).collect()
}
fn matching_favicon<'a>(rules: &'a [(regex::Regex, String)], domain: &str) -> Option<&'a str> {
    if domain.is_empty() {
        return None;
    }
    let domain = domain.trim_end_matches('.');
    rules
        .iter()
        .find(|(regex, _)| regex.is_match(domain))
        .map(|(_, icon)| icon.as_str())
}

#[derive(Default)]
pub struct Browser {
    pub scope: Option<(String, Value, String)>,
}
impl Browser {
    pub fn query(&self, q: &str, settings: &Value, demo: bool) -> Result<Vec<Entry>> {
        let (command, mut term) = q.split_once(' ').unwrap_or((q, ""));
        let history_mode = command == "/history";
        if history_mode && term.len() > 512 {
            bail!("History query is too long");
        }
        let tabs_mode = !["/browser", "/zen"].contains(&command);
        if !q.starts_with('/') {
            term = q;
        }
        let sessions = if demo {
            vec![(
                "demo".into(),
                if history_mode {
                    json!({"history":[{"id":"history-demo","title":"Previously visited documentation","url":"https://example.org/docs","lastVisitTime":1000,"visitCount":2}]})
                } else {
                    json!({"windows":[{"id":1,"title":"Research","tabs":[{"id":11,"title":"Firefox extension documentation","url":"https://example.org/firefox"},{"id":12,"title":"Hyprland window management","url":"https://example.org/hyprland"}]},{"id":2,"title":"Reading","tabs":[{"id":21,"title":"A synthetic reading list","url":"https://example.org/reading"}]}]})
                },
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
                match request(
                    &path,
                    &if history_mode {
                        json!({"op":"history","text":term})
                    } else {
                        json!({"op":"list"})
                    },
                ) {
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
                if history_mode {
                    if let Some(rows) = crate::offline_history::query(term)? {
                        return Ok(rows);
                    }
                } else if !q.starts_with('/') {
                    // A closed browser is normal; don't obscure application search.
                    return Ok(vec![]);
                }
                bail!("Browser bridge disconnected. Start Zen/Firefox with the signed Runtjik extension installed; offline history can be configured in Browser settings.");
            }
            sessions
        };
        let favicon_overrides =
            favicon_overrides(&settings["plugin_settings"]["Browser"]["favicon_overrides"])?;
        let mut rows = vec![];
        for (session, state) in sessions {
            if history_mode {
                for item in state["history"]
                    .as_array()
                    .ok_or_else(|| {
                        anyhow::anyhow!("Reload the updated Browser bridge to enable history")
                    })?
                    .iter()
                    .take(100)
                {
                    let url = s(&item["url"]);
                    if !url::Url::parse(url).is_ok_and(|u| ["http", "https"].contains(&u.scheme()))
                    {
                        continue;
                    }
                    let title = if s(&item["title"]).is_empty() {
                        url
                    } else {
                        s(&item["title"])
                    };
                    if demo && !folded(&format!("{title} {url}")).contains(folded(term).trim()) {
                        continue;
                    }
                    rows.push(Entry { id: format!("Browser:{session}:history:{}", item["id"]), name:title.into(), subtitle:format!("{} · {} visits", url, item["visitCount"].as_u64().unwrap_or(0)), icon:"document-open-recent".into(), provider:"Browser".into(), keywords:String::new(), data:json!({"session":session,"url":url,"history":true,"lastVisitTime":item["lastVisitTime"]}) });
                }
                continue;
            }
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
                        status.push(domain.clone());
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
                    if let Some(icon) = matching_favicon(&favicon_overrides, &domain) {
                        favicon = icon.to_owned();
                    }
                    rows.push(Entry{id:format!("Browser:{session}:{}:{}",if tabs_mode{"tab"}else{"window"},item["id"]),name:if s(&item["title"]).is_empty(){"Untitled".into()}else{s(&item["title"]).into()},subtitle:if tabs_mode{status.join(" · ")}else{format!("{} tabs",tabs.len())},icon:"zen-browser".into(),provider:"Browser".into(),keywords:String::new(),data:json!({"session":session,"window":window["id"],"url":url,"favicon":favicon,"keep_open":true,"tab":if tabs_mode{item["id"].clone()}else{Value::Null}})});
                }
            }
        }
        if history_mode {
            rows.sort_by(|a, b| {
                b.data["lastVisitTime"]
                    .as_f64()
                    .unwrap_or(0.0)
                    .total_cmp(&a.data["lastVisitTime"].as_f64().unwrap_or(0.0))
            });
            rows.truncate(100);
        }
        Ok(rows)
    }
    pub fn activate(&mut self, e: &Entry, mode: &str, demo: bool) -> Result<Value> {
        if e.data["offline_history"] == true && !demo {
            return crate::offline_history::activate(e, mode);
        }
        if e.data["history"] == true && mode == "switch" {
            return Ok(
                json!({"kind":"message","message":"Enter reopens this history item; Ctrl+Enter removes its URL from browser history."}),
            );
        }
        if mode == "switch" {
            if e.data["tab"].is_null() {
                self.scope = Some((
                    s(&e.data["session"]).into(),
                    e.data["window"].clone(),
                    e.name.clone(),
                ));
                return Ok(json!({"kind":"edit","text":"/tabs "}));
            }
            bail!("Open reading capture through Tasktjik");
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
        let op = if e.data["history"] == true {
            if mode == "close" {
                "delete-history"
            } else {
                "open-history"
            }
        } else if mode == "close" {
            "close"
        } else {
            "focus"
        };
        let response = if demo {
            json!({})
        } else {
            let mut msg = json!({"op":op,"window":e.data["window"],"tab":e.data["tab"]});
            if mode == "close" || e.data["history"] == true {
                msg["url"] = e.data["url"].clone();
            }
            request(&directory()?.join(session), &msg)?
        };
        if mode == "close" && e.data["history"] == true {
            return Ok(
                json!({"refresh_query":"/history","message":"URL removed from browser history"}),
            );
        }
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
    fn history_is_separate_from_tab_scope_and_deletes_on_close() {
        let mut b = Browser::default();
        let window = b
            .query("/browser", &crate::config::contract()["DEFAULTS"], true)
            .unwrap()
            .remove(0);
        b.activate(&window, "switch", true).unwrap();
        let rows = b
            .query(
                "/history example.org",
                &crate::config::contract()["DEFAULTS"],
                true,
            )
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].data["history"], true);
        assert_eq!(
            b.activate(&rows[0], "close", true).unwrap()["refresh_query"],
            "/history"
        );
        assert_eq!(
            b.activate(&rows[0], "normal", true).unwrap()["kind"],
            "activated"
        );
        assert!(b
            .query(
                "/history nonexistent",
                &crate::config::contract()["DEFAULTS"],
                true
            )
            .unwrap()
            .is_empty());
    }
    #[test]
    fn demo_scope_and_filter() {
        let mut b = Browser::default();
        let windows = b
            .query("/browser", &crate::config::contract()["DEFAULTS"], true)
            .unwrap();
        assert_eq!(windows.len(), 2);
        assert_eq!(
            b.query("/tabs", &crate::config::contract()["DEFAULTS"], true)
                .unwrap()
                .len(),
            3
        );
        b.activate(&windows[0], "switch", true).unwrap();
        assert_eq!(
            b.query("/tabs", &crate::config::contract()["DEFAULTS"], true)
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            b.query(
                "/tabs Hyprland",
                &crate::config::contract()["DEFAULTS"],
                true
            )
            .unwrap()[0]
                .name,
            "Hyprland window management"
        );
    }
}

#[cfg(test)]
mod favicon_tests {
    use super::*;

    #[test]
    fn defaults_match_domains_and_subdomains_only() {
        let settings = crate::config::parse("").unwrap();
        let rules = favicon_overrides(&settings["plugin_settings"]["Browser"]["favicon_overrides"])
            .unwrap();
        for domain in [
            "overturemaps.org",
            "docs.overturemaps.org",
            "DOCS.OVERTUREMAPS.ORG.",
        ] {
            assert!(matching_favicon(&rules, domain)
                .unwrap()
                .ends_with("/overture.png"));
        }
        assert!(matching_favicon(&rules, "portal.csdi.gov.hk")
            .unwrap()
            .ends_with("/hkgov.png"));
        for domain in [
            "",
            "example.org",
            "notoverturemaps.org",
            "overturemaps.org.example.org",
            "csdi.gov.hk.evil.org",
        ] {
            assert!(matching_favicon(&rules, domain).is_none());
        }
    }

    #[test]
    fn custom_rules_replace_defaults_and_first_match_wins() {
        let settings = crate::config::parse(
            r#"
[plugin_settings.Browser]
favicon_overrides = [
  { domain_regex = '^docs\.example\.org$', icon = '/tmp/icon with spaces.png' },
  { domain_regex = '(^|\.)example\.org$', icon = 'https://example.org/icon.png' }
]
"#,
        )
        .unwrap();
        let rules = favicon_overrides(&settings["plugin_settings"]["Browser"]["favicon_overrides"])
            .unwrap();
        assert_eq!(
            matching_favicon(&rules, "docs.example.org"),
            Some("file:///tmp/icon%20with%20spaces.png")
        );
        assert_eq!(
            matching_favicon(&rules, "example.org"),
            Some("https://example.org/icon.png")
        );
        assert!(matching_favicon(&rules, "docs.overturemaps.org").is_none());
        assert!(favicon_overrides(&json!([])).unwrap().is_empty());
    }

    #[test]
    fn tab_queries_use_overrides_but_windows_and_history_do_not() {
        let settings = crate::config::parse(
            r#"
[plugin_settings.Browser]
favicon_overrides = [{ domain_regex = '^example\.org$', icon = 'bundled:hkgov.png' }]
"#,
        )
        .unwrap();
        let browser = Browser::default();
        for query in ["/tabs", "Firefox"] {
            let rows = browser.query(query, &settings, true).unwrap();
            assert!(!rows.is_empty());
            assert!(rows
                .iter()
                .all(|row| s(&row.data["favicon"]).ends_with("/hkgov.png")));
        }
        for query in ["/browser", "/history"] {
            assert!(browser
                .query(query, &settings, true)
                .unwrap()
                .iter()
                .all(|row| s(&row.data["favicon"]).is_empty()));
        }
        let defaults = crate::config::parse("").unwrap();
        assert!(browser
            .query("/tabs", &defaults, true)
            .unwrap()
            .iter()
            .all(|row| s(&row.data["favicon"]).is_empty()));
    }

    #[test]
    fn invalid_rules_fail_configuration_validation() {
        for rule in [
            "{domain_regex='[', icon='bundled:overture.png'}",
            "{domain_regex='', icon='bundled:overture.png'}",
            "{domain_regex='.*', icon='relative.png'}",
            "{domain_regex='.*', icon='bundled:../other.png'}",
            "{domain_regex='.*', icon='http://example.org/icon.png'}",
            "{domain_regex='.*', icon=12}",
            "{domain_regex='.*', icon='bundled:overture.png', typo=true}",
        ] {
            assert!(
                crate::config::parse(&format!(
                    "[plugin_settings.Browser]\nfavicon_overrides=[{rule}]\n"
                ))
                .is_err(),
                "{rule}"
            );
        }
    }
}
