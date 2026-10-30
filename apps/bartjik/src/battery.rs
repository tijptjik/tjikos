use crate::util::*;
use anyhow::{ensure, Result};
use serde_json::{json, Value};
use std::path::Path;
const SERVICE: &str = "org.freedesktop.UPower.PowerProfiles";
const PATH: &str = "/org/freedesktop/UPower/PowerProfiles";
pub const PROFILES: [(&str, &str); 3] = [
    ("performance", "Performance"),
    ("balanced", "Balanced"),
    ("power-saver", "Power Saver"),
];
fn text(p: &Path, k: &str) -> String {
    std::fs::read_to_string(p.join(k))
        .unwrap_or_default()
        .trim()
        .into()
}
fn number(p: &Path, k: &str) -> Option<f64> {
    text(p, k)
        .parse::<f64>()
        .ok()
        .filter(|n| n.is_finite() && *n >= 0.)
}
pub fn row(percent: f64, external: bool) -> Value {
    let mut r = item(
        "battery",
        None,
        Some(if external { "bolt" } else { "battery_grid" }),
        "symbol",
        json!({}),
        &format!(
            "{} · {:.0}%",
            if external { "AC power" } else { "Battery" },
            percent
        ),
        json!({"leftClick":"battery.panel"}),
    );
    r["battery"] = json!({"percent":percent,"external":external});
    r
}
pub fn snapshot(root: &Path) -> Vec<Value> {
    let supplies: Vec<_> = std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .collect();
    let batteries: Vec<_> = supplies
        .iter()
        .filter(|p| {
            text(p, "type") == "Battery"
                && text(p, "scope") != "Device"
                && text(p, "present") != "0"
        })
        .collect();
    if batteries.is_empty() {
        return vec![];
    }
    let mut capacities = vec![];
    let mut energies = vec![];
    for p in &batteries {
        let mut now = number(p, "energy_now");
        let mut full = number(p, "energy_full").filter(|n| *n > 0.);
        if now.is_none() || full.is_none() {
            now = number(p, "charge_now");
            full = number(p, "charge_full").filter(|n| *n > 0.);
            if let (Some(n), Some(f), Some(v)) = (
                now,
                full,
                number(p, "voltage_min_design").filter(|n| *n > 0.),
            ) {
                now = Some(n * v / 1e6);
                full = Some(f * v / 1e6);
            } else {
                if let Some(cap) = number(p, "capacity").or_else(|| Some(100. * now? / full?)) {
                    capacities.push(cap);
                }
                continue;
            }
        }
        if let (Some(n), Some(f)) = (now, full) {
            capacities.push(100. * n / f);
            energies.push((n, f));
        }
    }
    if capacities.is_empty() {
        return vec![];
    }
    let percent = if energies.len() == batteries.len() {
        100. * energies.iter().map(|(n, _)| n).sum::<f64>()
            / energies.iter().map(|(_, f)| f).sum::<f64>()
    } else {
        capacities.iter().sum::<f64>() / capacities.len() as f64
    };
    let online: Vec<_> = supplies
        .iter()
        .filter(|p| text(p, "type") != "Battery" && text(p, "scope") != "Device")
        .map(|p| text(p, "online"))
        .filter(|v| v == "0" || v == "1")
        .collect();
    let external = if online.is_empty() {
        batteries.iter().any(|p| text(p, "status") == "Charging")
    } else {
        online.iter().any(|v| v == "1")
    };
    vec![row(percent.clamp(0., 100.), external)]
}
pub async fn properties(service: &str, path: &str, interface: &str) -> Result<Value> {
    let v = jc(&[
        "busctl",
        "--system",
        "--json=short",
        "call",
        service,
        path,
        "org.freedesktop.DBus.Properties",
        "GetAll",
        "s",
        interface,
    ])
    .await?;
    Ok(json!(obj(&v["data"][0])
        .into_iter()
        .map(|(k, v)| (k, v["data"].clone()))
        .collect::<serde_json::Map<_, _>>()))
}
pub async fn profiles() -> Value {
    properties(SERVICE, PATH, SERVICE)
        .await
        .unwrap_or(json!({}))
}
pub fn panel_data(b: &Value, e: &Value, p: &Value) -> Value {
    let external = truth(&b["external"]);
    let state = num(&e["State"]);
    let seconds = &e[if external {
        "TimeToFull"
    } else {
        "TimeToEmpty"
    }];
    let valid = state == if external { 1. } else { 2. } && num(seconds) > 0.;
    let status = if !external {
        "On battery"
    } else if state == 4. || num(&b["percent"]) >= 100. {
        "Fully charged"
    } else if state == 1. {
        "Charging"
    } else if state == 3. || state == 5. {
        "Charging paused"
    } else {
        "AC power"
    };
    let available: Vec<_> = arr(&p["Profiles"])
        .iter()
        .map(|p| s(&p["Profile"]["data"]))
        .collect();
    json!({"title":"Battery","percent":b["percent"],"external":external,"status":status,"seconds":if valid{seconds.clone()}else{Value::Null},"estimateLabel":if external{"Until fully charged"}else{"Battery life remaining"},"estimateUnavailable":if status=="Fully charged"{"Fully charged"}else if status=="Charging paused"&&truth(e){"Charging paused"}else{"Estimating…"},"profiles":PROFILES.iter().map(|(k,l)|json!({"id":k,"label":l,"available":available.contains(k),"active":p["ActiveProfile"]==*k})).collect::<Vec<_>>(),"profileMessage":if available.is_empty(){"Power modes unavailable".into()}else if truth(&p["PerformanceDegraded"]){format!("Performance limited: {}",s(&p["PerformanceDegraded"]))}else{String::new()}})
}
pub async fn provider() -> Result<Vec<Value>> {
    let mut rows = snapshot(Path::new("/sys/class/power_supply"));
    if !rows.is_empty() {
        let (e, p) = tokio::join!(
            properties(
                "org.freedesktop.UPower",
                "/org/freedesktop/UPower/devices/DisplayDevice",
                "org.freedesktop.UPower.Device"
            ),
            profiles()
        );
        rows[0]["panel"] = panel_data(&rows[0]["battery"], &e.unwrap_or(json!({})), &p);
    }
    Ok(rows)
}
pub async fn select(action: &str, selection: &str) -> Result<String> {
    ensure!(
        action == "profile" && PROFILES.iter().any(|(k, _)| *k == selection),
        "Unknown power mode"
    );
    let p = profiles().await;
    ensure!(
        arr(&p["Profiles"])
            .iter()
            .any(|p| p["Profile"]["data"] == selection),
        "This power mode is unavailable"
    );
    cmd(&[
        "busctl",
        "--system",
        "set-property",
        SERVICE,
        PATH,
        SERVICE,
        "ActiveProfile",
        "s",
        selection,
    ])
    .await?;
    ensure!(
        profiles().await["ActiveProfile"] == selection,
        "The power mode did not change"
    );
    Ok(format!(
        "{} selected",
        PROFILES.iter().find(|(k, _)| *k == selection).unwrap().1
    ))
}
pub struct Alert {
    alerted: bool,
    retry: f64,
    path: std::path::PathBuf,
}
impl Alert {
    pub fn new() -> Self {
        let path = xdg("XDG_STATE_HOME", ".local/state").join("bartjik/battery-alert.json");
        let alerted = read(&path).map(|v| v["alerted"] == true).unwrap_or(false);
        Self {
            alerted,
            retry: 0.,
            path,
        }
    }
    pub async fn update(&mut self, rows: &[Value]) {
        let Some(b) = rows.iter().find_map(|r| r.get("battery")) else {
            return;
        };
        let Some(p) = b["percent"]
            .as_f64()
            .filter(|p| p.is_finite() && (0. ..=100.).contains(p))
        else {
            return;
        };
        if b["external"] == true || p >= 7. {
            if self.alerted {
                self.alerted = false;
                let _ = save(&self.path, &json!({"alerted":false}));
            }
            self.retry = 0.;
            return;
        }
        if p > 5. || self.alerted || now() < self.retry {
            return;
        }
        if cmd(&[
            "notjikctl",
            "send",
            "Battery critically low",
            &format!("{}% remaining. Connect to power now.", p.ceil()),
            "--app",
            "Bartjik",
            "--id",
            "battery-critical",
            "--urgency",
            "critical",
            "--timeout",
            "never",
            "--icon",
            "battery-caution-symbolic",
        ])
        .await
        .is_ok()
        {
            self.alerted = true;
            let _ = save(&self.path, &json!({"alerted":true}));
        } else {
            self.retry = now() + 30.;
            eprintln!("Critical battery notification unavailable; retrying in 30 seconds");
        }
    }
}
