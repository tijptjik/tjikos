use crate::*;
use serde_json::{json, Value};
use std::{
    io::{Read, Write},
    os::unix::{fs::MetadataExt, net::UnixStream},
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

pub struct Hyprland {
    path: PathBuf,
    #[cfg(feature = "capture")]
    display: PathBuf,
    generation: String,
    inode: u64,
    lua: bool,
    version: String,
    revision: AtomicU64,
}
fn error(e: std::io::Error) -> DesktopError {
    match e.kind() {
        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock => DesktopError::Timeout,
        std::io::ErrorKind::PermissionDenied => DesktopError::PermissionDenied,
        _ => DesktopError::Unavailable(e.to_string()),
    }
}
fn s(v: &Value) -> &str {
    v.as_str().unwrap_or("")
}
fn text(v: &Value) -> Option<String> {
    v.as_str().map(str::to_owned)
}
fn request(path: &std::path::Path, command: &str) -> Result<String> {
    request_with_timeout(path, command, Duration::from_secs(1))
}
fn request_with_timeout(
    path: &std::path::Path,
    command: &str,
    timeout: Duration,
) -> Result<String> {
    let mut stream = UnixStream::connect(path).map_err(error)?;
    stream.set_read_timeout(Some(timeout)).map_err(error)?;
    stream.set_write_timeout(Some(timeout)).map_err(error)?;
    let wire = if command.starts_with("j/") {
        command.to_owned()
    } else {
        format!("/{command}")
    };
    stream.write_all(wire.as_bytes()).map_err(error)?;
    let mut bytes = Vec::new();
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let remaining = deadline
            .checked_duration_since(std::time::Instant::now())
            .ok_or(DesktopError::Timeout)?;
        stream.set_read_timeout(Some(remaining)).map_err(error)?;
        let mut chunk = [0; 16384];
        let count = stream.read(&mut chunk).map_err(error)?;
        if count == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..count]);
        if bytes.len() > 8 * 1024 * 1024 {
            return Err(DesktopError::Unavailable("IPC reply too large".into()));
        }
    }
    String::from_utf8(bytes).map_err(|e| DesktopError::Unavailable(e.to_string()))
}
impl Hyprland {
    pub(crate) fn connect_at(path: PathBuf, display: &std::path::Path) -> Result<Self> {
        let signature = path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|v| v.to_str())
            .ok_or(DesktopError::StaleTarget)?;
        let runtime = path
            .parent()
            .and_then(|p| p.parent())
            .and_then(|p| p.parent())
            .ok_or(DesktopError::StaleTarget)?;
        let lock =
            std::fs::read_to_string(path.parent().unwrap().join("hyprland.lock")).map_err(error)?;
        let expected = lock
            .lines()
            .nth(1)
            .ok_or_else(|| DesktopError::Unavailable("Missing display identity".into()))?;
        if runtime.join(expected) != display {
            return Err(DesktopError::Unavailable(
                "Selected compositor does not own the selected Wayland display".into(),
            ));
        }
        let signature = signature.to_owned();
        let inode = path.metadata().map_err(error)?.ino();
        let version: Value = serde_json::from_str(&request(&path, "j/version")?)
            .map_err(|e| DesktopError::Unavailable(e.to_string()))?;
        if !version.is_object() || version["commit"].as_str().is_none() {
            return Err(DesktopError::Unavailable(
                "Endpoint did not identify as Hyprland".into(),
            ));
        }
        let lua = request(&path, "eval return").is_ok_and(|s| s.trim() == "ok");
        Ok(Self {
            #[cfg(feature = "capture")]
            display: display.to_owned(),
            path,
            generation: format!("{signature}:{inode}"),
            inode,
            lua,
            version: s(&version["version"]).to_owned(),
            revision: AtomicU64::new(0),
        })
    }
    pub(crate) fn event_socket(&self) -> Result<UnixStream> {
        self.check()?;
        UnixStream::connect(self.path.with_file_name(".socket2.sock")).map_err(error)
    }
    pub(crate) fn check(&self) -> Result<()> {
        if self.path.metadata().map_err(error)?.ino() != self.inode {
            return Err(DesktopError::StaleTarget);
        }
        Ok(())
    }
    fn query(&self, kind: &str) -> Result<Value> {
        self.check()?;
        serde_json::from_str(&request(&self.path, &format!("j/{kind}"))?)
            .map_err(|e| DesktopError::Unavailable(e.to_string()))
    }
    fn command(&self, command: &str) -> Result<()> {
        self.check()?;
        let reply = request(&self.path, command)?;
        if reply.trim() != "ok" {
            return Err(DesktopError::Unavailable(reply));
        }
        Ok(())
    }
    fn id(&self, kind: &str, value: &str) -> String {
        json!([self.generation, kind, value]).to_string()
    }
    fn workspace(&self, v: &Value) -> Option<Workspace> {
        let id = v["id"].as_i64()?;
        Some(Workspace {
            id: self.id("workspace", &id.to_string()),
            name: text(&v["name"]),
        })
    }
    fn target(&self, id: &WindowId) -> Result<Value> {
        self.check()?;
        let parts: Vec<String> =
            serde_json::from_str(&id.0).map_err(|_| DesktopError::StaleTarget)?;
        if parts.len() != 3 || parts[0] != self.generation || parts[1] != "window" {
            return Err(DesktopError::StaleTarget);
        }
        self.query("clients")?
            .as_array()
            .and_then(|cs| cs.iter().find(|c| s(&c["address"]) == parts[2]).cloned())
            .ok_or(DesktopError::StaleTarget)
    }
}
impl DesktopSession for Hyprland {
    fn worker_environment(&self) -> Vec<(String, String)> {
        vec![(
            "HYPRLAND_INSTANCE_SIGNATURE".into(),
            self.path
                .parent()
                .unwrap()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
        )]
    }

