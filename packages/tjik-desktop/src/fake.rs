//! Deterministic, host-independent adapter with replaceable generations and receipts.
use crate::*;
use std::sync::Mutex;
pub struct Fake {
    state: Mutex<Snapshot>,
    events: events::Hub,
    pub commands: Mutex<Vec<(String, WindowId)>>,
    pub capabilities: Capabilities,
}
impl Default for Fake {
    fn default() -> Self {
        let fake = Self {
            state: Mutex::new(Snapshot {
                generation: "fixture-one".into(),
                revision: 1,
                outputs: vec![Output {
                    id: "output-north".into(),
                    name: "Synthetic".into(),
                    description: Some("Synthetic output".into()),
                    logical: Rect {
                        x: 0.,
                        y: 0.,
                        width: 1280.,
                        height: 720.,
                    },
                    width: 1920,
                    height: 1080,
                    scale: 1.5,
                    transform: 0,
                    focused: true,
                    workspace: Some(Workspace {
                        id: "desk-blue".into(),
                        name: Some("Blue".into()),
                    }),
                }],
                windows: vec![Window {
                    id: WindowId("fixture-one/window-fern".into()),
                    title: Some("Synthetic editor".into()),
                    app_id: Some("editor".into()),
                    initial_app_id: None,
                    pid: None,
                    output: Some("output-north".into()),
                    workspace: Some(Workspace {
                        id: "desk-blue".into(),
                        name: Some("Blue".into()),
                    }),
                    geometry: None,
                    mapped: Some(true),
                    hidden: Some(false),
                    floating: None,
                    focus_order: Some(0),
                }],
            }),
            events: events::Hub::default(),
            commands: Mutex::new(vec![]),
            capabilities: Capabilities {
                windows: true,
                activate: true,
                close: true,
                outputs: true,
                ..Capabilities::default()
            },
        };
        fake.events
            .publish(DesktopState::Ready(fake.snapshot().unwrap()), true);
        fake
    }
}
impl Fake {
    pub fn replace(&self, snapshot: Snapshot) {
        let mut state = self.state.lock().unwrap();
        let resync = state.generation != snapshot.generation;
        let event = self.events.publish(DesktopState::Ready(snapshot), resync);
        let DesktopState::Ready(snapshot) = event.state else {
            unreachable!()
        };
        *state = snapshot;
    }
    fn action(&self, action: &str, id: &WindowId, supported: bool) -> Result<()> {
        if !supported {
            return Err(DesktopError::Unsupported);
        }
        let state = self.state.lock().unwrap();
        if !state.windows.iter().any(|w| &w.id == id) {
            return Err(DesktopError::StaleTarget);
        }
        self.commands
            .lock()
            .unwrap()
            .push((action.into(), id.clone()));
        Ok(())
    }
}
impl DesktopSession for Fake {
    fn subscribe(&self) -> Result<Subscription> {
        Ok(self.events.subscribe())
    }
    fn diagnostics(&self) -> Diagnostics {
        Diagnostics {
            adapter: "fake".into(),
            version: None,
            capabilities: self.capabilities.clone(),
            error: None,
        }
    }
    fn snapshot(&self) -> Result<Snapshot> {
        Ok(self.state.lock().unwrap().clone())
    }
}
impl WindowActions for Fake {
    fn activate(&self, id: &WindowId) -> Result<()> {
        self.action("activate", id, self.capabilities.activate)
    }
    fn close(&self, id: &WindowId) -> Result<()> {
        self.action("close", id, self.capabilities.close)
    }
}
impl Desktop for Fake {}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn opaque_targets_and_restart() {
        let f = Fake::default();
        let mut s = f.snapshot().unwrap();
        let old = s.windows[0].id.clone();
        f.activate(&old).unwrap();
        s.generation = "fixture-two".into();
        s.windows[0].id = WindowId("fixture-two/window-fern".into());
        f.replace(s);
        assert_eq!(f.close(&old), Err(DesktopError::StaleTarget));
        assert_eq!(f.commands.lock().unwrap().len(), 1);
        assert_eq!(f.pointer(), Err(DesktopError::Unsupported));
    }
    #[test]
    fn unavailable_capabilities_do_not_record_success() {
        let mut f = Fake::default();
        f.capabilities.activate = false;
        assert_eq!(
            f.activate(&f.snapshot().unwrap().windows[0].id),
            Err(DesktopError::Unsupported)
        );
        assert!(f.commands.lock().unwrap().is_empty());
    }
}
