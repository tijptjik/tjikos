use anyhow::{Result, bail};
use sha2::{Digest, Sha256};
use std::{io::Read, path::Path};
use whatsover_engine::media::{MAX_MEDIA_BYTES, Upload};

pub struct Image {
    pub id: String,
    pub upload: Upload,
    pub bytes: Vec<u8>,
}

pub fn read(path: &Path) -> Result<Image> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(MAX_MEDIA_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    let mime = if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        "image/jpeg"
    } else if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        "image/webp"
    } else {
        bail!("--image requires JPEG, PNG or WebP content");
    };
    let upload = Upload {
        animated: false,
        kind: "image".into(),
        mime_type: mime.into(),
        duration: None,
        file_name: None,
    };
    upload.validate(&bytes)?;
    // Content identity makes a request-ID replay reject changed image bytes.
    let hash = hex::encode(Sha256::digest(&bytes));
    let id = format!(
        "{}-{}-{}-{}-{}",
        &hash[..8],
        &hash[8..12],
        &hash[12..16],
        &hash[16..20],
        &hash[20..32]
    );
    Ok(Image { id, upload, bytes })
}
