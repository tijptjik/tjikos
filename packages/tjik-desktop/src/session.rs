//! One event reader per process, pinned display discovery, bounded subscriber queues.
use crate::{events::EventSource, hyprland::Hyprland, *};
use std::{
    collections::BTreeMap,
    io::Read,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, Instant},
};
trait Changes: Send {
    fn changed(&mut self) -> Result<bool>;
}
trait Connector: Send + Sync {
    fn connect(&self) -> Result<(Arc<dyn Desktop>, Box<dyn Changes>)>;
}
struct Endpoint {
    runtime: PathBuf,
    display: PathBuf,
    preferred: PathBuf,
}
struct SocketChanges {
    stream: std::os::unix::net::UnixStream,
    pending: usize,
}
impl Changes for SocketChanges {
    fn changed(&mut self) -> Result<bool> {
        self.stream
            .set_read_timeout(Some(Duration::from_millis(200)))
            .map_err(|e| DesktopError::Unavailable(e.to_string()))?;
        let mut data = [0; 8192];
        match self.stream.read(&mut data) {
            Ok(0) => Err(DesktopError::Unavailable(
                "Compositor event stream disconnected".into(),
            )),
            Ok(n) => {
                self.pending += n;
                if self.pending > 65536 {
                    return Err(DesktopError::Unavailable(
                        "Oversized compositor event".into(),
                    ));
                }
                if let Some(last) = data[..n].iter().rposition(|b| *b == b'\n') {
                    self.pending = n - last - 1;
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                Ok(false)
            }
            Err(e) => Err(DesktopError::Unavailable(e.to_string())),
        }
    }
}
impl Endpoint {
    fn open(&self, path: PathBuf) -> Result<(Arc<dyn Desktop>, Box<dyn Changes>)> {
        let desktop = Hyprland::connect_at(path, &self.display)?;
        // Subscribe before taking the first snapshot, so changes during the read
        // remain queued for reconciliation. Never reuse a disconnected stream.
        let stream = desktop.event_socket()?;
        Ok((
            Arc::new(desktop),
            Box::new(SocketChanges { stream, pending: 0 }),
        ))
    }
    fn owns_display(&self, path: &Path) -> bool {
        std::fs::read_to_string(path.with_file_name("hyprland.lock"))
            .ok()
            .and_then(|s| s.lines().nth(1).map(|v| self.runtime.join(v)))
            .as_ref()
            == Some(&self.display)
    }
}
impl Connector for Endpoint {
    fn connect(&self) -> Result<(Arc<dyn Desktop>, Box<dyn Changes>)> {
        // While the selected endpoint is alive, do not migrate to another one.
        if let Ok(connection) = self.open(self.preferred.clone()) {
            return Ok(connection);
        }
        let mut live = vec![];
        let entries = std::fs::read_dir(self.runtime.join("hypr"))
            .map_err(|e| DesktopError::Unavailable(e.to_string()))?;
        for entry in entries.take(128).flatten() {
            let path = entry.path().join(".socket.sock");
            if path == self.preferred || !self.owns_display(&path) {
                continue;
            }
            if let Ok(connection) = self.open(path) {
                live.push(connection);
            }
            if live.len() > 1 {
                return Err(DesktopError::Unavailable(
                    "Ambiguous compositor endpoints for selected display".into(),
                ));
            }
        }
        live.pop().ok_or_else(|| {
            DesktopError::Unavailable("No available compositor for selected Wayland display".into())
        })
    }
}
#[derive(Clone)]
struct Observer {
    pid: u32,
    namespace: String,
    pinned: bool,
    command: String,
}
struct State {
    backend: Option<Arc<dyn Desktop>>,
    snapshot: Option<Snapshot>,
    error: DesktopError,
}
struct Inner {
    connector: Arc<dyn Connector>,
    state: Mutex<State>,
    refresh: Mutex<()>,
    hub: EventSource,
    observers: Mutex<BTreeMap<(u32, String), Observer>>,
    shortcuts: Mutex<BTreeMap<u32, String>>,
}
pub(crate) struct Session {
    inner: Arc<Inner>,
}
impl Inner {
    fn backend(&self) -> Result<Arc<dyn Desktop>> {
        let s = self.state.lock().unwrap();
        s.backend.clone().ok_or_else(|| s.error.clone())
    }
    fn disconnected(&self, error: DesktopError) {
        let mut s = self.state.lock().unwrap();
        let was_online = s.snapshot.is_some();
        s.backend = None;
        s.snapshot = None;
        s.error = error.clone();
        if was_online {
            self.hub.publish(DesktopState::Unavailable(error), true);
        }
    }
    fn refresh(&self, resync: bool) -> Result<Snapshot> {
        let _guard = self.refresh.lock().unwrap();
        let backend = self.backend()?;
        let mut next = backend.snapshot()?;
        let mut state = self.state.lock().unwrap();
        if !state
            .backend
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, &backend))
        {
            return Err(DesktopError::Cancelled);
        }
        next.revision = state.snapshot.as_ref().map_or(0, |s| s.revision);
        if resync || state.snapshot.as_ref() != Some(&next) {
            let event = self.hub.publish(DesktopState::Ready(next), resync);
            let DesktopState::Ready(snapshot) = event.state else {
                unreachable!()
            };
            state.snapshot = Some(snapshot);
        }
        Ok(state.snapshot.clone().unwrap())
    }
    fn connect(&self) -> Result<Box<dyn Changes>> {
        let (backend, events) = self.connector.connect()?;
        self.state.lock().unwrap().backend = Some(backend.clone());
        self.refresh(true)?;
        // Replay only registrations owned by this process; never replay actions.
        for o in self.observers.lock().unwrap().values() {
            let _ = backend.observe_outside_click(o.pid, &o.namespace, o.pinned, &o.command);
        }
        for (token, command) in self.shortcuts.lock().unwrap().iter() {
            let _ = backend.recording_shortcut(*token, Some(command));
        }
        Ok(events)
    }
}
impl Session {
    pub fn from_environment() -> Result<Self> {
        let runtime = PathBuf::from(
            std::env::var_os("XDG_RUNTIME_DIR")
                .ok_or_else(|| DesktopError::Unavailable("Missing runtime directory".into()))?,
        );
        let display = runtime.join(
            std::env::var_os("WAYLAND_DISPLAY")
                .ok_or_else(|| DesktopError::Unavailable("Missing Wayland display".into()))?,
        );
        let signature = std::env::var("HYPRLAND_INSTANCE_SIGNATURE")
            .map_err(|e| DesktopError::Unavailable(e.to_string()))?;
        if signature.contains('/') || signature == ".." {
            return Err(DesktopError::Unavailable("Invalid endpoint".into()));
        }
        let preferred = runtime.join("hypr").join(signature).join(".socket.sock");
        Ok(Self::new(Arc::new(Endpoint {
            runtime,
            display,
            preferred,
        })))
    }
    fn new(connector: Arc<dyn Connector>) -> Self {
        let inner = Arc::new(Inner {
            connector,
            state: Mutex::new(State {
                backend: None,
                snapshot: None,
                error: DesktopError::Unavailable("Connecting to compositor".into()),
            }),
            refresh: Mutex::new(()),
            hub: EventSource::default(),
            observers: Mutex::new(BTreeMap::new()),
            shortcuts: Mutex::new(BTreeMap::new()),
        });
        let mut connection = match inner.connect() {
            Ok(c) => Some(c),
            Err(e) => {
                inner.disconnected(e.clone());
                inner.hub.publish(DesktopState::Unavailable(e), true);
                None
            }
        };
        let weak = Arc::downgrade(&inner);
        std::thread::spawn(move || {
            let mut retry = Duration::from_millis(100);
            let mut audit = Instant::now();
            loop {
                let Some(inner) = weak.upgrade() else {
                    break;
                };
                if let Some(events) = connection.as_mut() {
                    let changed = events.changed();
                    let result = match changed {
                        Ok(true) => {
                            std::thread::sleep(Duration::from_millis(25));
                            inner.refresh(false).map(|_| ())
                        }
                        Ok(false) if audit.elapsed() > Duration::from_secs(5) => {
                            inner.refresh(false).map(|_| ())
                        }
                        Ok(false) => continue,
                        Err(e) => Err(e),
                    };
                    audit = Instant::now();
                    if let Err(e) = result {
                        inner.disconnected(e);
                        connection = None;
                    }
                } else {
                    match inner.connect() {
                        Ok(c) => {
                            connection = Some(c);
                            retry = Duration::from_millis(100);
                            audit = Instant::now();
                        }
                        Err(e) => {
                            inner.disconnected(e);
                            drop(inner);
                            std::thread::sleep(retry);
                            retry = (retry * 2).min(Duration::from_secs(2));
                        }
                    }
                }
            }
        });
        Self { inner }
    }
}
impl DesktopSession for Session {
    fn worker_environment(&self) -> Vec<(String, String)> {
        self.inner
            .backend()
            .map(|b| b.worker_environment())
            .unwrap_or_default()
    }
    fn diagnostics(&self) -> Diagnostics {
        match self.inner.backend() {
            Ok(b) => b.diagnostics(),
            Err(e) => Diagnostics {
                adapter: "hyprland".into(),
                version: None,
                capabilities: Capabilities::default(),
                error: Some(e.to_string()),
            },
        }
    }
    fn snapshot(&self) -> Result<Snapshot> {
        self.inner.refresh(false)
    }
    fn subscribe(&self) -> Result<Subscription> {
        Ok(self.inner.hub.subscribe())
    }
}
impl WindowActions for Session {
    fn prepare_toplevel(&self, title: &str, workspace: &WorkspaceTarget, rect: Rect) -> Result<()> {
        self.inner
            .backend()?
            .prepare_toplevel(title, workspace, rect)
    }
    fn release_toplevel(&self, title: &str) -> Result<()> {
        self.inner.backend()?.release_toplevel(title)
    }

