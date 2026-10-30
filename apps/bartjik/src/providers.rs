use crate::{config, connectivity, details, util::*};
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
pub fn validate(rows: &[Value]) -> Result<()> {
    ensure!(rows.len() <= 256, "provider items must be at most 256");
    let mut ids = std::collections::HashSet::new();
    for row in rows {
        let id = s(&row["id"]);
        ensure!(
            !id.is_empty() && id.len() <= 256 && ids.insert(id),
            "items need unique stable IDs"
        );
        ensure!(
            ["icon", "text", "icon_text"].contains(&s(&row["type"])),
            "unknown item type"
        );
        for part in ["icon", "text"] {
            if row["type"] == part || row["type"] == "icon_text" {
                ensure!(
                    row[part]["value"]
                        .as_str()
                        .is_some_and(|v| v.len() <= 16384),
                    "invalid item part"
                );
                if part == "icon" {
                    ensure!(
                        ["glyph", "symbol", "theme", "image", "native"]
                            .contains(&s(&row[part]["kind"])),
                        "invalid icon kind"
                    );
                }
            }
        }
        ensure!(
            row.get("state").is_none_or(Value::is_object),
            "state must be an object"
        );
    }
    Ok(())
}
pub fn untrusted(mut payload: Value) -> Result<Vec<Value>> {
    if payload.is_object() && payload.get("text").is_some() && !payload["text"].is_object() {
        let plain = |v: &Value| {
            let raw = st(v);
            let re = regex::Regex::new("<[^>]*>").unwrap();
            let raw = re.replace_all(&raw, "");
            raw.replace("&amp;", "&")
                .replace("&lt;", "<")
                .replace("&gt;", ">")
                .replace("&quot;", "\"")
                .replace("&#39;", "'")
        };
        let classes = if payload["class"].is_string() {
            vec![payload["class"].clone()]
        } else {
            arr(&payload["class"]).to_vec()
        };
        let state: serde_json::Map<_, _> = classes.iter().map(|c| (st(c), json!(true))).collect();
        payload = json!([item(
            "value",
            Some(&plain(payload.get("text").unwrap_or(&json!("")))),
            None,
            "glyph",
            json!(state),
            &plain(payload.get("tooltip").unwrap_or(&json!(""))),
            json!({})
        )]);
    }
    let mut rows = payload
        .as_array()
        .context("provider items must be an array")?
        .clone();
    validate(&rows)?;
    for r in &mut rows {
        r.as_object_mut().unwrap().remove("actions");
        for p in ["icon", "text"] {
            if let Some(v) = r.get_mut(p).and_then(Value::as_object_mut) {
                v.remove("actions");
            }
        }
    }
    Ok(rows)
}
pub fn fields(line: &str) -> Vec<String> {
    let (mut values, mut v, mut escaped) = (vec![], String::new(), false);
    for c in line.chars() {
        if escaped {
            v.push(c);
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == ':' {
            values.push(std::mem::take(&mut v));
        } else {
            v.push(c)
        }
    }
    values.push(v);
    values
}
pub async fn audio_state() -> Result<(Value, Value)> {
    let sinks = jc(&["pactl", "-f", "json", "list", "sinks"]).await?;
    let default = cmd(&["pactl", "get-default-sink"]).await?;
    let current = arr(&sinks)
        .iter()
        .find(|v| v["name"] == default)
        .context("No default audio sink")?
        .clone();
    Ok((sinks, current))
}
pub fn headphones(sink: &Value) -> bool {
    sink["description"] == "USB Audio Speakers"
}
pub fn volume(sink: &Value) -> f64 {
    let channels = obj(&sink["volume"]);
    if channels.is_empty() {
        0.
    } else {
        (channels.values().map(|v| num(&v["value"])).sum::<f64>() / channels.len() as f64 / 65536.
            * 100.)
            .round_ties_even()
    }
}
pub async fn running(name: &str) -> Result<bool> {
    match cmd(&["pgrep", "-x", name]).await {
        Ok(_) => Ok(true),
        Err(e) if e.to_string().contains("exit 1") => Ok(false),
        Err(e) => Err(e),
    }
}
pub fn quota_api_rows(data: &Value) -> Result<Vec<Value>> {
    let mut rows = vec![];
    for a in arr(&data["accounts"]) {
        if a["status"] != "active" {
            continue;
        }
        let w = if a["windowMinutesPrimary"] == 10080 {
            "Primary"
        } else if a["windowMinutesSecondary"] == 10080 || a["windowMinutesSecondary"].is_null() {
            "Secondary"
        } else {
            continue;
        };
        let reset = a[format!("resetAt{w}")]
            .as_str()
            .map(chrono::DateTime::parse_from_rfc3339)
            .transpose()?
            .map(|d| d.timestamp());
        rows.push(json!({"label":if truth(&a["alias"]){a["alias"].clone()}else{a["email"].clone()},"plan_type":s(&a["planType"]).to_lowercase(),"window_name":"weekly","remaining_percent":a["usage"][format!("{}RemainingPercent",w.to_lowercase())],"reset_at":reset}));
    }
    Ok(rows)
}
pub fn cmc_config(m: &Value) -> Value {
    let ini = if let Some(p) = m["legacyConfig"].as_str() {
        details::ini(&std::fs::read_to_string(expand(p)).unwrap_or_default())
    } else {
        json!({})
    };
    let g = &ini["general"];
    let mut assets = m["assets"].clone();
    if assets.is_null() && truth(&ini) {
        assets=json!(obj(&ini).iter().filter(|(k,_)|k.as_str()!="general").map(|(k,v)|json!({"symbol":k.to_uppercase(),"icon":v["icon"].as_str().unwrap_or(k),"in_tooltip":(["true","1","yes","on"].contains(&s(&v["in_tooltip"]).to_lowercase().as_str())),"price_precision":s(&v["price_precision"]).parse::<usize>().unwrap_or(2),"change_precision":s(&v["change_precision"]).parse::<usize>().unwrap_or(2),"volume_precision":s(&v["volume_precision"]).parse::<usize>().unwrap_or(2)})).collect::<Vec<_>>());
    }
    if !truth(&assets) {
        assets = config::contract()["ASSETS"].clone();
    }
    json!({"assets":assets,"currency":m["currency"].as_str().or(g["currency"].as_str()).unwrap_or("USD").to_uppercase(),"currencySymbol":m["currencySymbol"].as_str().or(g["currency_symbol"].as_str()).unwrap_or(""),"display":m.get("display").cloned().unwrap_or_else(||json!(g["display"].as_str().unwrap_or("price").split(',').collect::<Vec<_>>())),"key":std::env::var(m["keyEnv"].as_str().unwrap_or("COINMARKETCAP_API_KEY")).ok().or_else(||g["api_key"].as_str().map(str::to_owned))})
}
pub fn crypto_items(data: &Value, c: &Value) -> Result<Vec<Value>> {
    ensure!(
        !truth(&data["status"]["error_code"]),
        "CoinMarketCap rejected request"
    );
    let mut result = vec![];
    let mut extra = vec![];
    for a in arr(&c["assets"]) {
        let symbol = s(&a["symbol"]).to_uppercase();
        let q = &data["data"][&symbol]["quote"][s(&c["currency"])];
        let mut labels = vec![];
        for f in arr(&c["display"]) {
            let f = s(f);
            let precision = a[if f.contains("volume") {
                "volume_precision"
            } else if f.contains("change") {
                "change_precision"
            } else {
                "price_precision"
            }]
            .as_u64()
            .unwrap_or(2)
            .min(12) as usize;
            let value = q[f].as_f64().context("invalid price")?;
            labels.push(if f == "price" {
                format!("{}{value:.precision$}", s(&c["currencySymbol"]))
            } else {
                format!(
                    "{}:{value:.precision$}{}",
                    f.replace("percent_change_", ""),
                    if f.contains("change") { "%" } else { "" }
                )
            });
        }
        let label = labels.join(" ");
        if truth(&a["in_tooltip"]) {
            extra.push(format!("{symbol} {label}"));
            continue;
        }
        let default_icon = icon(match symbol.as_str() {
            "BTC" => "bitcoin",
            "SOL" => "solana",
            "AR" => "arweave",
            _ => "coin",
        });
        let mut row = item(
            &format!("{symbol}/{}", s(&c["currency"])),
            Some(&label),
            Some(a["icon"].as_str().unwrap_or(&default_icon)),
            if a.get("icon").is_some() {
                "glyph"
            } else {
                "image"
            },
            json!({}),
            &format!(
                "{symbol}/{}\n24h: {:.2}%",
                s(&c["currency"]),
                num(&q["percent_change_24h"])
            ),
            json!({"leftClick":"crypto.panel"}),
        );
        row["changes"] = json!({"1h":q["percent_change_1h"],"24h":q["percent_change_24h"],"7d":q["percent_change_7d"]});
        result.push(row);
    }
    if !extra.is_empty() {
        for r in &mut result {
            r["tooltip"] = json!(format!("{}\n{}", s(&r["tooltip"]), extra.join("\n")));
        }
    }
    Ok(result)
}
pub async fn poll(m: &Value) -> Result<Vec<Value>> {
    let row = match s(&m["provider"]) {
        "battery" => return crate::battery::provider().await,
        "process" => {
            let raw = command(
                &config::argv(&m["command"])?,
                m["timeout"].as_f64().unwrap_or(5.),
                m["maxOutput"].as_u64().unwrap_or(262144) as usize,
                &[],
                None,
            )
            .await?;
            return untrusted(serde_json::from_str(&raw)?);
        }
        "crypto" => {
            let c = cmc_config(m);
            ensure!(truth(&c["key"]), "COINMARKETCAP_API_KEY is not set");
            let symbols = arr(&c["assets"])
                .iter()
                .map(|a| s(&a["symbol"]))
                .collect::<Vec<_>>()
                .join(",");
            let mut url = url::Url::parse(
                "https://pro-api.coinmarketcap.com/v1/cryptocurrency/quotes/latest",
            )?;
            url.query_pairs_mut()
                .append_pair("symbol", &symbols)
                .append_pair("convert", s(&c["currency"]));
            return crypto_items(
                &hj(
                    url.as_str(),
                    &[
                        ("X-CMC_PRO_API_KEY".into(), st(&c["key"])),
                        ("Accept".into(), "application/json".into()),
                    ],
                )
                .await?,
                &c,
            );
        }
        "network" => {
            let raw = cmd(&[
                "nmcli",
                "-t",
                "-f",
                "DEVICE,TYPE,STATE,CONNECTION",
                "device",
                "status",
            ])
            .await?;
            let rows: Vec<_> = raw
                .lines()
                .map(fields)
                .filter(|r| {
                    r.len() == 4
                        && (!truth(&m["interface"]) || r[0] == s(&m["interface"]))
                        && ["wifi", "ethernet"].contains(&r[1].as_str())
                })
                .collect();
            let r = rows
                .iter()
                .find(|r| r[2].starts_with("connected"))
                .or(rows.first())
                .context("No matching network interface")?;
            let connected = r[2].starts_with("connected");
            let mut ssid = r[3].clone();
            let mut signal = String::new();
            if connected && r[1] == "wifi" {
                if let Ok(wifi) = cmd(&[
                    "nmcli",
                    "-t",
                    "-f",
                    "ACTIVE,SSID,SIGNAL",
                    "device",
                    "wifi",
                    "list",
                    "ifname",
                    &r[0],
                    "--rescan",
                    "no",
                ])
                .await
                {
                    if let Some(a) = wifi
                        .lines()
                        .map(fields)
                        .find(|r| r.len() == 3 && r[0] == "yes")
                    {
                        ssid = a[1].clone();
                        signal = format!(" · {}%", a[2]);
                    }
                }
            }
            item(
                "connection",
                Some(if r[1] == "wifi" && connected {
                    &ssid
                } else if connected {
                    &r[0]
                } else {
                    "Offline"
                }),
                Some(if r[1] == "wifi" { "wifi" } else { "lan" }),
                "symbol",
                json!({"disconnected":!connected}),
                &format!("{} · {}\n{ssid}{signal}", r[0], r[2]),
                json!({"leftClick":"network.panel"}),
            )
        }
        "mullvad" => {
            let data = jc(&["mullvad", "status", "--json"]).await?;
            let state = s(&data["state"]);
            ensure!(!state.is_empty(), "Mullvad status unavailable");
            let connected = state == "connected";
            let p = &data["details"]["location"];
            let location = ["country", "city"]
                .iter()
                .filter_map(|k| p[k].as_str())
                .collect::<Vec<_>>()
                .join(", ");
            let flag = connected && m["regionFlag"] != false;
            let image = icon(&format!(
                "flags/{}",
                config::contract()["COUNTRY_FLAGS"][s(&p["country"])]
                    .as_str()
                    .unwrap_or("un")
            ));
            let mut row = item(
                "vpn",
                None,
                Some(if flag {
                    &image
                } else if connected {
                    "shield_lock"
                } else {
                    "shield"
                }),
                if flag { "image" } else { "symbol" },
                json!({"active":connected,"disconnected":state=="disconnected"}),
                &format!("Mullvad {state}\n{location}"),
                json!({"leftClick":{"command":m.get("gui").cloned().unwrap_or(json!(["/opt/Mullvad VPN/mullvad-gui"])),"detach":true},"rightClick":["mullvad","connect"],"middleClick":["mullvad","disconnect"]}),
            );
            if flag {
                row["icon"]["style"] = json!({"greyscaleIcons":false,"monochromeIcon":false});
            }
            let cat = if truth(&m["panel"]["hover"]) {
                connectivity::catalog().await
            } else {
                json!({})
            };
            row["panel"] = connectivity::mullvad_panel(
                state,
                &location,
                &cat,
                m.get("quickCountries")
                    .unwrap_or(&json!(["nl", "jp", "sg"])),
            );
            row
        }
        "tailscale" => {
            let data = jc(&["tailscale", "status", "--json"]).await?;
            let state = s(&data["BackendState"]);
            ensure!(!state.is_empty(), "Tailscale backend unavailable");
            let mut row = item(
                "tailnet",
                None,
                Some(m["symbol"].as_str().unwrap_or("vpn_lock")),
                "symbol",
                json!({"active":state=="Running","disconnected":state!="Running"}),
                &format!(
                    "Tailscale {}",
                    match state {
                        "Running" => "connected",
                        "NeedsLogin" => "needs login",
                        "Stopped" => "disconnected",
                        _ => state,
                    }
                ),
                json!({"leftClick":"tailscale.toggle"}),
            );
            row["panel"] = connectivity::tailscale_panel(&data);
            row
        }
        "audio" => {
            let (_, sink) = audio_state().await?;
            let muted = truth(&sink["mute"]);
            let mut row = item(
                "master",
                Some(&if muted {
                    "-%".into()
                } else {
                    format!("{}%", volume(&sink))
                }),
                Some(if headphones(&sink) {
                    "headphones"
                } else {
                    "volume_up"
                }),
                "symbol",
                json!({"muted":muted}),
                sink["description"].as_str().unwrap_or(s(&sink["name"])),
                json!({"scrollUp":"audio.up","scrollDown":"audio.down","middleClick":"audio.mute","hoverEnter":"audio.panel"}),
            );
            row["icon"]["actions"] = json!({"leftClick":"audio.route"});
            row["text"]["actions"] = json!({"leftClick":"audio.mute"});
            row
        }
        "datetime" => {
            let mut env = vec![];
            for (k, name) in [("timezone", "TZ"), ("locale", "LC_TIME")] {
                if let Some(v) = m[k].as_str() {
                    env.push((name.into(), v.into()));
                }
            }
            let raw = command(
                &[
                    "date".into(),
                    format!(
                        "+{}\n{}\n{}",
                        m["format"].as_str().unwrap_or("%H:%M"),
                        m["alternate"].as_str().unwrap_or("%a %d %b"),
                        m["tooltipFormat"].as_str().unwrap_or("%A, %d %B %Y")
                    ),
                ],
                5.,
                262144,
                &env,
                None,
            )
            .await?;
            let values: Vec<_> = raw.lines().collect();
            ensure!(values.len() == 3, "date formats must produce one line each");
            let mut row = item(
                "time",
                Some(values[0]),
                None,
                "glyph",
                json!({}),
                values[2],
                json!({"leftClick":"datetime.toggle"}),
            );
            row["alternate"] = json!(values[1]);
            row["panel"] = details::world_clock(&m["worldClocks"], None);
            row
        }
        "locale" => {
            let current = cmd(&["fcitx5-remote", "--check", "-n"]).await?;
            ensure!(!current.is_empty(), "Fcitx 5 unavailable");
            let method = &config::contract()["METHODS"][&current];
            let (label, tip) = (
                method[0].as_str().unwrap_or(&current),
                method[1].as_str().unwrap_or(&current),
            );
            let mut row = item(
                "method",
                Some(label),
                None,
                "glyph",
                json!({}),
                tip,
                json!({"leftClick":"locale.cycle","rightClick":{"command":["fcitx5-configtool"],"detach":true}}),
            );
            row["panel"] = if let Ok(group) = cmd(&["fcitx5-remote", "--check", "-q"]).await {
                let methods = details::methods(&group);
                let mut rows: Vec<_> = methods
                    .iter()
                    .map(|m| {
                        let names = &config::contract()["METHODS"][m];
                        config::merge(
                            &field(
                                names[0].as_str().unwrap_or(m),
                                json!(if *m == current { "Active" } else { "" }),
                                names[1].as_str().unwrap_or(m),
                            ),
                            &json!({"id":m,"active":*m==current}),
                        )
                    })
                    .collect();
                if rows.is_empty() {
                    rows.push(field(
                        "Available methods",
                        json!("Unavailable"),
                        "Active group is missing from the Fcitx profile",
                    ));
                }
                panel(
                    "Input methods",
                    json!(rows),
                    &format!("Fcitx group · {group}"),
                )
            } else {
                panel(
                    "Input methods",
                    json!([field(label, json!("Active"), tip)]),
                    "Group list unavailable",
                )
            };
            row
        }
        "codex" => {
            let (rows, summary) = if m["source"] == "api" {
                (
                    quota_api_rows(
                        &hj(
                            m["url"]
                                .as_str()
                                .unwrap_or("http://127.0.0.1:2455/api/accounts"),
                            &[],
                        )
                        .await?,
                    )?,
                    vec![field("Request totals", json!("Local database only"), "")],
                )
            } else {
                let p = expand(m["database"].as_str().unwrap_or("~/.codex-lb/store.db"));
                ensure!(p.exists(), "Codex quota database unavailable");
                tokio::task::spawn_blocking(move || details::query_quota(&p)).await??
            };
            details::quota(&rows, summary)
        }
        "openrouter" => return openrouter(m).await,
        "DND" => {
            let data = jc(&[m["ctl"].as_str().unwrap_or("notjikctl"), "status"]).await?;
            ensure!(
                data["ok"] == true && data["paused"].is_boolean(),
                "notjik unavailable"
            );
            let paused = truth(&data["paused"]);
            item(
                "dnd",
                None,
                Some(if paused {
                    "notifications_off"
                } else {
                    "notifications"
                }),
                "symbol",
                json!({"active":paused}),
                if paused {
                    "Notifications : OFF"
                } else {
                    "Notifications : ON"
                },
                json!({"leftClick":"DND.toggle"}),
            )
        }
        "caffeine" => {
            let active = !running("hypridle").await?;
            item(
                "idle",
                None,
                Some(if active { "coffee" } else { "hotel" }),
                "symbol",
                json!({"active":active}),
                if active {
                    "Idle Inhibit : ON"
                } else {
                    "Idle Inhibit : OFF"
                },
                json!({"leftClick":"caffeine.toggle"}),
            )
        }
        "nightlight" => {
            let active = running("hyprsunset").await?;
            item(
                "filter",
                None,
                Some(if active { "nightlight" } else { "light_mode" }),
                "symbol",
                json!({"active":active}),
                if active {
                    "Nightshift : ON"
                } else {
                    "Nightshift : OFF"
                },
                json!({"leftClick":"nightlight.toggle"}),
            )
        }
        "monitor-layout" => {
            let rows = jc(&["hyprctl", "-j", "monitors", "all"]).await?;
            let active = arr(&rows).iter().any(|r| {
                r["name"] == m["tv"].as_str().unwrap_or("HDMI-A-1") && !truth(&r["disabled"])
            });
            item(
                "layout",
                None,
                Some(if active {
                    "sports_esports"
                } else {
                    "desktop_windows"
                }),
                "symbol",
                json!({"active":active}),
                if active { "Game Mode" } else { "Desktop Mode" },
                json!({"leftClick":"monitor.toggle"}),
            )
        }
        _ => anyhow::bail!("unknown provider"),
    };
    Ok(vec![row])
}
async fn openrouter(m: &Value) -> Result<Vec<Value>> {
    let key = std::env::var(m["keyEnv"].as_str().unwrap_or("OPENROUTER_API_KEY"))
        .context("OPENROUTER_API_KEY is not set")?;
    let headers = vec![
        ("Authorization".into(), format!("Bearer {key}")),
        ("Accept".into(), "application/json".into()),
    ];
    let credits = hj("https://openrouter.ai/api/v1/credits", &headers).await;
    let (mut source, mut remaining) = (false, None);
    match credits {
        Err(ref e) if e.to_string().contains("HTTP 429") => anyhow::bail!(e.to_string()),
        Ok(ref c) => {
            if let (Some(total), Some(used)) = (
                c["data"]["total_credits"].as_f64(),
                c["data"]["total_usage"].as_f64(),
            ) {
                remaining = Some((total - used).max(0.));
            }
        }
        _ => {}
    }
    if remaining.is_none() {
        let key = hj("https://openrouter.ai/api/v1/key", &headers).await?;
        remaining = key["data"]["limit_remaining"].as_f64().map(|n| n.max(0.));
        source = true;
    }
    let remaining = remaining.context("credits unavailable")?;
    let activity = hj("https://openrouter.ai/api/v1/activity", &headers).await;
    let mut daily = std::collections::BTreeMap::<String, i64>::new();
    let mut status = "available";
    let mut error = String::new();
    match activity {
        Ok(v) => {
            if let Some(entries) = v["data"].as_array() {
                for entry in entries {
                    if !entry["date"].is_string()
                        || ["prompt_tokens", "completion_tokens", "reasoning_tokens"]
                            .iter()
                            .any(|k| {
                                !entry[k].is_null() && entry[k].as_f64().is_none_or(|v| v < 0.)
                            })
                    {
                        status = "unavailable";
                        error = "invalid activity response".into();
                        daily.clear();
                        break;
                    }
                    let tokens = ["prompt_tokens", "completion_tokens", "reasoning_tokens"]
                        .iter()
                        .map(|k| num(&entry[k]) as i64)
                        .sum::<i64>();
                    *daily.entry(st(&entry["date"])).or_default() += tokens;
                }
            } else {
                status = "unavailable";
                error = "invalid activity response".into();
            }
        }
        Err(e) => {
            let text = e.to_string();
            if text.contains("HTTP 429") {
                return Err(e);
            }
            status = if text == "HTTP 401" || text == "HTTP 403" {
                "restricted"
            } else {
                "unavailable"
            };
            error = text;
        }
    }
    let balance = details::grouped(remaining, 2);
    let mut lines = vec![format!(
        "OpenRouter credits remaining{}: ${balance}",
        if source { " (key limit)" } else { "" }
    )];
    if status == "available" {
        lines.push("Token usage per UTC day (last 30 completed days):".into());
        if daily.is_empty() {
            lines.push("No token activity".into());
        }
        for (date, tokens) in daily.iter().rev() {
            lines.push(format!(
                "{date}  {} tokens",
                details::grouped(*tokens as f64, 0)
            ));
        }
    } else {
        lines.push(format!("Daily token usage unavailable ({error})"));
    }
    let mut row = item(
        "credit",
        Some(&format!("{}{balance}", s(&m["currencySymbol"]))),
        Some(&icon("openrouter")),
        "image",
        json!({}),
        &lines.join("\n"),
        json!({}),
    );
    let rows: Vec<_> = daily
        .iter()
        .rev()
        .take(30)
        .map(|(date, t)| {
            config::merge(
                &field(date, json!(details::grouped(*t as f64, 0)), ""),
                &json!({"id":date,"tokens":t}),
            )
        })
        .collect();
    row["panel"] = config::merge(
        &panel("OpenRouter", json!(rows), ""),
        &json!({"balance":balance,"balanceLabel":if source{"Key spending limit remaining"}else{"Credits remaining"},"activityStatus":status,"activityNote":if status=="restricted"{"Daily usage isn't available for this key."}else if status!="available"{"Daily usage is temporarily unavailable."}else if rows.is_empty(){"No token activity in this reporting period."}else{"Last 30 completed days · UTC"}}),
    );
    Ok(vec![row])
}
