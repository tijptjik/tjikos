//! Native Notjik request transport shared by the CLI and sync client.
//! A submitted mutation is never retried, including after an ambiguous disconnect.
use anyhow::{ensure, Context, Result};
use serde_json::Value;
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
    time::Duration,
};
pub const LIMIT: usize = 131072;
pub fn path() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("NOTJIK_SOCKET") {
        return Ok(PathBuf::from(path));
    }
    Ok(
        PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR").context("XDG_RUNTIME_DIR is required")?)
            .join("notjik.sock"),
    )
}
pub fn read_frame(stream: &mut UnixStream, limit: usize) -> Result<Vec<u8>> {
    use std::io::Read;
    let mut bytes = vec![];
    let n = BufReader::new(stream)
        .take(limit as u64 + 1)
        .read_until(b'\n', &mut bytes)?;
    ensure!(
        n > 0 && n <= limit + 1 && bytes.last() == Some(&b'\n'),
        "Invalid or oversized IPC message"
    );
    Ok(bytes)
}
pub fn request(value: &Value) -> Result<Option<Value>> {
    if std::env::var_os("XDG_RUNTIME_DIR").is_none() && std::env::var_os("NOTJIK_SOCKET").is_none()
    {
        return Ok(None);
    }
    request_at(&path()?, value)
}
pub fn request_at(path: &std::path::Path, value: &Value) -> Result<Option<Value>> {
    let mut stream = match UnixStream::connect(path) {
        Ok(s) => s,
        Err(e)
            if matches!(
                e.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
            ) =>
        {
            return Ok(None)
        }
        Err(e) => return Err(e.into()),
    };
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    let bytes = serde_json::to_vec(value)?;
    ensure!(bytes.len() <= LIMIT, "Request exceeds 128 KiB");
    stream.write_all(&bytes)?;
    stream.write_all(b"\n")?;
    let response: Value = serde_json::from_slice(&read_frame(&mut stream, 16 * 1024 * 1024)?)?;
    ensure!(
        response["ok"] == true,
        "{}",
        response["error"]
            .as_str()
            .unwrap_or("Native request failed")
    );
    Ok(Some(response))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::os::unix::net::UnixListener;

    fn exchange(request: Value, response: &'static [u8]) -> (Result<Option<Value>>, Value) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fixture.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let received =
                serde_json::from_slice(&read_frame(&mut stream, LIMIT).unwrap()).unwrap();
            stream.write_all(response).unwrap();
            received
        });
        let result = request_at(&path, &request);
        (result, server.join().unwrap())
    }

    #[test]
    fn status_and_global_removal_use_native_protocol_unchanged() {
        for request in [
            json!({"op":"status"}),
            json!({"op":"global-remove","id":"global-fixture"}),
        ] {
            let (response, received) = exchange(request.clone(), b"{\"ok\":true}\n");
            assert_eq!(received, request);
            assert_eq!(response.unwrap(), Some(json!({"ok":true})));
        }
    }

    #[test]
    fn rejected_and_ambiguous_mutations_are_errors() {
        for response in [
            b"{\"ok\":false,\"error\":\"rejected\"}\n".as_slice(),
            b"",
            b"{\"ok\":true}",
        ] {
            let (result, received) =
                exchange(json!({"op":"global-remove","id":"fixture"}), response);
            assert!(result.is_err());
            assert_eq!(received["op"], "global-remove");
        }
    }

    #[test]
    fn missing_socket_is_distinct_from_a_submitted_request() {
        let dir = tempfile::tempdir().unwrap();
        assert!(
            request_at(&dir.path().join("missing"), &json!({"op":"status"}))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn frames_require_delimiter_and_obey_payload_limit() {
        for (bytes, valid) in [
            (b"1234\n".as_slice(), true),
            (b"12345\n", false),
            (b"1234", false),
        ] {
            let (mut writer, mut reader) = UnixStream::pair().unwrap();
            writer.write_all(bytes).unwrap();
            drop(writer);
            assert_eq!(read_frame(&mut reader, 4).is_ok(), valid);
        }
    }
}