    fn capture(&self, id: &WindowId) -> Result<CapturedFrame> {
        self.inner.backend()?.capture(id)
    }
    fn activate(&self, id: &WindowId) -> Result<()> {
        self.inner.backend()?.activate(id)
    }
    fn activate_newly_mapped(&self, id: &WindowId) -> Result<()> {
        self.inner.backend()?.activate_newly_mapped(id)
    }
    fn close(&self, id: &WindowId) -> Result<()> {
        self.inner.backend()?.close(id)
    }
    fn place(&self, id: &WindowId, r: Rect) -> Result<()> {
        self.inner.backend()?.place(id, r)
    }
}
impl WorkspaceActions for Session {
    fn select_workspace(&self, target: &WorkspaceTarget) -> Result<()> {
        self.inner.backend()?.select_workspace(target)
    }
}
impl Desktop for Session {
    fn pointer(&self) -> Result<(f64, f64)> {
        self.inner.backend()?.pointer()
    }
    fn observe_outside_click(
        &self,
        pid: u32,
        namespace: &str,
        pinned: bool,
        command: &str,
    ) -> Result<()> {
        let o = Observer {
            pid,
            namespace: namespace.into(),
            pinned,
            command: command.into(),
        };
        self.inner
            .observers
            .lock()
            .unwrap()
            .insert((pid, namespace.into()), o);
        self.inner
            .backend()?
            .observe_outside_click(pid, namespace, pinned, command)
    }
    fn recording_shortcut(&self, token: u32, command: Option<&str>) -> Result<()> {
        if let Some(command) = command {
            self.inner
                .shortcuts
                .lock()
                .unwrap()
                .insert(token, command.into());
        } else {
            self.inner.shortcuts.lock().unwrap().remove(&token);
        }
        self.inner.backend()?.recording_shortcut(token, command)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    };
    struct TestChanges(Arc<Mutex<mpsc::Receiver<Result<bool>>>>);
    impl Changes for TestChanges {
        fn changed(&mut self) -> Result<bool> {
            self.0
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_millis(30))
                .unwrap_or(Ok(false))
        }
    }
    struct TestConnector {
        online: AtomicBool,
        backend: Mutex<Arc<dyn Desktop>>,
        receive: Arc<Mutex<mpsc::Receiver<Result<bool>>>>,
    }
    impl Connector for TestConnector {
        fn connect(&self) -> Result<(Arc<dyn Desktop>, Box<dyn Changes>)> {
            if !self.online.load(Ordering::SeqCst) {
                return Err(DesktopError::Unavailable("fixture offline".into()));
            }
            Ok((
                self.backend.lock().unwrap().clone(),
                Box::new(TestChanges(self.receive.clone())),
            ))
        }
    }
    #[test]
    fn push_disconnect_restart_and_no_action_replay() {
        let first = Arc::new(fake::Fake::default());
        let old = first.snapshot().unwrap().windows[0].id.clone();
        let (tx, rx) = mpsc::channel();
        let connector = Arc::new(TestConnector {
            online: AtomicBool::new(true),
            backend: Mutex::new(first.clone()),
            receive: Arc::new(Mutex::new(rx)),
        });
        let session = Session::new(connector.clone());
        let events = session.subscribe().unwrap();
        let initial = events.recv_timeout(Duration::ZERO).unwrap().unwrap();
        assert!(initial.resync);
        let mut snapshot = first.snapshot().unwrap();
        snapshot.windows[0].title = Some("renamed".into());
        first.replace(snapshot);
        tx.send(Ok(true)).unwrap();
        let updated = events
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .unwrap();
        assert!(updated.sequence > initial.sequence);
        let DesktopState::Ready(s) = updated.state else {
            panic!()
        };
        assert_eq!(s.windows[0].title.as_deref(), Some("renamed"));
        connector.online.store(false, Ordering::SeqCst);
        tx.send(Err(DesktopError::Timeout)).unwrap();
        assert!(matches!(
            events
                .recv_timeout(Duration::from_secs(1))
                .unwrap()
                .unwrap()
                .state,
            DesktopState::Unavailable(_)
        ));
        assert!(session.activate(&old).is_err());
        let second = Arc::new(fake::Fake::default());
        let mut s = second.snapshot().unwrap();
        s.generation = "fixture-two".into();
        s.windows[0].id = WindowId("second/fern".into());
        second.replace(s);
        *connector.backend.lock().unwrap() = second.clone();
        connector.online.store(true, Ordering::SeqCst);
        let recovered = events
            .recv_timeout(Duration::from_secs(3))
            .unwrap()
            .unwrap();
        assert!(recovered.resync);
        assert_eq!(session.close(&old), Err(DesktopError::StaleTarget));
        assert!(first.commands.lock().unwrap().is_empty());
        assert!(second.commands.lock().unwrap().is_empty());
        drop(session);
        assert!(events.recv_timeout(Duration::from_secs(1)).is_err());
    }
    #[test]
    fn fake_subscribers_receive_latest_state_and_generation() {
        let f = fake::Fake::default();
        let sub = f.subscribe().unwrap();
        let first = sub.recv_timeout(Duration::ZERO).unwrap().unwrap();
        for i in 0..100 {
            let mut s = f.snapshot().unwrap();
            s.outputs[0].logical.width = i as f64;
            f.replace(s);
        }
        let latest = sub.recv_timeout(Duration::ZERO).unwrap().unwrap();
        assert!(latest.resync);
        assert_eq!(latest.sequence, first.sequence + 100);
        let second = f
            .subscribe()
            .unwrap()
            .recv_timeout(Duration::ZERO)
            .unwrap()
            .unwrap();
        assert_eq!(second.sequence, latest.sequence);
    }
}
