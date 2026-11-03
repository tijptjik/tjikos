use anyhow::{ensure, Context, Result};
use serde_json::json;
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
    time::{interval, timeout, MissedTickBehavior},
};

const POLL_INTERVAL: Duration = Duration::from_millis(150);
const QUERY_TIMEOUT: Duration = Duration::from_millis(100);
const MAX_RESPONSE: u64 = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Idle,
    Recording,
    Processing,
    Unavailable,
}

impl State {
    fn name(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Recording => "recording",
            Self::Processing => "processing",
            Self::Unavailable => "unavailable",
        }
    }
}

fn default_socket() -> Result<PathBuf> {
    let cache = std::env::var_os("XDG_CACHE_HOME")
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|p| !p.is_empty())
                .map(|p| PathBuf::from(p).join(".cache"))
        })
        .context("Hyprvoice cache directory unavailable")?;
    Ok(cache.join("hyprvoice/control.sock"))
}

async fn query(path: &Path) -> Result<State> {
    timeout(QUERY_TIMEOUT, async {
        let mut socket = UnixStream::connect(path).await?;
        socket.write_all(b"s\n").await?;
        let mut response = Vec::new();
        BufReader::new(socket.take(MAX_RESPONSE + 1))
            .read_until(b'\n', &mut response)
            .await?;
        ensure!(
            response.len() as u64 <= MAX_RESPONSE,
            "Hyprvoice response too large"
        );
        let state = match response.as_slice() {
            b"STATUS status=idle\n" => State::Idle,
            // Batch Hyprvoice starts its transcriber while the recorder is still
            // capturing. The daemon switches to injecting when capture stops.
            b"STATUS status=recording\n" | b"STATUS status=transcribing\n" => State::Recording,
            b"STATUS status=injecting\n" | b"STATUS status=processing\n" => State::Processing,
            _ => anyhow::bail!("Invalid Hyprvoice status response"),
        };
        Ok(state)
    })
    .await?
}

async fn state(path: &Path) -> State {
    query(path).await.unwrap_or(State::Unavailable)
}

pub async fn watch(path: Option<&Path>) -> Result<()> {
    let path = path
        .map(Path::to_path_buf)
        .map(Ok)
        .unwrap_or_else(default_socket)?;
    let mut ticker = interval(POLL_INTERVAL);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut previous = None;
    loop {
        ticker.tick().await;
        let current = state(&path).await;
        if previous != Some(current) {
            crate::emit(&json!({
                "state": current.name(),
                "available": current != State::Unavailable,
            }));
            previous = Some(current);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::UnixListener;

    async fn reply(listener: UnixListener, responses: Vec<Vec<u8>>) {
        for response in responses {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0; 2];
            socket.read_exact(&mut request).await.unwrap();
            assert_eq!(&request, b"s\n", "status polling must remain read-only");
            socket.write_all(&response).await.unwrap();
        }
    }

    #[tokio::test]
    async fn maps_capture_and_post_capture_states() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("control.sock");
        let cases = [
            ("idle", State::Idle),
            ("recording", State::Recording),
            ("transcribing", State::Recording),
            ("injecting", State::Processing),
            ("processing", State::Processing),
            ("idle", State::Idle),
        ];
        let server = tokio::spawn(reply(
            UnixListener::bind(&path).unwrap(),
            cases
                .iter()
                .map(|(raw, _)| format!("STATUS status={raw}\n").into_bytes())
                .collect(),
        ));
        for (_, expected) in cases {
            assert_eq!(state(&path).await, expected);
        }
        server.await.unwrap();
    }

    #[tokio::test]
    async fn clears_stale_indicator_on_bad_response_and_reconnects() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("control.sock");
        assert_eq!(state(&path).await, State::Unavailable);
        let server = tokio::spawn(reply(
            UnixListener::bind(&path).unwrap(),
            vec![
                b"STATUS status=transcribing\n".to_vec(),
                b"STATUS status=unknown\n".to_vec(),
                b"STATUS status=processing".to_vec(),
                vec![b'x'; MAX_RESPONSE as usize + 1],
            ],
        ));
        assert_eq!(state(&path).await, State::Recording);
        for _ in 0..3 {
            assert_eq!(state(&path).await, State::Unavailable);
        }
        server.await.unwrap();
        assert_eq!(state(&path).await, State::Unavailable);
        std::fs::remove_file(&path).unwrap();
        let server = tokio::spawn(reply(
            UnixListener::bind(&path).unwrap(),
            vec![b"STATUS status=processing\n".to_vec()],
        ));
        assert_eq!(state(&path).await, State::Processing);
        server.await.unwrap();
    }

    #[tokio::test]
    async fn stalled_daemon_is_unavailable_within_query_budget() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("control.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let server = tokio::spawn(async move {
            let (_socket, _) = listener.accept().await.unwrap();
            std::future::pending::<()>().await;
        });
        assert_eq!(
            timeout(QUERY_TIMEOUT * 4, state(&path)).await.unwrap(),
            State::Unavailable,
        );
        server.abort();
    }
}
