//! Toolkit-independent desktop contract. Blocking methods belong on worker threads.
//! Full snapshots replace state atomically; revision orders snapshots in a session.
//! No GTK objects or async-runtime types cross this boundary.
mod events;
pub mod fake;
mod hyprland;
mod session;
pub use events::{DesktopEvent, DesktopState, Subscription};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, OnceLock};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DesktopError {
    Unsupported,
    Unavailable(String),
    PermissionDenied,
    Cancelled,
    StaleTarget,
    Timeout,
}
impl std::fmt::Display for DesktopError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for DesktopError {}
pub type Result<T> = std::result::Result<T, DesktopError>;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WindowId(pub String);
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Workspace {
    pub id: String,
    pub name: Option<String>,
}
/// Workspaces exist independently of windows, including empty workspaces.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceState {
    pub workspace: Workspace,
    pub output: Option<String>,
    pub special: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceTarget {
    pub generation: String,
    pub output: String,
    /// Existing opaque identity, or None to materialize a configured normal workspace.
    pub id: Option<String>,
    pub name: String,
}
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Output {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub logical: Rect,
    pub width: u64,
    pub height: u64,
    pub scale: f64,
    pub transform: u64,
    pub focused: bool,
    pub workspace: Option<Workspace>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Window {
    pub id: WindowId,
    pub title: Option<String>,
    pub app_id: Option<String>,
    pub initial_app_id: Option<String>,
    pub pid: Option<u64>,
    pub output: Option<String>,
    pub workspace: Option<Workspace>,
    pub geometry: Option<Rect>,
    pub mapped: Option<bool>,
    pub hidden: Option<bool>,
    pub floating: Option<bool>,
    pub focus_order: Option<i64>,
}
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Capabilities {
    pub windows: bool,
    pub workspace_select: bool,
    pub activate: bool,
    pub close: bool,
    pub outputs: bool,
    pub pointer: bool,
    pub outside_click: bool,
    pub placement: bool,
    pub recording_shortcut: bool,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub generation: String,
    pub revision: u64,
    pub windows: Vec<Window>,
    #[serde(default)]
    pub workspaces: Vec<WorkspaceState>,
    pub outputs: Vec<Output>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Diagnostics {
    pub adapter: String,
    pub version: Option<String>,
    pub capabilities: Capabilities,
    pub error: Option<String>,
}
pub trait DesktopSession: Send + Sync {
    fn diagnostics(&self) -> Diagnostics;
    fn snapshot(&self) -> Result<Snapshot>;
    fn worker_environment(&self) -> Vec<(String, String)> {
        vec![]
    }
    fn subscribe(&self) -> Result<Subscription> {
        Err(DesktopError::Unsupported)
    }
}
pub trait WindowActions: Send + Sync {
    fn activate(&self, target: &WindowId) -> Result<()>;
    fn activate_newly_mapped(&self, target: &WindowId) -> Result<()> {
        self.activate(target)
    }
    fn close(&self, target: &WindowId) -> Result<()>;
    fn place(&self, _target: &WindowId, _rect: Rect) -> Result<()> {
        Err(DesktopError::Unsupported)
    }
}
pub trait WorkspaceActions: Send + Sync {
    fn select_workspace(&self, _target: &WorkspaceTarget) -> Result<()> {
        Err(DesktopError::Unsupported)
    }
}
pub trait Desktop: DesktopSession + WindowActions + WorkspaceActions {
    fn pointer(&self) -> Result<(f64, f64)> {
        Err(DesktopError::Unsupported)
    }
    fn observe_outside_click(
        &self,
        _pid: u32,
        _namespace: &str,
        _pinned: bool,
        _command: &str,
    ) -> Result<()> {
        Err(DesktopError::Unsupported)
    }
    fn recording_shortcut(&self, _token: u32, _command: Option<&str>) -> Result<()> {
        Err(DesktopError::Unsupported)
    }
}
struct Unavailable(String);
impl DesktopSession for Unavailable {
    fn diagnostics(&self) -> Diagnostics {
        Diagnostics {
            adapter: "unavailable".into(),
            version: None,
            capabilities: Capabilities::default(),
            error: Some(self.0.clone()),
        }
    }
    fn snapshot(&self) -> Result<Snapshot> {
        Err(DesktopError::Unavailable(self.0.clone()))
    }
}
impl WindowActions for Unavailable {
    fn activate(&self, _: &WindowId) -> Result<()> {
        Err(DesktopError::Unavailable(self.0.clone()))
    }
    fn close(&self, _: &WindowId) -> Result<()> {
        Err(DesktopError::Unavailable(self.0.clone()))
    }
}
impl WorkspaceActions for Unavailable {}
impl Desktop for Unavailable {}
/// Resolve the runtime/display once. Recovery never searches another display or runtime.
/// TJIK_COMPOSITOR=fake never opens a host endpoint. Explicit unknown choices fail closed.
pub fn selected() -> &'static Arc<dyn Desktop> {
    static SESSION: OnceLock<Arc<dyn Desktop>> = OnceLock::new();
    SESSION.get_or_init(|| {
        let choice = std::env::var("TJIK_COMPOSITOR").unwrap_or_else(|_| "auto".into());
        if choice == "fake" { return Arc::new(fake::Fake::default()); }
        let result = if choice == "hyprland" || (choice == "auto" && std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some() && std::env::var_os("NIRI_SOCKET").is_none()) {
            session::Session::from_environment().map(|s| Arc::new(s) as Arc<dyn Desktop>)
        } else { Err(DesktopError::Unavailable(format!("Compositor choice {choice:?} is unavailable or ambiguous; set TJIK_COMPOSITOR explicitly"))) };
        result.unwrap_or_else(|e| Arc::new(Unavailable(e.to_string())))
    })
}

/// Inherit selected-session hints when launching a worker through a service manager.
pub fn environment() -> Vec<(String, String)> {
    let mut values: Vec<_> = [
        "TJIK_COMPOSITOR",
        "HYPRLAND_INSTANCE_SIGNATURE",
        "NIRI_SOCKET",
        "WAYLAND_DISPLAY",
        "XDG_RUNTIME_DIR",
    ]
    .into_iter()
    .filter_map(|k| std::env::var(k).ok().map(|v| (k.into(), v)))
    .collect();
    for (key, value) in selected().worker_environment() {
        values.retain(|(k, _)| k != &key);
        values.push((key, value));
    }
    values
}
