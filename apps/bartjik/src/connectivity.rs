use crate::{config, util::*};
use anyhow::{ensure, Result};
use serde_json::{json, Value};
pub fn parse_relays(raw: &str) -> Result<Value> {
    let country = regex::Regex::new(r"^([^\t].+?) \(([a-z]{2})\)$")?;
    let city = regex::Regex::new(r"^\t([^\t].+?) \(([a-z]{3})\) @")?;
    let mut result = json!({});
    let mut current = String::new();
    for line in raw.lines() {
        if let Some(c) = country.captures(line) {
            current = c[2].into();
            result[&current] = json!({"id":current,"name":&c[1],"cities":[]});
        } else if let Some(c) = city.captures(line) {
            if !current.is_empty()
                && !arr(&result[&current]["cities"])
                    .iter()
                    .any(|x| x["id"] == c[2])
            {
                result[&current]["cities"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!({"id":&c[2],"name":&c[1]}));
            }
        }
    }
    ensure!(truth(&result), "Relay locations unavailable");
    Ok(result)
}
pub async fn catalog() -> Value {
    static CACHE: std::sync::OnceLock<tokio::sync::Mutex<(f64, Value)>> =
        std::sync::OnceLock::new();
    let mut c = CACHE
        .get_or_init(|| tokio::sync::Mutex::new((0., json!({}))))
        .lock()
        .await;
    if now() >= c.0 {
        match cmd(&["mullvad", "relay", "list"])
            .await
            .and_then(|s| parse_relays(&s))
        {
            Ok(v) => {
                c.1 = v;
                c.0 = now() + 3600.;
            }
            Err(_) => c.0 = now() + 60.,
        }
    }
    c.1.clone()
}
pub fn mullvad_panel(state: &str, location: &str, catalog: &Value, quick: &Value) -> Value {
    let parts: Vec<_> = location.split(',').map(str::trim).collect();
    let (country_name, city_name) = (
        parts.first().copied().unwrap_or(""),
        parts.get(1).copied().unwrap_or(""),
    );
    let countries = obj(catalog);
    let country = countries
        .values()
        .find(|v| v["name"] == country_name)
        .cloned()
        .unwrap_or(json!({}));
    let city = arr(&country["cities"])
        .iter()
        .find(|v| v["name"] == city_name)
        .cloned()
        .unwrap_or(json!({}));
    let title = state
        .chars()
        .next()
        .map(|c| c.to_uppercase().to_string() + &state[c.len_utf8()..])
        .unwrap_or_default();
    config::merge(
        &panel(
            "Mullvad",
            json!([
                field("Connection", json!(title), ""),
                field(
                    "Location",
                    json!(if location.is_empty() { "—" } else { location }),
                    ""
                )
            ]),
            "",
        ),
        &json!({"connection":state,"country":if country_name.is_empty(){"Choose a country"}else{country_name},"city":city_name,"countryCode":s(&country["id"]),"cityCode":s(&city["id"]),"cities":arr(&country["cities"]).iter().map(|v|config::merge(v,&json!({"active":v["id"]==city["id"]}))).collect::<Vec<_>>(),"countries":arr(quick).iter().filter_map(|code|countries.get(s(code)).map(|v|json!({"id":code,"name":v["name"],"active":*code==country["id"]}))).collect::<Vec<_>>(),"locationsAvailable":truth(catalog)}),
    )
}
pub fn tailscale_panel(data: &Value) -> Value {
    let own = &data["Self"];
    let mut peers:Vec<_>=obj(&data["Peer"]).into_iter().filter(|(_,p)|p.is_object()).map(|(k,p)|json!({"id":k,"name":p["HostName"].as_str().filter(|s|!s.is_empty()).unwrap_or("Peer"),"online":truth(&p["Online"]),"address":s(&p["TailscaleIPs"][0]),"os":s(&p["OS"])})).collect();
    peers.sort_by(|a, b| {
        truth(&b["online"])
            .cmp(&truth(&a["online"]))
            .then(s(&a["name"]).cmp(s(&b["name"])))
    });
    let online = peers.iter().filter(|p| truth(&p["online"])).count();
    let total = peers.len();
    let ips = if truth(&data["TailscaleIPs"]) {
        &data["TailscaleIPs"]
    } else {
        &own["TailscaleIPs"]
    };
    let device = own["HostName"]
        .as_str()
        .filter(|s| !s.is_empty())
        .unwrap_or("This device");
    let addresses = arr(ips).iter().map(s).collect::<Vec<_>>().join(", ");
    peers.truncate(32);
    config::merge(
        &panel(
            "Tailscale",
            json!([
                field("Backend", data["BackendState"].clone(), ""),
                field("This device", json!(device), ""),
                field(
                    "Addresses",
                    json!(if addresses.is_empty() {
                        "—"
                    } else {
                        &addresses
                    }),
                    ""
                ),
                field("Peers online", json!(format!("{online} / {total}")), "")
            ]),
            "",
        ),
        &json!({"connection":data["BackendState"],"device":device,"address":s(&ips[0]),"peers":peers,"online":online,"total":total}),
    )
}
pub async fn act(
    provider: &str,
    action: &str,
    selection: &Value,
    module: &Value,
) -> Result<String> {
    if provider == "mullvad" {
        if ["connect", "disconnect"].contains(&action) {
            command(
                &vec!["mullvad".into(), action.into()],
                10.,
                262144,
                &[],
                None,
            )
            .await?;
            return Ok(if action == "connect" {
                "Connection requested"
            } else {
                "Disconnect requested"
            }
            .into());
        }
        let cat = catalog().await;
        if action == "country" {
            let defaults = json!(["nl", "jp", "sg"]);
            ensure!(
                arr(module.get("quickCountries").unwrap_or(&defaults)).contains(selection)
                    && cat.get(s(selection)).is_some(),
                "This country is no longer available"
            );
            cmd(&["mullvad", "relay", "set", "location", s(selection)]).await?;
            return Ok(format!("Selected {}", s(&cat[s(selection)]["name"])));
        }
        if action == "city" {
            let c = &cat[s(&selection["country"])];
            let city = arr(&c["cities"])
                .iter()
                .find(|v| v["id"] == selection["city"]);
            ensure!(city.is_some(), "This city is no longer available");
            cmd(&[
                "mullvad",
                "relay",
                "set",
                "location",
                s(&c["id"]),
                s(&selection["city"]),
            ])
            .await?;
            return Ok(format!("Selected {}", s(&city.unwrap()["name"])));
        }
    } else if provider == "tailscale" {
        if action == "admin" {
            crate::actions::detach(
                &[
                    "xdg-open".into(),
                    "https://login.tailscale.com/admin/machines".into(),
                ],
                &json!({}),
            )?;
            return Ok("Opened devices in your browser".into());
        }
        let data = jc(&["tailscale", "status", "--json"]).await?;
        ensure!(truth(&data["BackendState"]), "Tailscale is unavailable");
        if action == "toggle" {
            command(
                &vec![
                    "tailscale".into(),
                    if data["BackendState"] == "Running" {
                        "down"
                    } else {
                        "up"
                    }
                    .into(),
                ],
                15.,
                262144,
                &[],
                None,
            )
            .await?;
            return Ok("Connection change requested".into());
        }
        if ["copy", "ping"].contains(&action) {
            let target = if selection == "self" {
                &data["Self"]
            } else {
                &data["Peer"][s(selection)]
            };
            ensure!(truth(target), "This device is no longer available");
            let addresses = if selection == "self" && truth(&data["TailscaleIPs"]) {
                &data["TailscaleIPs"]
            } else {
                &target["TailscaleIPs"]
            };
            let address = s(&addresses[0]).parse::<std::net::IpAddr>()?.to_string();
            if action == "copy" {
                command(&["wl-copy".into()], 5., 262144, &[], Some(&address)).await?;
                return Ok("Address copied".into());
            }
            ensure!(
                data["BackendState"] == "Running" && truth(&target["Online"]),
                "Connect both devices before checking reachability"
            );
            let reply = cmd(&["tailscale", "ping", "--c=1", "--timeout=3s", &address]).await?;
            let re = regex::Regex::new(r"\bin ([\d.]+\s?ms)\b")?;
            return Ok(format!(
                "Reachable{}",
                re.captures(&reply)
                    .map(|c| format!(" · {}", &c[1]))
                    .unwrap_or_default()
            ));
        }
    }
    anyhow::bail!("This control is unavailable")
}
