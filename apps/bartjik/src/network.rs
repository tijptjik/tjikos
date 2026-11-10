//! Toolkit-independent network boundary. Platform tools are confined to linux.rs.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex, OnceLock},
};
#[cfg(target_os = "linux")]
#[path = "network/linux.rs"]
mod linux;
#[cfg(all(test, target_os = "linux"))]
#[allow(unused_imports)]
pub use linux::fields;

pub type Reply<'a, T> = Pin<Box<dyn Future<Output = Result<T, Error>> + Send + 'a>>;
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct Addresses {
    pub ip: Vec<String>,
    pub ipv6: Vec<String>,
    pub gateway: String,
    pub dns: Vec<String>,
    pub dhcp: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Wifi,
    Ethernet,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Device {
    pub interface: String,
    pub kind: Kind,
    pub state: String,
    pub connection: String,
    #[serde(default)]
    pub addresses: Addresses,
}
impl Device {
    pub fn connected(&self) -> bool {
        self.state.starts_with("connected")
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AccessPoint {
    pub ssid: String,
    pub bssid: String,
    pub interface: String,
    pub signal: i64,
    pub security: String,
    pub active: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct Snapshot {
    pub enabled: bool,
    pub devices: Vec<Device>,
    pub networks: Vec<AccessPoint>,
    pub default_dns: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct Counters {
    pub rx: Option<f64>,
    pub tx: Option<f64>,
    pub rx_rate: Option<f64>,
    pub tx_rate: Option<f64>,
}
pub type Traffic = BTreeMap<String, Counters>;
#[derive(Clone, Copy)]
pub struct Capabilities {
    pub wifi: bool,
    pub ethernet: bool,
    pub traffic: bool,
}
// Deliberately no Debug: credentials must never reach diagnostics or logs.
#[derive(Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "lowercase")]
pub enum Action {
    Status,
    Scan,
    Radio {
        enabled: bool,
    },
    Connect {
        interface: String,
        bssid: String,
        #[serde(default)]
        password: String,
    },
    Disconnect {
        interface: String,
    },
    Ethernet {
        interface: String,
    },
}
#[derive(Clone, Debug, PartialEq)]
pub enum Error {
    #[allow(dead_code)] // Required by adapters with a smaller capability set.
    Unsupported,
    Unavailable,
    PermissionDenied,
    Timeout,
    StaleTarget,
    Failed,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Unsupported => "This network operation is not supported",
            Self::Unavailable => "Network service is unavailable",
            Self::PermissionDenied => "Network permission denied; check your authorization agent",
            Self::Timeout => "Network request timed out; try again",
            Self::StaleTarget => "The network or adapter disappeared; scan again",
            Self::Failed => {
                "Could not complete the request; check credentials and network availability"
            }
        })
    }
}
impl std::error::Error for Error {}
pub trait Adapter: Send + Sync {
    fn subscribe(
        &self,
        _wake: Arc<tokio::sync::Notify>,
    ) -> Option<crate::status_events::Subscription> {
        None
    }
    fn name(&self) -> &'static str;
    fn capabilities(&self) -> Capabilities;
    fn snapshot(&self) -> Reply<'_, Snapshot>;
    fn execute(&self, action: Action) -> Reply<'_, Snapshot>;
    fn counters(&self) -> Reply<'_, Traffic>;
}
pub fn selected() -> Arc<dyn Adapter> {
    static ADAPTER: OnceLock<Arc<dyn Adapter>> = OnceLock::new();
    ADAPTER
        .get_or_init(|| {
            #[cfg(target_os = "linux")]
            {
                Arc::new(linux::NetworkManager)
            }
            #[cfg(not(target_os = "linux"))]
            {
                Arc::new(Unsupported)
            }
        })
        .clone()
}
#[cfg(not(target_os = "linux"))]
struct Unsupported;
#[cfg(not(target_os = "linux"))]
impl Adapter for Unsupported {
    fn name(&self) -> &'static str {
        "unsupported"
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            wifi: false,
            ethernet: false,
            traffic: false,
        }
    }
    fn snapshot(&self) -> Reply<'_, Snapshot> {
        Box::pin(async { Err(Error::Unsupported) })
    }
    fn execute(&self, _: Action) -> Reply<'_, Snapshot> {
        self.snapshot()
    }
    fn counters(&self) -> Reply<'_, Traffic> {
        Box::pin(async { Err(Error::Unsupported) })
    }
}
// Compatibility boundary for the existing QML helper protocol.
pub async fn execute(value: &Value) -> anyhow::Result<Value> {
    let mut value = value.clone();
    if value.is_object() && value.get("action").is_none() {
        value["action"] = json!("status");
    }
    let action = serde_json::from_value(value)?;
    let adapter = selected();
    let mut result = serde_json::to_value(adapter.execute(action).await?)?;
    let capabilities = adapter.capabilities();
    result["adapter"] = json!(adapter.name());
    result["capabilities"] = json!({"wifi":capabilities.wifi,"ethernet":capabilities.ethernet,"traffic":capabilities.traffic});
    result["ok"] = json!(true);
    Ok(result)
}
pub async fn watch_traffic(emit: impl Fn(Value)) -> anyhow::Result<()> {
    let adapter = selected();
    let mut previous = Traffic::new();
    let mut since = std::time::Instant::now();
    loop {
        let current = adapter.counters().await?;
        emit(serde_json::to_value(rates(
            &current,
            &previous,
            since.elapsed().as_secs_f64(),
        ))?);
        previous = current;
        since = std::time::Instant::now();
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
}
pub fn rates(current: &Traffic, previous: &Traffic, seconds: f64) -> Traffic {
    current
        .iter()
        .map(|(id, c)| {
            let mut c = c.clone();
            let rate = |value: Option<f64>, old: Option<f64>| {
                value
                    .zip(old)
                    .filter(|(a, b)| a >= b && seconds > 0.)
                    .map(|(a, b)| (a - b) / seconds)
            };
            c.rx_rate = rate(c.rx, previous.get(id).and_then(|p| p.rx));
            c.tx_rate = rate(c.tx, previous.get(id).and_then(|p| p.tx));
            (id.clone(), c)
        })
        .collect()
}
/// Synthetic adapter: no environment discovery, subprocesses, or host fallbacks.
#[allow(dead_code)] // Also compiled into the GTK demo frontend.
pub struct Fake {
    pub state: Mutex<Snapshot>,
    pub commands: Mutex<Vec<String>>,
    pub failure: Mutex<Option<Error>>,
}
#[allow(dead_code)]
impl Fake {
    pub fn new(snapshot: Snapshot) -> Self {
        Self {
            state: Mutex::new(snapshot),
            commands: Mutex::new(vec![]),
            failure: Mutex::new(None),
        }
    }
    pub fn demo() -> Self {
        Self::new(
            serde_json::from_str(include_str!("../tests/fixtures/network-card.json")).unwrap(),
        )
    }
}
impl Adapter for Fake {
    fn name(&self) -> &'static str {
        "synthetic"
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
            if let Some(e) = self.failure.lock().unwrap().clone() {
                return Err(e);
            }
            Ok(self.state.lock().unwrap().clone())
        })
    }
    fn execute(&self, action: Action) -> Reply<'_, Snapshot> {
        Box::pin(async move {
            if let Some(e) = self.failure.lock().unwrap().clone() {
                return Err(e);
            }
            let mut state = self.state.lock().unwrap();
            let name = match action {
                Action::Status => "status",
                Action::Scan => "scan",
                Action::Radio { enabled } => {
                    state.enabled = enabled;
                    if !enabled {
                        for d in &mut state.devices {
                            if d.kind == Kind::Wifi {
                                d.state = "disconnected".into();
                            }
                        }
                        for n in &mut state.networks {
                            n.active = false;
                        }
                    }
                    "radio"
                }
                Action::Connect {
                    interface, bssid, ..
                } => {
                    if !state.enabled {
                        return Err(Error::Unavailable);
                    }
                    let ap = state
                        .networks
                        .iter()
                        .find(|n| n.interface == interface && n.bssid == bssid)
                        .cloned()
                        .ok_or(Error::StaleTarget)?;
                    let d = state
                        .devices
                        .iter_mut()
                        .find(|d| d.interface == interface)
                        .ok_or(Error::StaleTarget)?;
                    d.state = "connected".into();
                    d.connection = ap.ssid;
                    for n in &mut state.networks {
                        if n.interface == interface {
                            n.active = n.bssid == bssid;
                        }
                    }
                    "connect"
                }
                Action::Disconnect { interface } => {
                    state
                        .devices
                        .iter_mut()
                        .find(|d| d.interface == interface)
                        .ok_or(Error::StaleTarget)?
                        .state = "disconnected".into();
                    for n in &mut state.networks {
                        if n.interface == interface {
                            n.active = false;
                        }
                    }
                    "disconnect"
                }
                Action::Ethernet { interface } => {
                    state
                        .devices
                        .iter_mut()
                        .find(|d| d.interface == interface && d.kind == Kind::Ethernet)
                        .ok_or(Error::StaleTarget)?
                        .state = "connected".into();
                    "ethernet"
                }
            };
            self.commands.lock().unwrap().push(name.into());
            Ok(state.clone())
        })
    }
    fn counters(&self) -> Reply<'_, Traffic> {
        Box::pin(async {
            Ok(BTreeMap::from([(
                "wlan-demo".into(),
                Counters {
                    rx: Some(1048576.),
                    tx: Some(524288.),
                    ..Default::default()
                },
            )]))
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn fake_actions_failures_and_device_removal() {
        let adapter = Fake::demo();
        let initial = adapter.snapshot().await.unwrap();
        let ap = initial.networks.iter().find(|n| !n.active).unwrap();
        let connected = adapter
            .execute(Action::Connect {
                interface: ap.interface.clone(),
                bssid: ap.bssid.clone(),
                password: "secret-not-recorded".into(),
            })
            .await
            .unwrap();
        assert!(connected
            .networks
            .iter()
            .any(|n| n.bssid == ap.bssid && n.active));
        assert_eq!(*adapter.commands.lock().unwrap(), vec!["connect"]);
        adapter.state.lock().unwrap().devices.clear();
        assert_eq!(
            adapter
                .execute(Action::Disconnect {
                    interface: ap.interface.clone()
                })
                .await
                .unwrap_err(),
            Error::StaleTarget
        );
        *adapter.failure.lock().unwrap() = Some(Error::PermissionDenied);
        assert_eq!(
            adapter.execute(Action::Scan).await.unwrap_err(),
            Error::PermissionDenied
        );
        *adapter.failure.lock().unwrap() = None;
        assert!(adapter.snapshot().await.is_ok());
    }
    #[test]
    fn counter_resets_and_unknown_are_not_zero() {
        let current = BTreeMap::from([(
            "nic".into(),
            Counters {
                rx: Some(10.),
                tx: None,
                ..Default::default()
            },
        )]);
        assert!(rates(&current, &Traffic::new(), 1.)["nic"]
            .rx_rate
            .is_none());
        let mut previous = current.clone();
        previous.get_mut("nic").unwrap().rx = Some(20.);
        assert!(rates(&current, &previous, 1.)["nic"].rx_rate.is_none());
        previous.get_mut("nic").unwrap().rx = Some(0.);
        assert_eq!(rates(&current, &previous, 2.)["nic"].rx_rate, Some(5.));
    }
}
