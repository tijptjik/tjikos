//! Card inputs are snapshots, not toolkit objects or executable commands.
use serde_json::Value;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct Notice(pub Value);
impl Notice {
    pub fn id(&self) -> String {
        text(&self.0["id"])
    }
    pub fn kind(&self) -> &str {
        self.0["type"].as_str().unwrap_or("standard")
    }
    pub fn data(&self) -> &Value {
        &self.0["data"]
    }
    pub fn title(&self) -> String {
        text(&self.0["title"])
    }
    pub fn body(&self) -> String {
        text(&self.0["body"])
    }
    pub fn archived(&self) -> bool {
        self.0["archived"].as_bool().unwrap_or(false)
    }
    pub fn remaining(&self, now: f64) -> u64 {
        let start = self.0["updatedAt"]
            .as_f64()
            .filter(|n| *n != 0.)
            .or_else(|| self.0["received"].as_f64())
            .unwrap_or(now);
        ((start + number(&self.data()["seconds"]) * 1000. - now) / 1000.)
            .ceil()
            .max(0.) as u64
    }
}
pub fn text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        _ => v.to_string(),
    }
}
pub fn fallback(v: &Value, default: &str) -> String {
    let s = text(v);
    if s.is_empty() {
        default.into()
    } else {
        s
    }
}
pub fn number(v: &Value) -> f64 {
    v.as_f64().filter(|n| n.is_finite()).unwrap_or(0.)
}
pub fn stamp(seconds: u64) -> String {
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}
pub fn local_path(source: &str) -> Option<PathBuf> {
    if source.starts_with('/') {
        return Some(PathBuf::from(source));
    }
    url::Url::parse(source).ok()?.to_file_path().ok()
}

/// State controlled by the host; default to hidden and inert until lock state is known.
#[derive(Clone, Debug)]
pub struct ViewState {
    pub locked: bool,
    pub show_when_locked: bool,
    pub paused: bool,
    pub selected: bool,
    pub floating: bool,
    pub history: bool,
    pub blocked: bool,
    pub busy: bool,
    pub group_count: usize,
    pub feedback: String,
    pub playback: Playback,
    pub recording: Recording,
}
impl Default for ViewState {
    fn default() -> Self {
        Self {
            locked: true,
            show_when_locked: false,
            paused: false,
            selected: false,
            floating: false,
            history: false,
            blocked: false,
            busy: false,
            group_count: 1,
            feedback: String::new(),
            playback: Playback::default(),
            recording: Recording::default(),
        }
    }
}
impl ViewState {
    pub fn interactive(&self, notice: &Notice) -> bool {
        !self.locked
            && !self.paused
            && !self.history
            && !self.blocked
            && !self.busy
            && !notice.archived()
    }
}
#[derive(Clone, Debug)]
pub struct Playback {
    pub playing: bool,
    pub position_ms: u64,
    pub duration_ms: u64,
    pub rate: f64,
    pub seekable: bool,
    pub error: String,
}
impl Default for Playback {
    fn default() -> Self {
        Self {
            playing: false,
            position_ms: 0,
            duration_ms: 0,
            rate: 1.,
            seekable: false,
            error: String::new(),
        }
    }
}
#[derive(Clone, Debug, Default)]
pub struct Recording {
    pub phase: String,
    pub duration_ms: u64,
    pub uncertain: bool,
    pub error: String,
}

/// Intent only. Hosts authorize and execute actions, and supply completion state.
/// Widgets never execute payload command arrays, send messages, or open a microphone.
#[derive(Clone, Debug, PartialEq)]
pub enum Intent {
    Invoke(usize),
    Dismiss,
    OpenGroup,
    Hover(bool),
    Select,
    Drag {
        dx: f64,
        dy: f64,
        finished: bool,
    },
    Task {
        action: String,
        until: Option<String>,
    },
    Telegram {
        action: String,
        text: String,
        quote: bool,
        chat_id: String,
        message_id: String,
    },
    PlaybackToggle,
    Seek(u64),
    PlaybackRate(f64),
    Record,
    RecordDiscard,
    RecordSend,
    Hold(bool),
}
#[derive(Clone, Debug, PartialEq)]
pub struct CardEvent {
    pub id: String,
    pub intent: Intent,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn countdown_uses_replacement_timestamp_and_clamps_expiry() {
        let n = Notice(json!({"received":1000,"updatedAt":3000,"data":{"seconds":3}}));
        assert_eq!(n.remaining(3001.), 3);
        assert_eq!(n.remaining(6001.), 0);
    }
    #[test]
    fn privacy_and_media_fail_closed() {
        let n = Notice(json!({}));
        assert!(!ViewState::default().interactive(&n));
        assert!(local_path("https://example.com/photo.png").is_none());
        assert!(local_path("file://remote-host/photo.png").is_none());
        assert_eq!(
            local_path("file:///tmp/a%20b.png"),
            Some(PathBuf::from("/tmp/a b.png"))
        );
    }
}
