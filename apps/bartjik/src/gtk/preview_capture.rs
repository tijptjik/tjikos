//! Independent, bounded capture streams. GTK polls completed frames; no worker
//! owns a widget, and dropping a worker cancels queued work and releases buffers.
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    time::{Duration, Instant},
};
use tjik_desktop::{CapturedFrame, Desktop, Result, WindowId};

struct Worker {
    requests: mpsc::SyncSender<(u32, u32)>,
    results: mpsc::Receiver<Result<CapturedFrame>>,
    cancelled: Arc<AtomicBool>,
    pending: bool,
    due: Instant,
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}
impl Worker {
    fn new(desktop: Arc<dyn Desktop>, id: WindowId) -> Self {
        let (requests, incoming) = mpsc::sync_channel(1);
        let (outgoing, results) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let stop = cancelled.clone();
        std::thread::spawn(move || {
            let mut stream = None;
            while let Ok((width, height)) = incoming.recv() {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                let result = (|| {
                    if stream.is_none() {
                        stream = Some(desktop.clone().capture_stream(&id)?);
                    }
                    stream
                        .as_mut()
                        .unwrap()
                        .capture()
                        .map(|frame| crate::thumbnail::resample(frame, width, height))
                })();
                if result.is_err() {
                    stream = None;
                }
                if stop.load(Ordering::Relaxed) || outgoing.send(result).is_err() {
                    break;
                }
            }
        });
        Self {
            requests,
            results,
            cancelled,
            pending: false,
            due: Instant::now(),
        }
    }
}
#[derive(Default)]
pub struct Previews {
    desktop: Option<Arc<dyn Desktop>>,
    workers: BTreeMap<String, Worker>,
}
impl Previews {
    pub fn attach(&mut self, desktop: Arc<dyn Desktop>) {
        self.clear();
        self.desktop = Some(desktop);
    }
    pub fn clear(&mut self) {
        self.workers.clear();
    }
    pub fn refresh(
        &mut self,
        targets: &[(WindowId, u32, u32)],
        interval: Duration,
    ) -> Vec<(WindowId, Result<CapturedFrame>)> {
        let Some(desktop) = self.desktop.as_ref() else {
            return vec![];
        };
        self.workers
            .retain(|id, _| targets.iter().any(|t| t.0 .0 == *id));
        let now = Instant::now();
        let mut ready = Vec::new();
        for (id, width, height) in targets {
            let worker = self
                .workers
                .entry(id.0.clone())
                .or_insert_with(|| Worker::new(desktop.clone(), id.clone()));
            if let Ok(result) = worker.results.try_recv() {
                worker.pending = false;
                if result.is_err() {
                    worker.due = now + Duration::from_secs(2);
                }
                ready.push((id.clone(), result));
            }
            if !worker.pending && now >= worker.due && *width > 0 && *height > 0 {
                if worker.requests.try_send((*width, *height)).is_ok() {
                    worker.pending = true;
                    worker.due = now + interval;
                }
            }
        }
        ready
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use tjik_desktop::*;
    #[derive(Default)]
    struct Animated {
        opened: AtomicUsize,
        frames: AtomicUsize,
        live: AtomicUsize,
    }
    struct Stream(Arc<Animated>, String);
    impl Drop for Stream {
        fn drop(&mut self) {
            self.0.live.fetch_sub(1, Ordering::SeqCst);
        }
    }
    impl CaptureStream for Stream {
        fn capture(&mut self) -> Result<CapturedFrame> {
            if self.1 == "slow" {
                std::thread::sleep(Duration::from_millis(200));
            }
            if self.1 == "failed" {
                return Err(DesktopError::Timeout);
            }
            let n = self.0.frames.fetch_add(1, Ordering::SeqCst);
            Ok(CapturedFrame {
                width: 1,
                height: 1,
                stride: 4,
                rgba: vec![n as u8, 0, 0, 255],
            })
        }
    }
    impl DesktopSession for Animated {
        fn diagnostics(&self) -> Diagnostics {
            fake::Fake::default().diagnostics()
        }
        fn snapshot(&self) -> Result<Snapshot> {
            fake::Fake::default().snapshot()
        }
    }
    impl WindowActions for Animated {
        fn capture_stream(self: Arc<Self>, id: &WindowId) -> Result<Box<dyn CaptureStream>> {
            self.opened.fetch_add(1, Ordering::SeqCst);
            self.live.fetch_add(1, Ordering::SeqCst);
            Ok(Box::new(Stream(self, id.0.clone())))
        }
        fn activate(&self, _: &WindowId) -> Result<()> {
            Err(DesktopError::Unsupported)
        }
        fn close(&self, _: &WindowId) -> Result<()> {
            Err(DesktopError::Unsupported)
        }
    }
    impl WorkspaceActions for Animated {}
    impl Desktop for Animated {}
    #[test]
    fn streams_refresh_independently_reuse_and_stop() {
        let desktop = Arc::new(Animated::default());
        let mut previews = Previews::default();
        previews.attach(desktop.clone());
        let targets: Vec<_> = ["fast", "slow", "failed"]
            .map(|id| (WindowId(id.into()), 1, 1))
            .into();
        let until = Instant::now() + Duration::from_millis(450);
        let mut fast = 0;
        let mut failed = 0;
        while Instant::now() < until {
            for (id, frame) in previews.refresh(&targets, Duration::from_millis(33)) {
                if id.0 == "fast" && frame.is_ok() {
                    fast += 1;
                }
                if id.0 == "failed" && frame.is_err() {
                    failed += 1;
                }
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(fast >= 6, "slow capture blocked healthy window: {fast}");
        assert_eq!(failed, 1, "failure retries must be throttled per window");
        assert_eq!(
            desktop.opened.load(Ordering::SeqCst),
            3,
            "reuse healthy streams"
        );
        previews.clear();
        let deadline = Instant::now() + Duration::from_secs(1);
        while desktop.live.load(Ordering::SeqCst) != 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(
            desktop.live.load(Ordering::SeqCst),
            0,
            "hidden preview retained streams"
        );
        // A reopened preview must not receive a delayed frame from the old worker.
        let result = previews.refresh(&targets[..1], Duration::from_millis(33));
        assert!(result.is_empty());
    }
}