    fn diagnostics(&self) -> Diagnostics {
        Diagnostics {
            adapter: "hyprland".into(),
            version: Some(self.version.clone()),
            capabilities: Capabilities {
                windows: true,
                workspace_select: true,
                activate: true,
                close: true,
                outputs: true,
                pointer: true,
                outside_click: self.lua,
                placement: self.lua,
                recording_shortcut: self.lua,
            },
            error: self.check().err().map(|e| e.to_string()),
        }
    }
    fn snapshot(&self) -> Result<Snapshot> {
        let monitors = self.query("monitors")?;
        let clients = self.query("clients")?;
        let mut outputs = vec![];
        for m in monitors
            .as_array()
            .ok_or_else(|| DesktopError::Unavailable("Invalid outputs".into()))?
        {
            let scale = m["scale"].as_f64().unwrap_or(1.).max(0.1);
            let width = m["width"].as_u64().unwrap_or(0);
            let height = m["height"].as_u64().unwrap_or(0);
            let transform = m["transform"].as_u64().unwrap_or(0);
            let (w, h) = if transform % 2 == 1 {
                (height, width)
            } else {
                (width, height)
            };
            let workspace = if m["specialWorkspace"]["id"].as_i64().unwrap_or(0) != 0 {
                &m["specialWorkspace"]
            } else {
                &m["activeWorkspace"]
            };
            outputs.push(Output {
                id: self.id("output", s(&m["name"])),
                name: s(&m["name"]).into(),
                description: text(&m["description"]),
                logical: Rect {
                    x: m["x"].as_f64().unwrap_or(0.),
                    y: m["y"].as_f64().unwrap_or(0.),
                    width: w as f64 / scale,
                    height: h as f64 / scale,
                },
                width,
                height,
                scale,
                transform,
                focused: m["focused"] == true,
                workspace: self.workspace(workspace),
            });
        }
        let mut windows = vec![];
        for c in clients
            .as_array()
            .ok_or_else(|| DesktopError::Unavailable("Invalid windows".into()))?
        {
            let output = monitors
                .as_array()
                .unwrap()
                .iter()
                .find(|m| m["id"] == c["monitor"])
                .map(|m| self.id("output", s(&m["name"])));
            let geometry = (|| {
                Some(Rect {
                    x: c["at"][0].as_f64()?,
                    y: c["at"][1].as_f64()?,
                    width: c["size"][0].as_f64()?,
                    height: c["size"][1].as_f64()?,
                })
            })();
            windows.push(Window {
                id: WindowId(self.id("window", s(&c["address"]))),
                title: text(&c["title"]),
                app_id: text(&c["class"]),
                initial_app_id: text(&c["initialClass"]),
                pid: c["pid"].as_u64(),
                output,
                workspace: self.workspace(&c["workspace"]),
                geometry,
                mapped: c["mapped"].as_bool(),
                hidden: c["hidden"].as_bool(),
                floating: c["floating"].as_bool(),
                focus_order: c["focusHistoryID"].as_i64(),
            });
        }
        let raw_workspaces = self.query("workspaces")?;
        let workspaces = raw_workspaces
            .as_array()
            .ok_or_else(|| DesktopError::Unavailable("Invalid workspaces".into()))?
            .iter()
            .filter_map(|w| {
                Some(WorkspaceState {
                    workspace: self.workspace(w)?,
                    output: outputs
                        .iter()
                        .find(|o| o.name == s(&w["monitor"]))
                        .map(|o| o.id.clone()),
                    special: s(&w["name"]).starts_with("special:") || s(&w["name"]) == "special",
                })
            })
            .collect();
        self.check()?;
        Ok(Snapshot {
            generation: self.generation.clone(),
            revision: self.revision.fetch_add(1, Ordering::Relaxed) + 1,
            windows,
            workspaces,
            outputs,
        })
    }
}
impl WindowActions for Hyprland {
    #[cfg(feature = "capture")]
    fn capture(&self, id: &WindowId) -> Result<CapturedFrame> {
        let target = self.target(id)?;
        let address = u64::from_str_radix(s(&target["address"]).trim_start_matches("0x"), 16)
            .map_err(|_| DesktopError::StaleTarget)?;
        let frame = crate::capture::capture(&self.display, address as u32)?;
        self.target(id)?;
        Ok(frame)
    }
    fn activate(&self, id: &WindowId) -> Result<()> {
        let client = self.target(id)?;
        let monitors = self.query("monitors")?;
        for args in focus_commands(
            &client,
            monitors
                .as_array()
                .ok_or_else(|| DesktopError::Unavailable("Invalid outputs".into()))?,
            self.lua,
        )? {
            self.target(id)?;
            self.command(&format!("dispatch {}", args.join(" ")))?;
        }
        self.reveal(id)
    }
    fn activate_newly_mapped(&self, id: &WindowId) -> Result<()> {
        self.activate(id)?;
        std::thread::sleep(Duration::from_millis(150));
        self.reveal(id)
    }
    fn close(&self, id: &WindowId) -> Result<()> {
        let c = self.target(id)?;
        let target = lua(&format!("address:{}", s(&c["address"])));
        if self.lua {
            self.command(&format!(
                "dispatch hl.dsp.window.close({{window={target}}})"
            ))
        } else {
            self.command(&format!(
                "dispatch closewindow address:{}",
                s(&c["address"])
            ))
        }
    }
    fn prepare_toplevel(&self, title: &str, workspace: &WorkspaceTarget, rect: Rect) -> Result<()> {
        if !self.lua {
            return Err(DesktopError::Unsupported);
        }
        if title.is_empty()
            || !title
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b":-_".contains(&c))
            || ![rect.x, rect.y, rect.width, rect.height]
                .iter()
                .all(|v| v.is_finite())
            || rect.width <= 0.
            || rect.height <= 0.
        {
            return Err(DesktopError::Unavailable(
                "Invalid toplevel placement".into(),
            ));
        }
        let snapshot = self.snapshot()?;
        if snapshot.generation != workspace.generation {
            return Err(DesktopError::StaleTarget);
        }
        let output = snapshot
            .outputs
            .iter()
            .find(|o| o.id == workspace.output)
            .ok_or(DesktopError::StaleTarget)?;
        let selected = snapshot
            .workspaces
            .iter()
            .find(|w| Some(&w.workspace.id) == workspace.id.as_ref())
            .ok_or(DesktopError::StaleTarget)?;
        if selected.output.as_ref() != Some(&workspace.output) {
            return Err(DesktopError::StaleTarget);
        }
        let name = selected
            .workspace
            .name
            .as_deref()
            .ok_or(DesktopError::StaleTarget)?;
        let key = lua(&format!("tjik-toplevel-{title}"));
        self.command(&format!("eval if _G[{key}] then _G[{key}]:set_enabled(false) end; _G[{key}]=hl.window_rule({{name={key},float=true,no_initial_focus=true,no_anim=true,no_blur=true,no_shadow=true,border_size=0,workspace={},monitor={},size={{{},{}}},move={{{},{}}},match={{initial_title={}}}}})",
            lua(&format!("{name} silent")), lua(&output.name), rect.width.ceil(), rect.height.ceil(),
            lua(&format!("({})", (rect.x-output.logical.x).round())), lua(&format!("({})", (rect.y-output.logical.y).round())),
            lua(&format!("^{title}$"))))
    }
    fn release_toplevel(&self, title: &str) -> Result<()> {
        if !self.lua {
            return Err(DesktopError::Unsupported);
        }
        let key = lua(&format!("tjik-toplevel-{title}"));
        self.command(&format!(
            "eval if _G[{key}] then _G[{key}]:set_enabled(false); _G[{key}]=nil end"
        ))
    }
    fn place(&self, id: &WindowId, r: Rect) -> Result<()> {
        if !self.lua {
            return Err(DesktopError::Unsupported);
        }
        if ![r.x, r.y, r.width, r.height].iter().all(|x| x.is_finite())
            || r.width <= 0.
            || r.height <= 0.
        {
            return Err(DesktopError::Unavailable("Invalid geometry".into()));
        }
        let c = self.target(id)?;
        let target = lua(&format!("address:{}", s(&c["address"])));
        self.command(&format!(
            "dispatch hl.dsp.window.resize({{x={},y={},relative=false,window={target}}})",
            r.width, r.height
        ))?;
        self.target(id)?;
        self.command(&format!(
            "dispatch hl.dsp.window.move({{x={},y={},relative=false,window={target}}})",
            r.x, r.y
        ))
    }
}
impl WorkspaceActions for Hyprland {
    fn select_workspace(&self, target: &WorkspaceTarget) -> Result<()> {
        self.check()?;
        if target.generation != self.generation {
            return Err(DesktopError::StaleTarget);
        }
        if target.name.is_empty()
            || target.name.len() > 256
            || target.name.chars().any(char::is_control)
        {
            return Err(DesktopError::Unavailable("Invalid workspace name".into()));
        }
        let monitors = self.query("monitors")?;
        let monitor = monitors
            .as_array()
            .and_then(|rows| {
                rows.iter()
                    .find(|m| self.id("output", s(&m["name"])) == target.output)
            })
            .ok_or(DesktopError::StaleTarget)?;
        let name = s(&monitor["name"]);
        let workspaces = self.query("workspaces")?;
        let rows = workspaces
            .as_array()
            .ok_or_else(|| DesktopError::Unavailable("Invalid workspaces".into()))?;
        let existing = if let Some(id) = &target.id {
            Some(
                rows.iter()
                    .find(|w| self.workspace(w).is_some_and(|ws| ws.id == *id))
                    .ok_or(DesktopError::StaleTarget)?,
            )
        } else {
            rows.iter().find(|w| s(&w["name"]) == target.name)
        };
        if existing.is_some_and(|w| s(&w["monitor"]) != name) {
            return Err(DesktopError::StaleTarget);
        }
        let workspace_name = existing.map(|w| s(&w["name"])).unwrap_or(&target.name);
        let special = workspace_name == "special" || workspace_name.starts_with("special:");
        if special && target.id.is_none() {
            return Err(DesktopError::Unsupported);
        }
        let focus = if self.lua {
            format!("hl.dsp.focus({{monitor={}}})", lua(name))
        } else {
            format!("focusmonitor {name}")
        };
        self.command(&format!("dispatch {focus}"))?;
        // Monitor removal or workspace migration during the handoff invalidates the intent.
        let current = self.query("monitors")?;
        let current_monitor = current
            .as_array()
            .and_then(|ms| ms.iter().find(|m| s(&m["name"]) == name))
            .ok_or(DesktopError::StaleTarget)?;
        if let Some(id) = &target.id {
            let current = self.query("workspaces")?;
            if !current.as_array().is_some_and(|ws| {
                ws.iter().any(|w| {
                    self.workspace(w).is_some_and(|ws| ws.id == *id) && s(&w["monitor"]) == name
                })
            }) {
                return Err(DesktopError::StaleTarget);
            }
        }
        let visible = s(&current_monitor["specialWorkspace"]["name"]);
        let toggle = |name: &str| {
            if self.lua {
                format!(
                    "hl.dsp.workspace.toggle_special({})",
                    lua(name.trim_start_matches("special:"))
                )
            } else {
                format!(
                    "togglespecialworkspace {}",
                    name.trim_start_matches("special:")
                )
            }
        };
        if !visible.is_empty() && (!special || visible != workspace_name) {
            self.command(&format!("dispatch {}", toggle(visible)))?;
        }
        if special {
            if visible != workspace_name {
                self.command(&format!("dispatch {}", toggle(workspace_name)))?;
            }
            return Ok(());
        }
        let selector =
            if let Some(number) = existing.and_then(|w| w["id"].as_i64()).filter(|id| *id > 0) {
                number.to_string()
            } else if let Ok(number) = workspace_name.parse::<u64>() {
                if number == 0 || number > i32::MAX as u64 {
                    return Err(DesktopError::Unsupported);
                }
                number.to_string()
            } else {
                format!("name:{workspace_name}")
            };
        let dispatch = if self.lua {
            format!("hl.dsp.focus({{workspace={}}})", lua(&selector))
        } else {
            format!("workspace {selector}")
        };
        self.command(&format!("dispatch {dispatch}"))
    }
}

