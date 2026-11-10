//! Runtjik's local preview helper adapter. The reusable view owns no process paths.
use crate::previews::{PagePreview, PreviewFuture, PreviewProvider};
use std::{path::PathBuf, rc::Rc};
use tjik_gtk::gtk::{gio, glib};

struct Helper(PathBuf);
struct Running {
    process: gio::Subprocess,
    deadline: Option<glib::SourceId>,
    complete: bool,
}
impl Drop for Running {
    fn drop(&mut self) {
        if let Some(deadline) = self.deadline.take() {
            deadline.remove();
        }
        if !self.complete {
            // The native helper handles TERM by dropping its browser process-group guard.
            self.process.send_signal(15);
        }
    }
}

/// A host-configured local preview helper; no application paths are inferred.
pub fn provider(helper: PathBuf) -> Rc<dyn PreviewProvider> {
    Rc::new(Helper(helper))
}

impl PreviewProvider for Helper {
    fn load(&self, urls: Vec<String>) -> PreviewFuture {
        let helper = self.0.clone();
        Box::pin(async move {
            let Ok(process) = gio::Subprocess::newv(
                &[helper.as_os_str(), "--batch".as_ref()],
                gio::SubprocessFlags::STDIN_PIPE
                    | gio::SubprocessFlags::STDOUT_PIPE
                    | gio::SubprocessFlags::STDERR_SILENCE,
            ) else {
                return vec![];
            };
            let timed = process.clone();
            // Four URLs, each bounded by the helper's fetch/render deadlines.
            let deadline =
                glib::timeout_add_local(std::time::Duration::from_secs(180), move || {
                    timed.send_signal(15);
                    glib::ControlFlow::Continue
                });
            let mut running = Running {
                process,
                deadline: Some(deadline),
                complete: false,
            };
            let input = glib::Bytes::from_owned(serde_json::to_vec(&urls).unwrap());
            let result = running.process.communicate_future(Some(&input)).await;
            running.complete = result.is_ok();
            let Ok((Some(output), _)) = result else {
                return vec![];
            };
            String::from_utf8_lossy(&output)
                .lines()
                .filter_map(|line| {
                    let value: serde_json::Value = serde_json::from_str(line).ok()?;
                    let url = value["url"].as_str()?;
                    if !urls.iter().any(|u| u == url) {
                        return None;
                    }
                    Some(PagePreview {
                        url: url.into(),
                        description: value["preview"]["description"]
                            .as_str()
                            .unwrap_or("")
                            .into(),
                        image: value["preview"]["image"].as_str().unwrap_or("").into(),
                    })
                })
                .collect()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helper_results_and_cancellation_are_owned_by_the_future() {
        let directory = tempfile::tempdir().unwrap();
        let helper = directory.path().join("helper.py");
        let started = directory.path().join("started");
        let stopped = directory.path().join("stopped");
        std::fs::write(
            &helper,
            format!(
                r#"#!/usr/bin/env python3
import json, signal, sys, time
from pathlib import Path
def stop(signum, frame):
    Path({stopped:?}).write_text('stopped')
    raise SystemExit(0)
signal.signal(signal.SIGTERM, stop)
urls = json.load(sys.stdin)
if urls == ['https://example.test/ok']:
    print(json.dumps({{'url': urls[0], 'preview': {{'description': 'synthetic'}}}}))
else:
    Path({started:?}).write_text('started')
    time.sleep(30)
"#,
                stopped = stopped.to_str().unwrap(),
                started = started.to_str().unwrap()
            ),
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o700)).unwrap();
        let context = glib::MainContext::new();
        context
            .with_thread_default(|| {
                context.block_on(async {
                    let provider = Helper(helper);
                    let results = provider.load(vec!["https://example.test/ok".into()]).await;
                    assert_eq!(results.len(), 1);
                    assert_eq!(results[0].description, "synthetic");
                    let task = context
                        .spawn_local(provider.load(vec!["https://example.test/wait".into()]));
                    for _ in 0..200 {
                        if started.exists() {
                            break;
                        }
                        glib::timeout_future(std::time::Duration::from_millis(10)).await;
                    }
                    assert!(started.exists(), "synthetic helper did not start");
                    task.abort();
                    drop(task);
                    for _ in 0..200 {
                        if stopped.exists() {
                            break;
                        }
                        glib::timeout_future(std::time::Duration::from_millis(10)).await;
                    }
                    assert!(
                        stopped.exists(),
                        "dropping the future did not terminate its helper"
                    );
                })
            })
            .unwrap();
    }
}
