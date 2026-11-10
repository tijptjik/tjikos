//! Bounded helper subscriptions. Polling remains the recovery path when unavailable.
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, BufReader},
    sync::Notify,
};
pub struct Subscription {
    healthy: Arc<AtomicBool>,
    task: tokio::task::JoinHandle<()>,
}
impl Subscription {
    pub fn start(
        program: &'static str,
        args: &[&str],
        relevant: fn(&str) -> bool,
        wake: Arc<Notify>,
    ) -> Self {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let healthy = Arc::new(AtomicBool::new(false));
        let live = healthy.clone();
        let task = tokio::spawn(async move {
            loop {
                if let Ok(mut child) = tokio::process::Command::new(program)
                    .args(&args)
                    .env("LC_ALL", "C")
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::null())
                    .kill_on_drop(true)
                    .spawn()
                {
                    let mut reader = BufReader::new(child.stdout.take().unwrap());
                    live.store(true, Ordering::Relaxed);
                    wake.notify_one();
                    loop {
                        let mut line = Vec::new();
                        match (&mut reader).take(65537).read_until(b'\n', &mut line).await {
                            Ok(n) if n > 0 && n <= 65536 => {
                                if relevant(&String::from_utf8_lossy(&line)) {
                                    wake.notify_one();
                                }
                            }
                            _ => break,
                        }
                    }
                    let _ = child.kill().await;
                    let _ = child.wait().await;
                }
                live.store(false, Ordering::Relaxed);
                wake.notify_one();
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
        });
        Self { healthy, task }
    }
    pub fn healthy(&self) -> bool {
        self.healthy.load(Ordering::Relaxed)
    }
}
impl Drop for Subscription {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn changes_wake_and_disconnect_restores_polling() {
        let wake = Arc::new(Notify::new());
        let subscription = Subscription::start(
            "/bin/sh",
            &["-c", "sleep 0.05; echo changed; sleep 0.1"],
            |line| line.trim() == "changed",
            wake.clone(),
        );
        tokio::time::timeout(std::time::Duration::from_secs(1), wake.notified())
            .await
            .unwrap();
        assert!(subscription.healthy());
        tokio::time::timeout(std::time::Duration::from_secs(1), wake.notified())
            .await
            .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            while subscription.healthy() {
                wake.notified().await;
            }
        })
        .await
        .unwrap();
        assert!(!subscription.healthy());
    }
}