impl Desktop for Hyprland {
    fn pointer(&self) -> Result<(f64, f64)> {
        self.check()?;
        let p: Value = serde_json::from_str(&request_with_timeout(
            &self.path,
            "j/cursorpos",
            Duration::from_millis(75),
        )?)
        .map_err(|e| DesktopError::Unavailable(e.to_string()))?;
        Ok((
            p["x"].as_f64().ok_or(DesktopError::Unsupported)?,
            p["y"].as_f64().ok_or(DesktopError::Unsupported)?,
        ))
    }
    fn observe_outside_click(
        &self,
        pid: u32,
        namespace: &str,
        pinned: bool,
        command: &str,
    ) -> Result<()> {
        if !self.lua {
            return Err(DesktopError::Unsupported);
        }
        let script = include_str!("outside-click.lua")
            .replace("__PID__", &pid.to_string())
            .replace("__PINNED__", if pinned { "true" } else { "false" })
            .replace("__COMMAND__", &lua(command))
            .replace("'runtjik-gtk'", &lua(namespace))
            .replace("'^runtjik-gtk$'", &lua(&format!("^{namespace}$")));
        self.command(&format!("eval {script}"))
    }
    fn recording_shortcut(&self, token: u32, command: Option<&str>) -> Result<()> {
        if !self.lua {
            return Err(DesktopError::Unsupported);
        }
        let script = if let Some(command) = command {
            format!(
                "local stop_command={}; local stop_token={token};\n{}",
                lua(command),
                include_str!("recording-stop.lua")
            )
        } else {
            format!("if _G.runtjikRecordingStop and _G.runtjikRecordingStop.token == {token} then for _, binding in ipairs(_G.runtjikRecordingStop.bindings) do binding:remove() end; _G.runtjikRecordingStop = nil end")
        };
        self.command(&format!("eval {script}"))
    }
}

