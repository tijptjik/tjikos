//! Opt-in CPU attribution for synchronous hot paths. No timers when disabled.
use std::{
    collections::BTreeMap,
    sync::{Mutex, OnceLock},
    time::Duration,
};
#[derive(Default)]
struct Counter {
    calls: u64,
    cpu_ns: u64,
}
static COUNTERS: Mutex<BTreeMap<&'static str, Counter>> = Mutex::new(BTreeMap::new());
fn cpu() -> u64 {
    let mut t = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    unsafe {
        libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut t);
    }
    t.tv_sec as u64 * 1_000_000_000 + t.tv_nsec as u64
}
pub struct Span(Option<(&'static str, u64)>);
pub fn span(name: &'static str) -> Span {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    let enabled = *ENABLED.get_or_init(|| {
        let Some(directory) = std::env::var_os("BARTJIK_PROFILE_DIR") else {
            return false;
        };
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_secs(1));
            let values: BTreeMap<_, _> = COUNTERS
                .lock()
                .unwrap()
                .iter()
                .map(|(k, v)| (*k, serde_json::json!({"calls":v.calls,"cpu_ns":v.cpu_ns})))
                .collect();
            let path =
                std::path::PathBuf::from(&directory).join(format!("{}.json", std::process::id()));
            let _ = std::fs::write(path, serde_json::to_vec(&values).unwrap());
        });
        true
    });
    Span(enabled.then(|| (name, cpu())))
}
impl Drop for Span {
    fn drop(&mut self) {
        if let Some((name, start)) = self.0 {
            let elapsed = cpu().saturating_sub(start);
            let mut counters = COUNTERS.lock().unwrap();
            let counter = counters.entry(name).or_default();
            counter.calls += 1;
            counter.cpu_ns += elapsed;
        }
    }
}

/// Attribute only time spent polling a future, excluding suspension and allowing
/// Tokio to resume it on a different thread.
pub async fn future<F: std::future::Future>(name: &'static str, work: F) -> F::Output {
    let mut work = Box::pin(work);
    std::future::poll_fn(|cx| {
        let _span = span(name);
        work.as_mut().poll(cx)
    })
    .await
}
