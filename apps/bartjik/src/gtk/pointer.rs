//! One bounded, demand-driven pointer sampler shared by every bar.
use std::{
    sync::{Arc, Condvar, Mutex},
    time::{Duration, Instant},
};
use tjik_desktop::Desktop;
#[derive(Default)]
struct State {
    enabled: bool,
    stopped: bool,
    sample: Option<((f64, f64), Instant)>,
    error: String,
}
pub struct Pointer(Arc<(Mutex<State>, Condvar)>);
impl Pointer {
    pub fn new(desktop: Arc<dyn Desktop>) -> Self {
        let shared = Arc::new((Mutex::new(State::default()), Condvar::new()));
        let worker = shared.clone();
        std::thread::spawn(move || {
            let (lock, wake) = &*worker;
            loop {
                let mut state = lock.lock().unwrap();
                while !state.enabled && !state.stopped {
                    state = wake.wait(state).unwrap();
                }
                if state.stopped {
                    break;
                }
                drop(state);
                let result = if desktop.diagnostics().capabilities.pointer {
                    desktop.pointer()
                } else {
                    Err(tjik_desktop::DesktopError::Unsupported)
                };
                let mut state = lock.lock().unwrap();
                if state.enabled {
                    match result {
                        Ok(point) => {
                            state.sample = Some((point, Instant::now()));
                            state.error.clear();
                        }
                        Err(error) => {
                            state.sample = None;
                            state.error = error.to_string();
                        }
                    }
                }
                let delay = if state.error.is_empty() { 50 } else { 500 };
                let _ = wake
                    .wait_timeout(state, Duration::from_millis(delay))
                    .unwrap();
            }
        });
        Self(shared)
    }
    pub fn enable(&self, enabled: bool) {
        let mut state = self.0 .0.lock().unwrap();
        if state.enabled != enabled {
            state.enabled = enabled;
            state.sample = None;
            state.error.clear();
            self.0 .1.notify_one();
        }
    }
    pub fn latest(&self) -> (Option<(f64, f64)>, String) {
        let state = self.0 .0.lock().unwrap();
        let sample = state
            .sample
            .filter(|(_, at)| at.elapsed() < Duration::from_millis(1500));
        let error = if state.enabled && sample.is_none() && state.error.is_empty() {
            "Pointer data pending or stale; auto-hide suspended".into()
        } else {
            state.error.clone()
        };
        (sample.map(|(point, _)| point), error)
    }
}
impl Drop for Pointer {
    fn drop(&mut self) {
        self.0 .0.lock().unwrap().stopped = true;
        self.0 .1.notify_one();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unavailable_pointer_disables_cleanly_and_worker_exits() {
        let desktop = Arc::new(tjik_desktop::fake::Fake::default());
        let weak = Arc::downgrade(&desktop);
        let pointer = Pointer::new(desktop);
        assert_eq!(pointer.latest(), (None, String::new()));
        pointer.enable(true);
        let deadline = Instant::now() + Duration::from_secs(2);
        while pointer.0 .0.lock().unwrap().error.is_empty() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(pointer.latest().0.is_none());
        pointer.enable(false);
        assert_eq!(pointer.latest(), (None, String::new()));
        drop(pointer);
        while weak.upgrade().is_some() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    #[test]
    fn old_sample_is_never_used_to_hide_a_bar() {
        let pointer = Pointer(Arc::new((
            Mutex::new(State {
                enabled: true,
                sample: Some(((1., 2.), Instant::now() - Duration::from_secs(2))),
                ..State::default()
            }),
            Condvar::new(),
        )));
        assert!(pointer.latest().0.is_none());
        assert!(pointer.latest().1.contains("stale"));
    }
}
