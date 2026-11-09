//! Quickshell subprocess adapter. Native frontends can use backend::run directly.
use crate::backend::{self, Request, Transport};
use anyhow::Result;
use serde_json::Value;
use std::{
    io::{BufRead, Read, Write},
    path::Path,
    sync::mpsc,
    time::Duration,
};

struct Stdio {
    input: mpsc::Receiver<Request>,
}
impl Transport for Stdio {
    fn receive(
        &mut self,
        timeout: Duration,
    ) -> std::result::Result<Request, mpsc::RecvTimeoutError> {
        self.input.recv_timeout(timeout)
    }
    fn publish(&mut self, event: &Value) -> Result<()> {
        let mut out = std::io::stdout().lock();
        writeln!(out, "{event}")?;
        out.flush()?;
        Ok(())
    }
}

pub fn run(root: &Path) -> Result<()> {
    let (input_tx, input) = mpsc::sync_channel(100);
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        let mut r = stdin.lock();
        loop {
            let mut line = Vec::new();
            match r.by_ref().take(131073).read_until(b'\n', &mut line) {
                Ok(0) => break,
                Ok(_) => {
                    let v = if line.len() > 131072 {
                        Err("Worker request exceeds 128 KiB".into())
                    } else {
                        serde_json::from_slice::<Value>(&line).map_err(|e| e.to_string())
                    };
                    if input_tx.send(v).is_err() {
                        break;
                    }
                }
                Err(e) => {
                    let _ = input_tx.send(Err(e.to_string()));
                    break;
                }
            }
        }
    });
    backend::run(root, &mut Stdio { input })
}
