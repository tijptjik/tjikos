//! Ordered full-state events. One pending state per subscriber bounds memory.
//! Coalescing sets `resync`: consumers replace state, never replay lost deltas.
use crate::*;
use std::sync::{Condvar, Mutex, Weak};
use std::time::Duration;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DesktopState {
    Ready(Snapshot),
    Unavailable(DesktopError),
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DesktopEvent {
    pub sequence: u64,
    pub resync: bool,
    pub state: DesktopState,
}
#[derive(Default)]
struct Slot {
    pending: Option<DesktopEvent>,
    closed: bool,
}
#[derive(Default)]
struct Mailbox {
    slot: Mutex<Slot>,
    wake: Condvar,
}
/// A bounded receiver. Dropping it unregisters the consumer lazily.
/// Pending state is delivered before a closed-source error.
pub struct Subscription {
    mailbox: Arc<Mailbox>,
}
impl Subscription {
    pub fn recv_timeout(&self, timeout: Duration) -> Result<Option<DesktopEvent>> {
        let slot = self.mailbox.slot.lock().unwrap();
        let (mut slot, _) = self
            .mailbox
            .wake
            .wait_timeout_while(slot, timeout, |s| s.pending.is_none() && !s.closed)
            .unwrap();
        if let Some(event) = slot.pending.take() {
            return Ok(Some(event));
        }
        if slot.closed {
            return Err(DesktopError::Unavailable("Event source closed".into()));
        }
        Ok(None)
    }
}
#[derive(Default)]
struct HubState {
    sequence: u64,
    latest: Option<DesktopEvent>,
    listeners: Vec<Weak<Mailbox>>,
}
#[derive(Default)]
/// Adapter-owned source of ordered, full-state desktop events.
///
/// Keep one source for the lifetime of an adapter session. Subscribers immediately
/// receive the latest state, if any. Slow consumers retain only the newest state,
/// marked for resynchronization. Dropping the source wakes and closes receivers.
/// No producer thread or async runtime is created by this type.
pub struct EventSource(Mutex<HubState>);
impl EventSource {
    /// Publish a complete state and assign its sequence and snapshot revision.
    /// Set `resync` after reconnecting or changing the adapter generation.
    /// The returned event contains the assigned revision for adapter snapshot caches.
    pub fn publish(&self, mut state: DesktopState, resync: bool) -> DesktopEvent {
        let mut hub = self.0.lock().unwrap();
        hub.sequence += 1;
        if let DesktopState::Ready(snapshot) = &mut state {
            snapshot.revision = hub.sequence;
        }
        let event = DesktopEvent {
            sequence: hub.sequence,
            resync,
            state,
        };
        hub.latest = Some(event.clone());
        hub.listeners.retain(|weak| {
            let Some(mailbox) = weak.upgrade() else {
                return false;
            };
            let mut slot = mailbox.slot.lock().unwrap();
            let mut next = event.clone();
            next.resync |= slot.pending.is_some();
            slot.pending = Some(next);
            mailbox.wake.notify_one();
            true
        });
        event
    }
    /// Subscribe without blocking or performing adapter I/O.
    pub fn subscribe(&self) -> Subscription {
        let mut hub = self.0.lock().unwrap();
        hub.listeners.retain(|listener| listener.strong_count() > 0);
        let mailbox = Arc::new(Mailbox::default());
        mailbox.slot.lock().unwrap().pending = hub.latest.clone().map(|mut e| {
            e.resync = true;
            e
        });
        hub.listeners.push(Arc::downgrade(&mailbox));
        Subscription { mailbox }
    }
}
impl Drop for EventSource {
    fn drop(&mut self) {
        for mailbox in self
            .0
            .get_mut()
            .unwrap()
            .listeners
            .iter()
            .filter_map(Weak::upgrade)
        {
            mailbox.slot.lock().unwrap().closed = true;
            mailbox.wake.notify_all();
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn slow_consumers_resync_and_teardown_wakes() {
        let hub = EventSource::default();
        let sub = hub.subscribe();
        for _ in 0..1000 {
            hub.publish(DesktopState::Unavailable(DesktopError::Timeout), false);
        }
        let e = sub.recv_timeout(Duration::ZERO).unwrap().unwrap();
        assert_eq!(e.sequence, 1000);
        assert!(e.resync);
        assert!(sub.recv_timeout(Duration::ZERO).unwrap().is_none());
        drop(hub);
        assert!(sub.recv_timeout(Duration::from_secs(1)).is_err());
    }
}
