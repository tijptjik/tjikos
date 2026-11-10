//! Compile and exercise the extension boundary as a downstream crate would.
use std::time::Duration;
use tjik_desktop::{
    Capabilities, Desktop, DesktopError, DesktopSession, DesktopState, Diagnostics, EventSource,
    Result, Snapshot, Subscription, WindowActions, WindowId, WorkspaceActions,
};

#[derive(Default)]
struct Adapter {
    events: EventSource,
}
impl DesktopSession for Adapter {
    fn diagnostics(&self) -> Diagnostics {
        Diagnostics {
            adapter: "external-fixture".into(),
            version: None,
            capabilities: Capabilities::default(),
            error: None,
        }
    }
    fn snapshot(&self) -> Result<Snapshot> {
        Err(DesktopError::Unsupported)
    }
    fn subscribe(&self) -> Result<Subscription> {
        Ok(self.events.subscribe())
    }
}
impl WindowActions for Adapter {
    fn activate(&self, _: &WindowId) -> Result<()> {
        Err(DesktopError::Unsupported)
    }
    fn close(&self, _: &WindowId) -> Result<()> {
        Err(DesktopError::Unsupported)
    }
}
impl WorkspaceActions for Adapter {}
impl Desktop for Adapter {}

#[test]
fn external_adapter_can_publish_coalesce_and_close_subscriptions() {
    let adapter = Adapter::default();
    let desktop: &dyn Desktop = &adapter;
    let early = desktop.subscribe().unwrap();
    assert!(early.recv_timeout(Duration::ZERO).unwrap().is_none());
    for revision in 1..=3 {
        let event = adapter.events.publish(
            DesktopState::Ready(Snapshot {
                generation: "fixture".into(),
                revision: 0,
                focused_window: None,
                windows: vec![],
                workspaces: vec![],
                outputs: vec![],
            }),
            false,
        );
        assert_eq!(event.sequence, revision);
        let DesktopState::Ready(snapshot) = event.state else {
            panic!("expected snapshot")
        };
        assert_eq!(snapshot.revision, revision);
    }
    let late = desktop.subscribe().unwrap();
    drop(adapter);
    for receiver in [early, late] {
        let event = receiver.recv_timeout(Duration::ZERO).unwrap().unwrap();
        assert_eq!(event.sequence, 3);
        assert!(event.resync);
        assert!(receiver.recv_timeout(Duration::ZERO).is_err());
    }
}