fn lua(value: &str) -> String {
    let mut out = String::from("\"");
    for c in value.chars() {
        if c == '\\' || c == '"' {
            out.push('\\');
            out.push(c);
        } else if (c as u32) < 32 {
            out.push_str(&format!("\\{:03}", c as u32));
        } else {
            out.push(c);
        }
    }
    out.push('"');
    out
}
pub fn focus_commands(
    client: &Value,
    monitors: &[Value],
    is_lua: bool,
) -> Result<Vec<Vec<String>>> {
    let address = s(&client["address"]);
    if !address.starts_with("0x")
        || address.len() < 3
        || !address[2..].bytes().all(|c| c.is_ascii_hexdigit())
    {
        return Err(DesktopError::StaleTarget);
    }
    let monitor = monitors.iter().find(|m| m["id"] == client["monitor"]);
    let workspace = s(&client["workspace"]["name"]);
    let mut commands = vec![];
    let focus = |name: &str| {
        if is_lua {
            vec![format!("hl.dsp.focus({{ monitor = {} }})", lua(name))]
        } else {
            vec!["focusmonitor".into(), name.into()]
        }
    };
    let toggle = |name: &str| {
        let name = if name == "special" {
            ""
        } else {
            name.strip_prefix("special:").unwrap_or(name)
        };
        if is_lua {
            vec![format!("hl.dsp.workspace.toggle_special({})", lua(name))]
        } else {
            vec!["togglespecialworkspace".into(), name.into()]
        }
    };
    if let Some(m) = monitor {
        for other in monitors {
            let special = s(&other["specialWorkspace"]["name"]);
            if special.is_empty() || (other["id"] == m["id"] && special == workspace) {
                continue;
            }
            commands.push(focus(s(&other["name"])));
            commands.push(toggle(special));
        }
        commands.push(focus(s(&m["name"])));
        if (workspace == "special" || workspace.starts_with("special:"))
            && s(&m["specialWorkspace"]["name"]) != workspace
        {
            commands.push(toggle(workspace));
        }
    }
    commands.push(if is_lua {
        vec![format!(
            "hl.dsp.focus({{ window = {} }})",
            lua(&format!("address:{address}"))
        )]
    } else {
        vec!["focuswindow".into(), format!("address:{address}")]
    });
    Ok(commands)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn special_workspace_focus() {
        let c = json!({"address":"0xabc","monitor":2,"workspace":{"name":"special:notes"}});
        let m = vec![
            json!({"id":1,"name":"DP-1","specialWorkspace":{"name":"special:other"}}),
            json!({"id":2,"name":"DP-2","specialWorkspace":{"name":""}}),
        ];
        let commands = focus_commands(&c, &m, false).unwrap();
        assert_eq!(
            commands.iter().map(|v| v.join(" ")).collect::<Vec<_>>(),
            [
                "focusmonitor DP-1",
                "togglespecialworkspace other",
                "focusmonitor DP-2",
                "togglespecialworkspace notes",
                "focuswindow address:0xabc"
            ]
        );
    }
    #[test]
    fn rejects_address_injection() {
        assert!(focus_commands(&json!({"address":"0xabc;exec bad"}), &[], true).is_err());
        assert_eq!(lua("a\"\n\\b"), "\"a\\\"\\010\\\\b\"");
    }
}

