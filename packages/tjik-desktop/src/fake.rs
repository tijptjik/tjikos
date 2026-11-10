//! Deterministic, host-independent adapter with replaceable generations and receipts.
use crate::*;
use std::sync::Mutex;
pub struct Fake {
    state: Mutex<Snapshot>,
    events: events::EventSource,
    pub commands: Mutex<Vec<(String, WindowId)>>,
    pub capabilities: Capabilities,
    pub workspace_commands: Mutex<Vec<WorkspaceTarget>>,
}
impl Default for Fake {
    fn default() -> Self {
        let fake = Self {
            state: Mutex::new(Snapshot {
                generation: "fixture-one".into(),
                revision: 1,
                workspaces: vec![
                    WorkspaceState {
                        workspace: Workspace {
                            id: "desk-blue".into(),
                            name: Some("Blue".into()),
                        },
                        output: Some("output-north".into()),
                        special: false,
                    },
                    WorkspaceState {
                        workspace: Workspace {
                            id: "desk-empty".into(),
                            name: Some("Empty".into()),
                        },
                        output: Some("output-north".into()),
                        special: false,
                    },
                ],
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
                focused_window: None,
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
            events: events::EventSource::default(),
            commands: Mutex::new(vec![]),
            workspace_commands: Mutex::new(vec![]),
            capabilities: Capabilities {
                windows: true,
                workspace_select: true,
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
impl WorkspaceActions for Fake {
    fn select_workspace(&self, target: &WorkspaceTarget) -> Result<()> {
        if !self.capabilities.workspace_select {
            return Err(DesktopError::Unsupported);
        }
        let mut state = self.state.lock().unwrap();
        if target.generation != state.generation
            || !state.outputs.iter().any(|o| o.id == target.output)
        {
            return Err(DesktopError::StaleTarget);
        }
        let workspace = if let Some(id) = &target.id {
            state
                .workspaces
                .iter()
                .find(|w| w.workspace.id == *id && w.output.as_ref() == Some(&target.output))
                .map(|w| w.workspace.clone())
                .ok_or(DesktopError::StaleTarget)?
        } else {
            if target.name.is_empty() || target.name.chars().any(char::is_control) {
                return Err(DesktopError::Unavailable("Invalid workspace name".into()));
            }
            let workspace = Workspace {
                id: format!("{}/workspace/{}", state.generation, target.name),
                name: Some(target.name.clone()),
            };
            state.workspaces.push(WorkspaceState {
                workspace: workspace.clone(),
                output: Some(target.output.clone()),
                special: false,
            });
            workspace
        };
        state
            .outputs
            .iter_mut()
            .find(|o| o.id == target.output)
            .unwrap()
            .workspace = Some(workspace);
        self.workspace_commands.lock().unwrap().push(target.clone());
        let event = self
            .events
            .publish(DesktopState::Ready(state.clone()), false);
        let DesktopState::Ready(snapshot) = event.state else {
            unreachable!()
        };
        *state = snapshot;
        Ok(())
    }
}
impl Desktop for Fake {}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn empty_workspace_selection_rejects_old_generations_and_missing_capabilities() {
        let mut fake = Fake::default();
        let mut target = WorkspaceTarget {
            generation: "fixture-one".into(),
            output: "output-north".into(),
            id: Some("desk-empty".into()),
            name: "Empty".into(),
        };
        fake.select_workspace(&target).unwrap();
        assert_eq!(
            fake.snapshot().unwrap().outputs[0]
                .workspace
                .as_ref()
                .unwrap()
                .id,
            "desk-empty"
        );
        target.generation = "old-session".into();
        assert_eq!(
            fake.select_workspace(&target),
            Err(DesktopError::StaleTarget)
        );
        target.generation = "fixture-one".into();
        target.id = None;
        target.name = "7".into();
        fake.select_workspace(&target).unwrap();
        assert_eq!(
            fake.snapshot().unwrap().outputs[0]
                .workspace
                .as_ref()
                .unwrap()
                .name
                .as_deref(),
            Some("7")
        );
        fake.capabilities.workspace_select = false;
        assert_eq!(
            fake.select_workspace(&target),
            Err(DesktopError::Unsupported)
        );
        assert_eq!(fake.workspace_commands.lock().unwrap().len(), 2);
    }
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
