use crate::{providers::fields, util::*};
use anyhow::{ensure, Result};
use serde_json::{json, Value};
async fn nm(args: &[&str], password: Option<&str>) -> Result<String> {
    let mut a = vec![
        "nmcli".into(),
        "--colors".into(),
        "no".into(),
        "--wait".into(),
        "25".into(),
    ];
    a.extend(args.iter().map(|s| s.to_string()));
    let input = password.map(|p| format!("{p}\n"));
    command(
        &a,
        30.,
        1048576,
        &[("LC_ALL".into(), "C".into())],
        input.as_deref(),
    )
    .await
    .map_err(|_| anyhow::anyhow!("NetworkManager could not complete the request"))
}
async fn rows(columns: &str, args: &[&str]) -> Result<Vec<Vec<String>>> {
    let mut a = vec!["-t", "-f", columns];
    a.extend(args);
    Ok(nm(&a, None)
        .await?
        .lines()
        .filter(|l| !l.is_empty())
        .map(fields)
        .collect())
}
pub fn address_details(raw: &str) -> Value {
    let mut devices = json!({});
    let mut current = String::new();
    for line in raw.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        if key == "GENERAL.DEVICE" {
            current = value.into();
            devices[&current] = json!({"ip":[],"ipv6":[],"gateway":"","dns":[],"dhcp":""});
        } else if !current.is_empty() {
            let c = &mut devices[&current];
            let array = if key.starts_with("IP4.ADDRESS[") {
                Some("ip")
            } else if key.starts_with("IP6.ADDRESS[") {
                Some("ipv6")
            } else if key.starts_with("IP4.DNS[") {
                Some("dns")
            } else {
                None
            };
            if let Some(k) = array {
                c[k].as_array_mut().unwrap().push(json!(value));
            } else if key == "IP4.GATEWAY" {
                c["gateway"] = json!(value);
            } else if key.starts_with("DHCP4.OPTION[") {
                if let Some(("dhcp_server_identifier", address)) = value.split_once(" = ") {
                    c["dhcp"] = json!(address);
                }
            }
        }
    }
    devices
}
pub fn dns_routes(links: &Value) -> Value {
    let routes: Vec<_> = arr(links).iter().filter(|l| truth(&l["servers"])).collect();
    let catch = routes
        .iter()
        .any(|l| arr(&l["searchDomains"]).iter().any(|d| d["name"] == "."));
    let mut out = vec![];
    for l in routes {
        if if catch {
            arr(&l["searchDomains"]).iter().any(|d| d["name"] == ".")
        } else {
            truth(&l["defaultRoute"]) || !truth(&l["ifname"])
        } {
            for server in arr(&l["servers"]) {
                if truth(&server["addressString"]) && !out.contains(&server["addressString"]) {
                    out.push(server["addressString"].clone());
                }
            }
        }
    }
    json!(out)
}
pub async fn snapshot() -> Result<Value> {
    let mut devices = vec![];
    for r in rows("DEVICE,TYPE,STATE,CONNECTION", &["device", "status"]).await? {
        if r.len() == 4 && ["wifi", "ethernet"].contains(&r[1].as_str()) {
            devices.push(json!({"interface":r[0],"kind":r[1],"state":r[2],"connection":r[3]}));
        }
    }
    let enabled = nm(&["radio", "wifi"], None).await? == "enabled";
    let mut networks = std::collections::BTreeMap::<(String, String, String), Value>::new();
    if enabled && devices.iter().any(|d| d["kind"] == "wifi") {
        for r in rows(
            "IN-USE,SSID,BSSID,SIGNAL,SECURITY,DEVICE",
            &["device", "wifi", "list", "--rescan", "no"],
        )
        .await?
        {
            if r.len() != 6 || r[1].is_empty() {
                continue;
            }
            let signal = r[3].parse::<i64>().unwrap_or(0);
            let active = r[0] == "*";
            let key = (r[5].clone(), r[1].clone(), r[4].clone());
            let new = json!({"ssid":r[1],"bssid":r[2],"signal":signal,"security":if r[4]=="--"{""}else{&r[4]},"interface":r[5],"active":active});
            if networks.get(&key).is_none_or(|old| {
                (active, signal) > (truth(&old["active"]), old["signal"].as_i64().unwrap_or(0))
            }) {
                networks.insert(key, new);
            }
        }
    }
    let addresses = address_details(
        &nm(
            &[
                "-t",
                "-f",
                "GENERAL.DEVICE,IP4.ADDRESS,IP4.GATEWAY,IP4.DNS,DHCP4.OPTION,IP6.ADDRESS",
                "device",
                "show",
            ],
            None,
        )
        .await?,
    );
    for d in &mut devices {
        d["addresses"] = addresses
            .get(s(&d["interface"]))
            .cloned()
            .unwrap_or(json!({}));
    }
    let mut networks: Vec<_> = networks.into_values().collect();
    networks.sort_by(|a, b| {
        truth(&b["active"])
            .cmp(&truth(&a["active"]))
            .then(num(&b["signal"]).total_cmp(&num(&a["signal"])))
            .then(s(&a["ssid"]).cmp(s(&b["ssid"])))
    });
    let dns = jc(&["resolvectl", "--json=short", "status"])
        .await
        .map(|v| dns_routes(&v))
        .unwrap_or(json!([]));
    Ok(json!({"enabled":enabled,"devices":devices,"defaultDns":dns,"networks":networks}))
}
pub async fn execute(r: &Value) -> Result<Value> {
    match r["action"].as_str().unwrap_or("status") {
        "scan" => {
            nm(&["device", "wifi", "rescan"], None).await?;
        }
        "radio" => {
            ensure!(r["enabled"].is_boolean(), "A Wi-Fi radio state is required");
            nm(
                &[
                    "radio",
                    "wifi",
                    if truth(&r["enabled"]) { "on" } else { "off" },
                ],
                None,
            )
            .await?;
        }
        action @ ("connect" | "disconnect" | "ethernet") => {
            let interface = s(&r["interface"]);
            ensure!(
                !interface.is_empty() && !interface.starts_with('-') && !interface.contains('\0'),
                "Select a network interface"
            );
            if action == "connect" {
                let bssid = s(&r["bssid"]);
                ensure!(
                    regex::Regex::new(r"^(?:[0-9a-fA-F]{2}:){5}[0-9a-fA-F]{2}$")?.is_match(bssid),
                    "Select a visible Wi-Fi network and try again"
                );
                let password = s(&r["password"]);
                ensure!(
                    !password.contains(['\r', '\n', '\0']),
                    "The password must be a single line"
                );
                let mut args = vec![];
                if !password.is_empty() {
                    args.push("--ask");
                }
                args.extend(["device", "wifi", "connect", bssid, "ifname", interface]);
                nm(
                    &args,
                    if password.is_empty() {
                        None
                    } else {
                        Some(password)
                    },
                )
                .await?;
            } else {
                nm(
                    &[
                        "device",
                        if action == "disconnect" {
                            "disconnect"
                        } else {
                            "connect"
                        },
                        interface,
                    ],
                    None,
                )
                .await?;
            }
        }
        "status" => {}
        _ => anyhow::bail!("Unknown networking action"),
    };
    Ok(crate::config::merge(
        &snapshot().await?,
        &json!({"ok":true}),
    ))
}
pub fn counters(raw: &str) -> Value {
    let mut result = json!({});
    for line in raw.lines() {
        if let Some((interface, values)) = line.rsplit_once(':') {
            let values: Vec<_> = values.split_whitespace().collect();
            if values.len() >= 16 {
                if let (Ok(rx), Ok(tx)) = (values[0].parse::<u64>(), values[8].parse::<u64>()) {
                    result[interface.trim()] = json!({"rx":rx,"tx":tx});
                }
            }
        }
    }
    result
}
pub fn traffic(current: &Value, previous: &Value, elapsed: f64) -> Value {
    let mut result = current.clone();
    for (k, c) in obj(current) {
        let p = &previous[&k];
        let valid = p.is_object()
            && elapsed > 0.
            && ["rx", "tx"].iter().all(|key| num(&c[key]) >= num(&p[key]));
        for key in ["rx", "tx"] {
            result[&k][format!("{key}Rate")] = if valid {
                json!((num(&c[key]) - num(&p[key])) / elapsed)
            } else {
                Value::Null
            };
        }
    }
    result
}
pub async fn watch_traffic() -> Result<()> {
    let mut old = json!({});
    let mut stamp = now();
    loop {
        let c = counters(&std::fs::read_to_string("/proc/net/dev")?);
        let t = now();
        crate::emit(&traffic(&c, &old, t - stamp));
        old = c;
        stamp = t;
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
}
