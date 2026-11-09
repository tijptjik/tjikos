//! One bounded, cancellable worker while the shared network card is mapped.
use crate::network::{self, Action, Adapter, Error, Snapshot, Traffic};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
pub enum Update {
    Snapshot(Result<Snapshot, Error>, bool),
    Traffic(Traffic),
}
pub struct Session {
    commands: tokio::sync::mpsc::Sender<Action>,
    stop: tokio::sync::watch::Sender<bool>,
}
impl Session {
    pub fn send(&self, action: Action) -> Result<(), &'static str> {
        self.commands
            .try_send(action)
            .map_err(|_| "A network request is already pending")
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.stop.send(true);
    }
}
pub fn start(adapter: Arc<dyn Adapter>) -> (Session, async_channel::Receiver<Update>) {
    let (commands, mut requests) = tokio::sync::mpsc::channel(1);
    let (stop, mut cancelled) = tokio::sync::watch::channel(false);
    let (send, updates) = async_channel::bounded(2);
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("network runtime");
        runtime.block_on(async move {
            let work = async {
                if send.send(Update::Snapshot(adapter.snapshot().await, false)).await.is_err() { return; }
                let mut status = tokio::time::interval_at(tokio::time::Instant::now() + Duration::from_secs(10), Duration::from_secs(10));
                let mut traffic = tokio::time::interval(Duration::from_secs(1));
                status.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                traffic.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                let mut previous = Traffic::new();
                let mut since = Instant::now();
                loop {
                    let update = tokio::select! {
                        action = requests.recv() => match action { Some(action) => Update::Snapshot(adapter.execute(action).await, true), None => break },
                        _ = status.tick() => Update::Snapshot(adapter.snapshot().await, false),
                        _ = traffic.tick(), if adapter.capabilities().traffic => {
                            let current = adapter.counters().await.unwrap_or_default();
                            let rate = network::rates(&current, &previous, since.elapsed().as_secs_f64());
                            previous = current; since = Instant::now();
                            Update::Traffic(rate)
                        }
                    };
                    if send.send(update).await.is_err() { break; }
                }
            };
            tokio::select! { _ = cancelled.changed() => {}, _ = work => {} }
        });
    });
    (Session { commands, stop }, updates)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dropping_hidden_card_stops_worker_and_closes_updates() {
        let adapter = Arc::new(network::Fake::demo());
        let (session, updates) = start(adapter.clone());
        assert!(matches!(
            updates.recv_blocking().unwrap(),
            Update::Snapshot(Ok(_), _)
        ));
        session.send(Action::Scan).unwrap();
        loop {
            if matches!(updates.recv_blocking().unwrap(), Update::Snapshot(_, true)) {
                break;
            }
        }
        assert_eq!(*adapter.commands.lock().unwrap(), vec!["scan"]);
        drop(session);
        let deadline = Instant::now() + Duration::from_secs(2);
        while !updates.is_closed() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(updates.is_closed(), "hidden card retained its worker");
    }
}
