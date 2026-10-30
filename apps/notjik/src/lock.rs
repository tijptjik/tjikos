//! Read-only Wayland lock notification subscription. Unknown always fails closed.
use anyhow::{bail, ensure, Result};
use std::{
    io::{Read, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
    time::{Duration, Instant},
};
pub struct Observer {
    socket: Option<UnixStream>,
    pub state: &'static str,
    buffer: Vec<u8>,
    bound: bool,
    retry: Instant,
}
fn word(bytes: &[u8], offset: usize) -> Result<u32> {
    Ok(u32::from_ne_bytes(
        bytes
            .get(offset..offset + 4)
            .ok_or_else(|| anyhow::anyhow!("Truncated Wayland event"))?
            .try_into()?,
    ))
}
impl Observer {
    pub fn new() -> Self {
        Self {
            socket: None,
            state: "unknown",
            buffer: vec![],
            bound: false,
            retry: Instant::now(),
        }
    }
    fn close(&mut self) {
        self.socket = None;
        self.state = "unknown";
        self.buffer.clear();
        self.bound = false;
        self.retry = Instant::now() + Duration::from_secs(2);
    }
    fn send(&mut self, id: u32, opcode: u32, payload: &[u8]) -> Result<()> {
        let mut b = id.to_ne_bytes().to_vec();
        b.extend((((payload.len() + 8) as u32) << 16 | opcode).to_ne_bytes());
        b.extend(payload);
        self.socket
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("Disconnected"))?
            .write_all(&b)?;
        Ok(())
    }
    fn connect(&mut self) -> Result<()> {
        let display = std::env::var("WAYLAND_DISPLAY").unwrap_or("wayland-0".into());
        let path = if display.starts_with('/') {
            PathBuf::from(display)
        } else {
            PathBuf::from(std::env::var("XDG_RUNTIME_DIR")?).join(display)
        };
        let s = UnixStream::connect(path)?;
        s.set_read_timeout(Some(Duration::from_millis(1)))?;
        s.set_write_timeout(Some(Duration::from_millis(500)))?;
        self.socket = Some(s);
        self.send(1, 1, &2u32.to_ne_bytes())?;
        self.send(1, 0, &3u32.to_ne_bytes())?;
        Ok(())
    }
    fn feed(&mut self, b: &[u8]) -> Result<()> {
        self.buffer.extend(b);
        while self.buffer.len() >= 8 {
            let id = word(&self.buffer, 0)?;
            let header = word(&self.buffer, 4)?;
            let size = (header >> 16) as usize;
            let opcode = header & 65535;
            ensure!(size >= 8 && size % 4 == 0, "Invalid Wayland event size");
            if self.buffer.len() < size {
                break;
            }
            let p = self.buffer[8..size].to_vec();
            self.buffer.drain(..size);
            if id == 1 && opcode == 0 {
                bail!("Wayland protocol error")
            }
            if id == 2 && opcode == 0 {
                let name = word(&p, 0)?;
                let length = word(&p, 4)? as usize;
                let interface = p
                    .get(8..8 + length)
                    .ok_or_else(|| anyhow::anyhow!("Truncated global"))?;
                if interface == b"hyprland_lock_notifier_v1\0" && !self.bound {
                    let mut payload = name.to_ne_bytes().to_vec();
                    payload.extend((length as u32).to_ne_bytes());
                    payload.extend(interface);
                    while payload.len() % 4 != 0 {
                        payload.push(0)
                    }
                    payload.extend(1u32.to_ne_bytes());
                    payload.extend(4u32.to_ne_bytes());
                    self.send(2, 0, &payload)?;
                    self.send(4, 1, &5u32.to_ne_bytes())?;
                    self.send(1, 0, &6u32.to_ne_bytes())?;
                    self.bound = true;
                }
            } else if id == 5 && opcode <= 1 {
                self.state = if opcode == 0 { "locked" } else { "unlocked" };
            } else if id == 6 && opcode == 0 && self.state == "unknown" && self.bound {
                self.state = "unlocked";
            }
        }
        Ok(())
    }
    pub fn poll(&mut self) {
        if self.socket.is_none() && Instant::now() >= self.retry && self.connect().is_err() {
            self.close();
        }
        if let Some(s) = self.socket.as_mut() {
            let mut b = [0; 65536];
            match s.read(&mut b) {
                Ok(0) => self.close(),
                Ok(n) => {
                    if self.feed(&b[..n]).is_err() {
                        self.close()
                    }
                }
                Err(e)
                    if [std::io::ErrorKind::WouldBlock, std::io::ErrorKind::TimedOut]
                        .contains(&e.kind()) => {}
                Err(_) => self.close(),
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn malformed_fails() {
        let mut o = Observer::new();
        assert!(o.feed(&[1, 0, 0, 0, 0, 0, 4, 0]).is_err());
        assert_eq!(o.state, "unknown");
    }
    #[test]
    fn unsolicited_sync_is_not_unlock() {
        let mut o = Observer::new();
        let mut b = 6u32.to_ne_bytes().to_vec();
        b.extend((8u32 << 16).to_ne_bytes());
        o.feed(&b).unwrap();
        assert_eq!(o.state, "unknown");
    }
}
