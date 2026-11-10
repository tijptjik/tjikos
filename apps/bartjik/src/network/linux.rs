use super::{Action, Adapter, Capabilities, Error, Reply, Snapshot, Traffic};
use crate::util::*;
use anyhow::{ensure, Result};
use serde_json::{json, Value};
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
// Keep credentials on stdin, bound both output streams, and never return raw tool errors.
async fn nm(args: &[&str], password: Option<&str>) -> Result<String> {
    use std::process::Stdio;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    struct Child(tokio::process::Child);
    impl Drop for Child {
        fn drop(&mut self) {
            if let Some(pid) = self.0.id() {
                unsafe {
                    libc::kill(-(pid as i32), libc::SIGKILL);
                }
            }
        }
    }
    let mut command = tokio::process::Command::new("nmcli");
    command
        .args(["--colors", "no", "--wait", "25"])
        .args(args)
        .env("LC_ALL", "C")
        .stdin(if password.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .process_group(0);
    let parent = std::process::id();
    unsafe {
        command.pre_exec(move || {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::getppid() as u32 != parent {
                return Err(std::io::Error::other("network owner exited"));
            }
            Ok(())
        });
    }
    let mut child = Child(
        command
            .spawn()
            .map_err(|_| anyhow::anyhow!("executable unavailable"))?,
    );
    let mut output = child.0.stdout.take().unwrap();
    let mut errors = child.0.stderr.take().unwrap();
    let input = child.0.stdin.take();
    let work = async {
        let write = async move {
            if let (Some(mut pipe), Some(password)) = (input, password) {
                pipe.write_all(password.as_bytes()).await?;
                pipe.write_all(b"\n").await?;
                pipe.shutdown().await?;
            }
            Ok::<_, anyhow::Error>(())
        };
        let read = async {
            let mut data = vec![];
            (&mut output).take(1_048_577).read_to_end(&mut data).await?;
            ensure!(data.len() <= 1_048_576, "network output too large");
            Ok::<_, anyhow::Error>(data)
        };
        let error = async {
            let mut data = vec![];
            (&mut errors).take(16_385).read_to_end(&mut data).await?;
            ensure!(data.len() <= 16_384, "network error too large");
            Ok::<_, anyhow::Error>(data)
        };
        let (_, data, error) = tokio::try_join!(write, read, error)?;
        let status = child.0.wait().await?;
        if !status.success() {
            let error = String::from_utf8_lossy(&error).to_lowercase();
            if [
                "not authorized",
                "not authorised",
                "permission denied",
                "insufficient privileges",
            ]
            .iter()
            .any(|s| error.contains(s))
            {
                anyhow::bail!("network permission denied");
            }
            anyhow::bail!("nmcli failed (exit {})", status.code().unwrap_or(-1));
        }
        Ok(String::from_utf8_lossy(&data).trim().to_owned())
    };
    tokio::time::timeout(std::time::Duration::from_secs(30), work)
        .await
        .map_err(|_| anyhow::anyhow!("network request timed out"))?
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

fn classify(error: anyhow::Error) -> Error {
    let message = format!("{error:#}");
    if message.contains("timed out") || message.contains("exit 3") {
        Error::Timeout
    } else if message.contains("permission denied") {
        Error::PermissionDenied
    } else if message.contains("exit 10") {
        Error::StaleTarget
    } else if message.contains("executable unavailable")
        || message.contains("exit 8")
        || message.contains("exit 9")
    {
        Error::Unavailable
    } else {
        Error::Failed
    }
}
pub(super) struct NetworkManager;
impl Adapter for NetworkManager {
    fn subscribe(
        &self,
        wake: std::sync::Arc<tokio::sync::Notify>,
    ) -> Option<crate::status_events::Subscription> {
        Some(crate::status_events::Subscription::start(
            "nmcli",
            &["monitor"],
            |_| true,
            wake,
        ))
    }
    fn name(&self) -> &'static str {
        "network-manager-cli"
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            wifi: true,
            ethernet: true,
            traffic: true,
        }
    }
    fn snapshot(&self) -> Reply<'_, Snapshot> {
        Box::pin(async {
            serde_json::from_value(
                tokio::time::timeout(std::time::Duration::from_secs(35), snapshot())
                    .await
                    .map_err(|_| Error::Timeout)?
                    .map_err(classify)?,
            )
            .map_err(|_| Error::Failed)
        })
    }
    fn execute(&self, action: Action) -> Reply<'_, Snapshot> {
        Box::pin(async move {
            let value = serde_json::to_value(action).map_err(|_| Error::Failed)?;
            serde_json::from_value(
                tokio::time::timeout(std::time::Duration::from_secs(60), execute(&value))
                    .await
                    .map_err(|_| Error::Timeout)?
                    .map_err(classify)?,
            )
            .map_err(|_| Error::Failed)
        })
    }
    fn counters(&self) -> Reply<'_, Traffic> {
        Box::pin(async {
            let text = tokio::fs::read_to_string("/proc/net/dev")
                .await
                .map_err(|_| Error::Unavailable)?;
            serde_json::from_value(counters(&text)).map_err(|_| Error::Failed)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tool_errors_are_classified_without_exposing_output() {
        assert_eq!(
            classify(anyhow::anyhow!("nmcli failed (exit 8)")),
            Error::Unavailable
        );
        assert_eq!(
            classify(anyhow::anyhow!("nmcli failed (exit 3)")),
            Error::Timeout
        );
        assert_eq!(
            classify(anyhow::anyhow!("network permission denied")),
            Error::PermissionDenied
        );
        assert_eq!(
            classify(anyhow::anyhow!("nmcli failed (exit 10)")),
            Error::StaleTarget
        );
    }
    #[test]
    fn escaped_access_points_addresses_and_dns_routes() {
        assert_eq!(
            fields(r"*:Cafe\:guest:02\:00\:00\:00\:00\:01:80:WPA2:wlan0"),
            vec![
                "*",
                "Cafe:guest",
                "02:00:00:00:00:01",
                "80",
                "WPA2",
                "wlan0"
            ]
        );
        let addresses = address_details("GENERAL.DEVICE:eth0\nIP4.ADDRESS[1]:192.0.2.2/24\nIP4.GATEWAY:192.0.2.1\nDHCP4.OPTION[1]:dhcp_server_identifier = 192.0.2.1");
        assert_eq!(addresses["eth0"]["ip"][0], "192.0.2.2/24");
        assert_eq!(addresses["eth0"]["dhcp"], "192.0.2.1");
        assert_eq!(
            dns_routes(
                &json!([{ "ifname":"vpn", "searchDomains":[{"name":"."}],"servers":[{"addressString":"192.0.2.53"}] }, {"ifname":"eth0","defaultRoute":true,"servers":[{"addressString":"192.0.2.1"}]}])
            ),
            json!(["192.0.2.53"])
        );
    }
}
