//! One coalesced provider set per process; no helpers or blocking work on GTK.
use crate::{config, datetime};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    sync::{Arc, Condvar, Mutex},
    time::{Duration, Instant},
};
#[derive(Clone, Debug)]
pub enum Reading {
    Ready(datetime::Snapshot),
    Error(String),
}
#[derive(Default)]
struct State {
    specs: BTreeMap<String, Value>,
    readings: BTreeMap<String, Reading>,
    epoch: u64,
    stopped: bool,
}
pub struct Service(
    Arc<(Mutex<State>, Condvar, tokio::sync::Notify)>,
    Mutex<Option<std::thread::JoinHandle<()>>>,
);
impl Service {
    pub fn new(demo: bool) -> Self {
        let shared = Arc::new((
            Mutex::new(State::default()),
            Condvar::new(),
            tokio::sync::Notify::new(),
        ));
        let thread = shared.clone();
        let worker = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("clock runtime");
            let mut due = BTreeMap::<String, Instant>::new();
            let mut last_epoch = 0;
            loop {
                let (lock, wake, _) = &*thread;
                let mut state = lock.lock().unwrap();
                while state.specs.is_empty() && !state.stopped {
                    state = wake.wait(state).unwrap();
                }
                if state.stopped {
                    break;
                }
                if last_epoch != state.epoch {
                    due.clear();
                    last_epoch = state.epoch;
                }
                let specs: Vec<_> = state
                    .specs
                    .iter()
                    .filter(|(key, _)| due.get(*key).is_none_or(|at| *at <= Instant::now()))
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                let epoch = state.epoch;
                drop(state);
                for batch in specs.chunks(4) {
                    {
                        let state = thread.0.lock().unwrap();
                        if state.stopped || state.epoch != epoch {
                            break;
                        }
                    }
                    let results=runtime.block_on(async {
                        let mut jobs=tokio::task::JoinSet::new();
                        for (key,spec) in batch.iter().cloned() {
                            jobs.spawn(async move {
                                let value=datetime::read(&spec,if demo {Some(1781524800)}else{None}).await;
                                (key,spec,value)
                            });
                        }
                        tokio::select! {
                            _=thread.2.notified()=>vec![],
                            values=async {let mut values=vec![]; while let Some(Ok(value))=jobs.join_next().await {values.push(value);} values}=>values,
                        }
                    });
                    let mut state = lock.lock().unwrap();
                    if state.epoch != epoch {
                        break;
                    }
                    for (key, spec, value) in results {
                        due.insert(
                            key.clone(),
                            Instant::now()
                                + Duration::from_secs_f64(
                                    spec["interval"].as_f64().unwrap_or(1.).clamp(0.1, 86400.),
                                ),
                        );
                        state.readings.insert(
                            key,
                            match value {
                                Ok(v) => Reading::Ready(v),
                                Err(e) => Reading::Error(e.to_string()),
                            },
                        );
                    }
                }
                let state = lock.lock().unwrap();
                if state.stopped {
                    break;
                }
                if state.epoch != epoch {
                    continue;
                }
                let delay = due
                    .values()
                    .map(|at| at.saturating_duration_since(Instant::now()))
                    .min()
                    .unwrap_or(Duration::from_millis(100));
                let _ = wake
                    .wait_timeout(state, delay.max(Duration::from_millis(10)))
                    .unwrap();
            }
        });
        Self(shared, Mutex::new(Some(worker)))
    }
    pub fn configure(&self, specs: impl Iterator<Item = Value>) {
        let specs: BTreeMap<_, _> = specs
            .map(|s| (config::provider_key(&s), config::provider_config(&s)))
            .collect();
        let mut state = self.0 .0.lock().unwrap();
        if state.specs != specs {
            let previous = state.specs.clone();
            state
                .readings
                .retain(|key, _| specs.get(key) == previous.get(key));
            state.specs = specs;
            state.epoch += 1;
            self.0 .1.notify_one();
            self.0 .2.notify_one();
        }
    }
    pub fn stop(&self) {
        self.0 .0.lock().unwrap().stopped = true;
        self.0 .1.notify_one();
        self.0 .2.notify_one();
        if let Some(worker) = self.1.lock().unwrap().take() {
            let _ = worker.join();
        }
    }
    pub fn readings(&self) -> BTreeMap<String, Reading> {
        self.0 .0.lock().unwrap().readings.clone()
    }
    pub fn count(&self) -> usize {
        self.0 .0.lock().unwrap().specs.len()
    }
}
impl Drop for Service {
    fn drop(&mut self) {
        self.stop();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn deduplication_reload_and_failure_recovery() {
        let service = Service::new(true);
        let spec = json!({"provider":"datetime","timezone":"UTC","locale":"C","format":"%H:%M"});
        service.configure([spec.clone(), spec.clone()].into_iter());
        assert_eq!(service.count(), 1);
        let wait = |service: &Service| {
            let until = Instant::now() + Duration::from_secs(3);
            while service.readings().is_empty() {
                assert!(Instant::now() < until);
                std::thread::sleep(Duration::from_millis(10));
            }
        };
        wait(&service);
        assert!(matches!(
            service.readings().values().next(),
            Some(Reading::Ready(_))
        ));
        let invalid = json!({"provider":"datetime","format":"a\nb"});
        service.configure([invalid].into_iter());
        wait(&service);
        assert!(matches!(
            service.readings().values().next(),
            Some(Reading::Error(_))
        ));
        service.configure([spec].into_iter());
        wait(&service);
        assert!(matches!(
            service.readings().values().next(),
            Some(Reading::Ready(_))
        ));
        service.configure(std::iter::empty());
        assert_eq!(service.count(), 0);
        assert!(service.readings().is_empty());
    }
}
