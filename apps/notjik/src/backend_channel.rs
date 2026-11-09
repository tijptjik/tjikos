//! Bounded in-process adapter for a native frontend's background service thread.
use crate::backend::{Request, Transport};
use anyhow::{ensure, Result};
use serde_json::Value;
use std::{sync::mpsc, time::Duration};

const CAPACITY: usize = 100;
const MAX_REQUEST_BYTES: usize = 131072;

/// Keep this handle on the UI thread and drain updates regularly.
/// Dropping it disconnects both directions, releasing a blocked service thread.
pub struct Client {
    requests: mpsc::SyncSender<Request>,
    events: mpsc::Receiver<Value>,
}
impl Client {
    /// Never block the UI thread. A full queue is an explicit error; do not retry
    /// actions automatically, since some requests have external side effects.
    pub fn submit(&self, request: Value) -> Result<()> {
        ensure!(request.is_object(), "Worker event must be an object");
        ensure!(
            serde_json::to_vec(&request)?.len() <= MAX_REQUEST_BYTES,
            "Worker request exceeds 128 KiB"
        );
        self.requests.try_send(Ok(request))?;
        Ok(())
    }

    /// Empty means no update yet; Disconnected means the runtime has stopped.
    pub fn try_recv(&self) -> std::result::Result<Value, mpsc::TryRecvError> {
        self.events.try_recv()
    }
}

struct Channel {
    requests: mpsc::Receiver<Request>,
    events: mpsc::SyncSender<Value>,
}
impl Transport for Channel {
    fn receive(
        &mut self,
        timeout: Duration,
    ) -> std::result::Result<Request, mpsc::RecvTimeoutError> {
        self.requests.recv_timeout(timeout)
    }
    fn publish(&mut self, event: &Value) -> Result<()> {
        // Backpressure is confined to the service thread. Updates are never lost.
        self.events.send(event.clone())?;
        Ok(())
    }
}

/// Move the transport into a background thread and call `backend::run` there.
/// The frontend owns that thread and must report its terminal Result. It must
/// drop Client before joining, so shutdown also works with a full update queue.
pub fn channel() -> (Client, impl Transport + Send) {
    let (requests_tx, requests_rx) = mpsc::sync_channel(CAPACITY);
    let (events_tx, events_rx) = mpsc::sync_channel(CAPACITY);
    (
        Client {
            requests: requests_tx,
            events: events_rx,
        },
        Channel {
            requests: requests_rx,
            events: events_tx,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn round_trip_preserves_protocol_and_order() {
        let (client, mut transport) = channel();
        for event in [
            json!({"op":"pausePreference","paused":true}),
            json!({"op":"history"}),
        ] {
            client.submit(event.clone()).unwrap();
            assert_eq!(transport.receive(Duration::ZERO).unwrap().unwrap(), event);
            transport.publish(&event).unwrap();
            assert_eq!(client.try_recv().unwrap(), event);
        }
        assert_eq!(client.try_recv(), Err(mpsc::TryRecvError::Empty));
    }

    #[test]
    fn input_is_bounded_and_validated_without_blocking() {
        let (client, _transport) = channel();
        assert!(client.submit(json!(null)).is_err());
        assert!(client
            .submit(json!({"text":"x".repeat(MAX_REQUEST_BYTES)}))
            .is_err());
        for _ in 0..CAPACITY {
            client.submit(json!({"op":"history"})).unwrap();
        }
        assert!(client.submit(json!({"op":"history"})).is_err());
    }

    #[test]
    fn dropping_client_releases_backpressure_and_input() {
        let (client, mut transport) = channel();
        for _ in 0..CAPACITY {
            transport.publish(&json!({"paused":true})).unwrap();
        }
        let thread = std::thread::spawn(move || {
            assert!(transport.publish(&json!({"paused":false})).is_err());
            assert_eq!(
                transport.receive(Duration::ZERO),
                Err(mpsc::RecvTimeoutError::Disconnected)
            );
        });
        drop(client);
        thread.join().unwrap();
    }

    #[test]
    fn runtime_exit_is_visible_to_frontend() {
        let (client, transport) = channel();
        drop(transport);
        assert!(client.submit(json!({"op":"history"})).is_err());
        assert_eq!(client.try_recv(), Err(mpsc::TryRecvError::Disconnected));
    }
}
