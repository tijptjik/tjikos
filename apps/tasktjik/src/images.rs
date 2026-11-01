//! Bounded image attachments. Task records contain metadata; bytes live separately.
use anyhow::{ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use image::{ImageFormat, ImageReader, Limits};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::Cursor;

pub const MAX_BYTES: usize = 5 * 1024 * 1024;

pub fn decode(data: &str) -> Result<(Value, Vec<u8>)> {
    ensure!(
        data.len() <= MAX_BYTES.div_ceil(3) * 4,
        "Image exceeds 5 MiB"
    );
    let bytes = STANDARD.decode(data).context("Invalid image encoding")?;
    let metadata = inspect(&bytes)?;
    Ok((metadata, bytes))
}

pub fn inspect(bytes: &[u8]) -> Result<Value> {
    ensure!(bytes.len() <= MAX_BYTES, "Image exceeds 5 MiB");
    let format =
        image::guess_format(bytes).context("Clipboard does not contain a supported image")?;
    let mime = match format {
        ImageFormat::Png => "image/png",
        ImageFormat::Jpeg => "image/jpeg",
        ImageFormat::WebP => "image/webp",
        _ => anyhow::bail!("Use a PNG, JPEG or WebP image"),
    };
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    let mut limits = Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    let decoded = reader
        .decode()
        .context("Image is invalid or too large to render")?;
    Ok(
        json!({"id":format!("{:x}",Sha256::digest(bytes)),"mime":mime,
        "width":decoded.width(),"height":decoded.height(),"bytes":bytes.len()}),
    )
}

pub fn attached<'a>(tasks: &'a [Value], request: &Value) -> Result<&'a Value> {
    tasks
        .iter()
        .find(|t| t["id"] == request["id"])
        .and_then(|t| t["images"].as_array())
        .and_then(|images| images.iter().find(|i| i["id"] == request["image_id"]))
        .context("Image is no longer attached to this task")
}

pub fn source(metadata: &Value, bytes: &[u8]) -> Value {
    json!({"source":format!("data:{};base64,{}",metadata["mime"].as_str().unwrap_or("image/png"),STANDARD.encode(bytes))})
}

/// Capture only image MIME types; text pastes never become attachments.
pub fn clipboard() -> Result<Value> {
    use std::{
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    let types = crate::client_util::command(&["wl-paste", "--list-types"])
        .context("Cannot read the Wayland clipboard (wl-paste is required)")?;
    let mime = ["image/png", "image/jpeg", "image/webp"]
        .into_iter()
        .find(|mime| types.lines().any(|line| line == *mime))
        .context("Copy an image first, then paste it onto a selected task")?;
    let file = tempfile::NamedTempFile::new()?;
    let mut child = Command::new("wl-paste")
        .args(["--no-newline", "--type", mime])
        .stdin(Stdio::null())
        .stdout(file.reopen()?)
        .stderr(Stdio::null())
        .spawn()?;
    let start = Instant::now();
    loop {
        if file.as_file().metadata()?.len() > MAX_BYTES as u64
            || start.elapsed() > Duration::from_secs(5)
        {
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!("Clipboard image exceeds 5 MiB or took too long to read");
        }
        if let Some(status) = child.try_wait()? {
            ensure!(status.success(), "Could not read clipboard image");
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let bytes = std::fs::read(file.path())?;
    inspect(&bytes)?;
    Ok(json!({"data":STANDARD.encode(bytes)}))
}
