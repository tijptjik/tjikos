//! Pinned fleet SSH discovery and timing-transfer helpers.
use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::{
    fs,
    io::{Read, Write},
    net::{IpAddr, SocketAddr, SocketAddrV6, TcpStream},
    os::unix::{
        fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
        process::CommandExt,
    },
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};
fn output(command: &mut Command, seconds: u64) -> String {
    crate::command_io::capture(command, Duration::from_secs(seconds))
        .ok()
        .filter(|r| r.code == 0)
        .map(|r| r.stdout)
        .unwrap_or_default()
}
fn json(command: &mut Command, seconds: u64) -> Value {
    serde_json::from_str(&output(command, seconds)).unwrap_or(Value::Null)
}
fn tailnet(host: &str) -> Vec<String> {
    let mut status = json(Command::new("tailscale").args(["status", "--json"]), 5);
    if status.is_null() {
        output(
            Command::new("sudo").args(["-n", "systemctl", "start", "tailscaled"]),
            10,
        );
        status = json(Command::new("tailscale").args(["status", "--json"]), 5);
    }
    if status["BackendState"] == "Stopped" {
        eprintln!("Tjikup: reconnecting the local tailnet");
        output(Command::new("tailscale").arg("up"), 20);
        status = json(Command::new("tailscale").args(["status", "--json"]), 5);
    }
    if status["BackendState"] != "Running" {
        return vec![];
    }
    let mut peers = vec![&status["Self"]];
    if let Some(map) = status["Peer"].as_object() {
        peers.extend(map.values());
    }
    let matches: Vec<_> = peers
        .into_iter()
        .filter(|p| p["DNSName"].as_str().and_then(|s| s.split('.').next()) == Some(host))
        .collect();
    if matches.len() != 1 {
        return vec![];
    }
    let mut ips: Vec<_> = matches[0]["TailscaleIPs"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| s.as_str())
        .filter(|s| s.parse::<IpAddr>().is_ok())
        .map(str::to_owned)
        .collect();
    ips.sort_by_key(|s| !s.contains(':'));
    ips
}
fn reachable(address: &str) -> bool {
    let socket = if let Some((ip, scope)) = address.split_once('%') {
        let Ok(ip) = ip.parse() else {
            return false;
        };
        let Ok(scope) = scope.parse() else {
            return false;
        };
        SocketAddr::V6(SocketAddrV6::new(ip, 22, 0, scope))
    } else {
        let Ok(ip) = address.parse::<IpAddr>() else {
            return false;
        };
        SocketAddr::new(ip, 22)
    };
    TcpStream::connect_timeout(&socket, Duration::from_secs(2)).is_ok()
}
fn lan(host: &str) -> Option<String> {
    for protocol in ["1", "0"] {
        let value = json(
            Command::new("busctl").args([
                "--system",
                "--timeout=2",
                "--json=short",
                "call",
                "--",
                "org.freedesktop.Avahi",
                "/",
                "org.freedesktop.Avahi.Server",
                "ResolveHostName",
                "iisiu",
                "-1",
                protocol,
                &format!("{host}.local"),
                "-1",
                "0",
            ]),
            3,
        );
        if let (Some(address), Some(index)) = (value["data"][4].as_str(), value["data"][0].as_u64())
        {
            if let Ok(ip) = address.parse::<IpAddr>() {
                let addr = if matches!(ip,IpAddr::V6(v) if v.is_unicast_link_local()) {
                    format!("{address}%{index}")
                } else {
                    address.into()
                };
                if reachable(&addr) {
                    return Some(addr);
                }
            }
        }
    }
    for line in output(
        Command::new("getent").args(["ahosts", &format!("{host}.local")]),
        3,
    )
    .lines()
    {
        if let Some(ip) = line.split_whitespace().next() {
            if reachable(ip) {
                return Some(ip.into());
            }
        }
    }
    None
}
pub fn cli(mut args: Vec<String>) -> Result<i32> {
    if args.is_empty() || args == ["--help"] || args == ["-h"] {
        println!("tjikup ssh [--transport auto|lan|tailnet] HOST [SSH_ARGS ...]\n\nResolve one reachable address, enforce the declared host key, and execute SSH once.");
        return Ok(0);
    }
    let mut transport = std::env::var("FLEET_SSH_TRANSPORT").unwrap_or("auto".into());
    if args.first().is_some_and(|s| s == "--transport") {
        if args.len() < 3 {
            bail!("--transport requires a mode and host");
        }
        transport = args.remove(1);
        args.remove(0);
    }
    if !["auto", "lan", "tailnet"].contains(&transport.as_str()) {
        bail!("transport must be auto, lan or tailnet");
    }
    let host = args.remove(0);
    let policy: Value = serde_json::from_slice(&fs::read(
        crate::commands::repository().join("native/fleet.json"),
    )?)?;
    let entry = policy["hosts"].get(&host).context("unknown fleet host")?;
    let user = entry["user"].as_str().context("host user missing")?;
    let alias = entry["host_key_alias"]
        .as_str()
        .context("host key alias missing")?;
    let mut found = None;
    if transport != "lan" {
        for address in tailnet(&host) {
            if reachable(&address) {
                found = Some(("tailnet", address));
                break;
            }
        }
    }
    if found.is_none() && transport != "tailnet" {
        if let Some(address) = lan(&host) {
            found = Some(("LAN", address));
        }
    }
    let (via,address)=found.with_context(||format!("{host} has no reachable SSH address via {transport}; check that it is awake, connected and running sshd"))?;
    let home = crate::commands::home()?;
    let known = entry["known_hosts"]
        .as_str()
        .context("known-host policy missing")?
        .replace("{home}", &home.to_string_lossy());
    eprintln!("Tjikup: {host} via {via} ({address})");
    Err(Command::new("ssh")
        .args([
            "-x",
            "-o",
            "ConnectTimeout=10",
            "-o",
            "ServerAliveInterval=30",
            "-o",
            "ServerAliveCountMax=3",
            "-o",
            "StrictHostKeyChecking=yes",
            "-o",
            &format!("HostKeyAlias={alias}"),
            "-o",
            &format!(
                "UserKnownHostsFile={known} {}",
                home.join(".ssh/known_hosts").display()
            ),
            &format!("{user}@{address}"),
        ])
        .args(args)
        .exec()
        .into())
}
pub fn timing(args: &[String]) -> Result<i32> {
    if args.len() != 2 || !["take", "accept"].contains(&args[0].as_str()) {
        bail!("timing take PATH | accept DIRECTORY");
    }
    let path = if args[0] == "take" {
        PathBuf::from(&args[1])
    } else {
        Path::new(&args[1]).join("timings.json.part")
    };
    let mut file = match fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&path)
    {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && args[0] == "take" => return Ok(44),
        Err(e) => return Err(e.into()),
    };
    let info = file.metadata()?;
    if !info.is_file() || info.uid() != unsafe { libc::geteuid() } || info.len() > 16 * 1024 * 1024
    {
        bail!("invalid timing report");
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    serde_json::from_slice::<Value>(&bytes).context("invalid timing JSON")?;
    if args[0] == "take" {
        std::io::stdout().write_all(&bytes)?;
        std::io::stdout().flush()?;
        fs::remove_file(path)?;
    } else {
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
        fs::rename(path, Path::new(&args[1]).join("timings.json"))?;
    }
    Ok(0)
}