#[cfg(test)]
mod transport_tests {
    use super::*;
    use std::os::unix::net::UnixListener;
    fn fixture(replies: Vec<(&'static str, String)>) -> (Hyprland, std::thread::JoinHandle<()>) {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "tjik-desktop-{}-{}.sock",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let listener = UnixListener::bind(&path).unwrap();
        let inode = path.metadata().unwrap().ino();
        let worker = std::thread::spawn(move || {
            for (expected, reply) in replies {
                let (mut stream, _) = listener.accept().unwrap();
                let mut bytes = [0; 8192];
                let n = stream.read(&mut bytes).unwrap();
                assert_eq!(std::str::from_utf8(&bytes[..n]).unwrap(), expected);
                stream.write_all(reply.as_bytes()).unwrap();
            }
        });
        (
            Hyprland {
                #[cfg(feature = "capture")]
                display: PathBuf::from("/synthetic/display"),
                path,
                generation: "synthetic-session".into(),
                inode,
                lua: true,
                version: "fixture".into(),
                revision: AtomicU64::new(0),
            },
            worker,
        )
    }
    #[test]
    fn configured_empty_workspace_focuses_output_then_workspace() {
        let monitors =
            json!([{"name":"fixture-output","specialWorkspace":{"name":""}}]).to_string();
        let (d, worker) = fixture(vec![
            ("j/monitors", monitors.clone()),
            ("j/workspaces", "[]".into()),
            (
                "/dispatch hl.dsp.focus({monitor=\"fixture-output\"})",
                "ok".into(),
            ),
            ("j/monitors", monitors),
            ("/dispatch hl.dsp.focus({workspace=\"8\"})", "ok".into()),
        ]);
        d.select_workspace(&WorkspaceTarget {
            generation: d.generation.clone(),
            output: d.id("output", "fixture-output"),
            id: None,
            name: "8".into(),
        })
        .unwrap();
        worker.join().unwrap();
        std::fs::remove_file(d.path).unwrap();
    }
    #[test]
    fn normalized_snapshot_rotation_and_opaque_ids() {
        let monitor = json!([{"id":42,"name":"fixture-output","width":1920,"height":1080,"scale":1.5,"transform":1,"x":-720,"y":0,"focused":true,"activeWorkspace":{"id":3,"name":"three"},"specialWorkspace":{"id":0}}]);
        let clients = json!([{"address":"0xabc","monitor":42,"workspace":{"id":3,"name":"three"},"class":"editor","title":"test","at":[1,2],"size":[300,200]}]);
        let (d, t) = fixture(vec![
            ("j/monitors", monitor.to_string()),
            ("j/clients", clients.to_string()),
            ("j/workspaces", json!([{ "id":3,"name":"three","monitor":"fixture-output" },{"id":4,"name":"empty","monitor":"fixture-output"}]).to_string()),
        ]);
        let s = d.snapshot().unwrap();
        assert_eq!(s.outputs[0].logical.width, 720.);
        assert_eq!(s.outputs[0].logical.height, 1280.);
        assert_eq!(s.windows[0].output.as_ref(), Some(&s.outputs[0].id));
        assert!(s.windows[0].id.0.contains("synthetic-session"));
        assert_eq!(s.windows[0].hidden, None);
        assert_eq!(s.revision, 1);
        assert_eq!(s.workspaces.len(), 2);
        t.join().unwrap();
        std::fs::remove_file(d.path).unwrap();
    }
    #[test]
    fn stale_generation_never_dispatches_and_error_ack_is_failure() {
        let (d, t) = fixture(vec![("/dispatch deliberate-test", "error: denied".into())]);
        assert_eq!(
            d.close(&WindowId("[\"old-session\",\"window\",\"0xabc\"]".into())),
            Err(DesktopError::StaleTarget)
        );
        assert!(d.command("dispatch deliberate-test").is_err());
        t.join().unwrap();
        std::fs::remove_file(&d.path).unwrap();
        let _replacement = UnixListener::bind(&d.path).unwrap();
        assert_eq!(d.snapshot().unwrap_err(), DesktopError::StaleTarget);
        std::fs::remove_file(d.path).unwrap();
    }
    #[test]
    fn stalled_endpoint_times_out() {
        let path = std::env::temp_dir().join(format!("tjik-timeout-{}.sock", std::process::id()));
        let listener = UnixListener::bind(&path).unwrap();
        let worker = std::thread::spawn(move || {
            let (_stream, _) = listener.accept().unwrap();
            std::thread::sleep(Duration::from_millis(500));
        });
        assert_eq!(
            request_with_timeout(&path, "j/version", Duration::from_millis(250)),
            Err(DesktopError::Timeout)
        );
        worker.join().unwrap();
        std::fs::remove_file(path).unwrap();
    }
}

impl Hyprland {
    fn reveal(&self, id: &WindowId) -> Result<()> {
        // Reveal a scrolling column only while the requested window still owns focus.
        let client = self.target(id)?;
        if self.query("activewindow")?["address"] != client["address"] {
            return Err(DesktopError::Cancelled);
        }
        if self.query("activewindow")?["address"] == client["address"]
            && self.query("activeworkspace")?["tiledLayout"] == "scrolling"
            && client["floating"] != true
        {
            self.command(if self.lua {
                "dispatch hl.dsp.layout(\"center\")"
            } else {
                "dispatch layoutmsg center"
            })?;
        }
        Ok(())
    }
}
