//! Single-owner media/action execution. Worker threads never touch GTK objects.
use super::model::{local_path, Playback, Recording};
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Child, Command, Stdio},
    sync::mpsc,
    time::Instant,
};
pub struct Completion {
    pub id: String,
    pub kind: String,
    pub result: Value,
}
pub struct Capture {
    pub id: String,
    pub state: Recording,
    pub destination: Value,
    pub token: String,
    pub send_when_ready: bool,
    child: Child,
}
struct Audio {
    id: String,
    child: Child,
    started: Instant,
    start_ms: u64,
    rate: f64,
}
pub struct Executor {
    pub tx: mpsc::Sender<Completion>,
    pub rx: mpsc::Receiver<Completion>,
    pub capture: Option<Capture>,
    audio: Option<Audio>,
    pub playback: HashMap<String, Playback>,
}
impl Executor {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            tx,
            rx,
            capture: None,
            audio: None,
            playback: HashMap::new(),
        }
    }
    pub fn action(&self, id: String, kind: String, request: Value, demo: bool) {
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let result = if demo {
                json!({"ok":true,"message":"Demo only · no Telegram action sent"})
            } else {
                crate::telegram::socket_request(&request).unwrap_or_else(|e|json!({"ok":false,"uncertain":kind=="reply"||kind=="voiceReply","error":e.to_string()}))
            };
            let _ = tx.send(Completion { id, kind, result });
        });
    }
    pub fn command(&self, id: String, kind: String, args: Vec<String>) {
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let result = crate::util::run(
                Command::new(&args[0]).args(&args[1..]),
                std::time::Duration::from_secs(30),
            )
            .map(|_| json!({"ok":true}))
            .unwrap_or_else(|e| json!({"ok":false,"error":e.to_string()}));
            let _ = tx.send(Completion { id, kind, result });
        });
    }
    pub fn record(&mut self, root: &Path, id: String, n: &Value) -> Result<()> {
        ensure!(self.capture.is_none(), "A recording is already active");
        ensure!(
            n["data"]["demo"] != true,
            "Microphone is disabled for demo cards"
        );
        self.stop_audio();
        let mut child = Command::new(root.join("bin/notjik-core"))
            .arg("record")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let output = child
            .stdout
            .take()
            .context("Recording output unavailable")?;
        let tx = self.tx.clone();
        let key = id.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(output).lines() {
                match line
                    .ok()
                    .and_then(|l| serde_json::from_str::<Value>(&l).ok())
                {
                    Some(v) => {
                        if tx
                            .send(Completion {
                                id: key.clone(),
                                kind: "capture".into(),
                                result: v,
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                    None => break,
                }
            }
            let _ = tx.send(Completion {
                id: key,
                kind: "capture-exited".into(),
                result: json!({}),
            });
        });
        self.capture = Some(Capture {
            id,
            state: Recording {
                phase: "starting".into(),
                ..Default::default()
            },
            destination: n["data"].clone(),
            token: String::new(),
            send_when_ready: false,
            child,
        });
        Ok(())
    }
    pub fn stop_recording(&mut self, send: bool) -> Result<()> {
        if let Some(c) = &mut self.capture {
            c.send_when_ready = send;
            c.state.phase = "stopping".into();
            if let Some(stdin) = &mut c.child.stdin {
                writeln!(stdin, "{}", json!({"action":"stop"}))?;
            }
        }
        Ok(())
    }
    pub fn discard(&mut self) {
        // A bridge request may still be reading the capture. Never delete it mid-send.
        if self
            .capture
            .as_ref()
            .is_some_and(|c| c.state.phase == "sending" || c.state.uncertain)
        {
            return;
        }
        if let Some(mut c) = self.capture.take() {
            if let Some(stdin) = &mut c.child.stdin {
                let _ = writeln!(stdin, "{}", json!({"action":"discard"}));
            }
            c.child.stdin.take();
            std::thread::spawn(move || {
                let _ = c.child.wait();
            });
        }
    }
    pub fn send_voice(&mut self) -> Result<()> {
        let c = self.capture.as_mut().context("No recording")?;
        ensure!(
            !c.state.uncertain,
            "Check the chat before retrying an uncertain send"
        );
        if c.state.phase == "recording" {
            return self.stop_recording(true);
        }
        ensure!(
            c.state.phase == "ready" && !c.token.is_empty(),
            "Recording is not ready"
        );
        c.state.phase = "sending".into();
        c.send_when_ready = false;
        let id = c.id.clone();
        let r = json!({"action":"voiceReply","chatId":c.destination["chatId"],"messageId":c.destination["messageId"],"token":c.token});
        self.action(id, "voiceReply".into(), r, false);
        Ok(())
    }
    pub fn capture_event(&mut self, event: &Completion) {
        if let Some(c) = &mut self.capture {
            if c.id != event.id {
                return;
            }
            let v = &event.result;
            if event.kind == "capture-exited" {
                if c.state.phase != "discarding" {
                    c.state.phase = "error".into();
                    c.state.error = "Recording helper stopped".into();
                }
                return;
            }
            if let Some(state) = v["state"].as_str() {
                if !(c.state.phase == "stopping" && state == "recording") {
                    c.state.phase = state.into();
                }
            }
            if let Some(d) = v["duration"].as_u64() {
                c.state.duration_ms = d;
            }
            if let Some(token) = v["token"].as_str() {
                c.token = token.into();
            }
            if let Some(e) = v["error"].as_str() {
                c.state.error = e.into();
                c.send_when_ready = false;
            }
        }
    }
    pub fn stop_audio(&mut self) {
        self.audio_tick();
        if let Some(mut a) = self.audio.take() {
            let _ = a.child.kill();
            let _ = a.child.wait();
            if let Some(p) = self.playback.get_mut(&a.id) {
                p.playing = false;
            }
        }
    }
    pub fn play(&mut self, id: &str, n: &Value, toggle: bool) -> Result<()> {
        let same = self.audio.as_ref().is_some_and(|a| a.id == id);
        self.stop_audio();
        if same && toggle {
            return Ok(());
        }
        let p = self.playback.entry(id.into()).or_default();
        p.duration_ms = (n["data"]["duration"].as_f64().unwrap_or(0.) * 1000.) as u64;
        p.seekable = true;
        if p.position_ms >= p.duration_ms {
            p.position_ms = 0;
        }
        let path = local_path(n["data"]["source"].as_str().unwrap_or(""))
            .filter(|p| p.is_file())
            .context("Voice media is unavailable")?;
        let child = Command::new("ffplay")
            .args([
                "-nodisp",
                "-autoexit",
                "-loglevel",
                "error",
                "-ss",
                &(p.position_ms as f64 / 1000.).to_string(),
                "-af",
                &format!("atempo={}", p.rate),
            ])
            .arg(path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        p.playing = true;
        p.error.clear();
        self.audio = Some(Audio {
            id: id.into(),
            child,
            started: Instant::now(),
            start_ms: p.position_ms,
            rate: p.rate,
        });
        Ok(())
    }
    pub fn audio_id(&self) -> Option<String> {
        self.audio.as_ref().map(|a| a.id.clone())
    }
    pub fn audio_tick(&mut self) {
        if let Some(a) = &mut self.audio {
            if let Some(p) = self.playback.get_mut(&a.id) {
                p.position_ms = (a.start_ms
                    + (a.started.elapsed().as_millis() as f64 * a.rate) as u64)
                    .min(p.duration_ms);
                if let Ok(Some(status)) = a.child.try_wait() {
                    p.playing = false;
                    if !status.success() {
                        p.error = "Voice playback failed".into();
                    }
                    self.audio = None;
                }
            }
        }
    }
}
impl Drop for Executor {
    fn drop(&mut self) {
        self.stop_audio();
        self.discard();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn synthetic_actions_do_not_connect_or_record() {
        let mut executor = Executor::new();
        executor.action(
            "demo".into(),
            "reply".into(),
            json!({"action":"reply"}),
            true,
        );
        assert_eq!(
            executor
                .rx
                .recv_timeout(std::time::Duration::from_secs(1))
                .unwrap()
                .result["ok"],
            true
        );
        assert!(executor
            .record(
                Path::new("/does-not-exist"),
                "demo".into(),
                &json!({"data":{"demo":true}})
            )
            .is_err());
        assert!(executor.capture.is_none());
    }
}
