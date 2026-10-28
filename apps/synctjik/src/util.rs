use anyhow::{bail, Context, Result};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    process::Command,
};
pub fn canonical(v: &Value) -> Vec<u8> {
    // Python v2 used ensure_ascii=True. Preserve its baseline and folder hashes.
    let s = serde_json::to_string(v).expect("JSON");
    let mut out = String::new();
    for c in s.chars() {
        if c.is_ascii() {
            out.push(c);
        } else {
            let mut units = [0; 2];
            for u in c.encode_utf16(&mut units) {
                out.push_str(&format!("\\u{:04x}", u));
            }
        }
    }
    out.into_bytes()
}
pub fn digest(v: &Value) -> String {
    format!("{:x}", Sha256::digest(canonical(v)))
}
pub fn hash(p: &Path) -> Result<String> {
    let mut f = read_file(p)?;
    let mut h = Sha256::new();
    let mut b = [0; 65536];
    loop {
        let n = f.read(&mut b)?;
        if n == 0 {
            break;
        }
        h.update(&b[..n]);
    }
    Ok(format!("{:x}", h.finalize()))
}
pub fn read_file(p: &Path) -> Result<File> {
    let f = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(p)?;
    if !f.metadata()?.is_file() {
        bail!("Not a regular file: {}", p.display())
    }
    Ok(f)
}
pub fn read_json(p: &Path) -> Result<Value> {
    Ok(serde_json::from_reader(read_file(p)?)?)
}
pub fn optional_json(p: &Path) -> Result<Value> {
    if p.exists() {
        read_json(p)
    } else {
        Ok(Value::Null)
    }
}
pub fn private_dir(p: &Path) -> Result<()> {
    fs::create_dir_all(p)?;
    if fs::symlink_metadata(p)?.file_type().is_symlink() {
        bail!("Refusing symlinked directory")
    };
    fs::set_permissions(p, fs::Permissions::from_mode(0o700))?;
    Ok(())
}
pub fn atomic_json(p: &Path, v: &Value) -> Result<()> {
    let parent = p.parent().context("Missing parent")?;
    private_dir(parent)?;
    let mut f = tempfile::NamedTempFile::new_in(parent)?;
    f.as_file()
        .set_permissions(fs::Permissions::from_mode(0o600))?;
    f.write_all(&canonical(v))?;
    f.write_all(b"\n")?;
    f.as_file().sync_all()?;
    f.persist(p)?;
    File::open(parent)?.sync_all()?;
    Ok(())
}
pub fn safe_name(s: &str) -> Result<()> {
    if s.is_empty()
        || s.split('/').any(|p| p.is_empty() || p == "." || p == "..")
        || s.contains('\0')
    {
        bail!("Unsafe relative path")
    };
    Ok(())
}
pub fn host(s: &str) -> Result<()> {
    if s.is_empty()
        || !s.as_bytes()[0].is_ascii_alphanumeric()
        || !s
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
    {
        bail!("Invalid device name")
    };
    Ok(())
}
pub fn snapshot(s: &str) -> Result<()> {
    let (date, id) = s.split_once('-').context("Invalid snapshot ID")?;
    if date.is_empty()
        || !date
            .bytes()
            .all(|b| b.is_ascii_digit() || b"TZ.".contains(&b))
        || id.is_empty()
        || !id
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        bail!("Invalid snapshot ID")
    };
    Ok(())
}
pub fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}
pub fn id() -> String {
    let mut b = [0; 16];
    File::open("/dev/urandom")
        .unwrap()
        .read_exact(&mut b)
        .unwrap();
    b.iter().map(|b| format!("{b:02x}")).collect()
}
pub fn expand(p: &str, home: &Path) -> PathBuf {
    if p == "~" {
        home.into()
    } else if let Some(s) = p.strip_prefix("~/") {
        home.join(s)
    } else {
        p.into()
    }
}
pub fn run(cmd: &mut Command) -> Result<()> {
    let name = cmd.get_program().to_string_lossy().into_owned();
    let output = cmd.output().with_context(|| format!("Cannot run {name}"))?;
    if !output.status.success() {
        bail!("{name} failed ({})", output.status)
    }
    Ok(())
}
pub fn strings(v: &Value) -> Result<Vec<String>> {
    v.as_array()
        .context("Expected an array")?
        .iter()
        .map(|v| Ok(v.as_str().context("Expected text")?.into()))
        .collect()
}
pub fn text<'a>(v: &'a Value, k: &str) -> Result<&'a str> {
    v[k].as_str()
        .with_context(|| format!("Missing string: {k}"))
}
pub fn mode(p: &Value) -> &str {
    p["mode"].as_str().unwrap_or("")
}
pub fn encrypted(p: &Value) -> bool {
    p["encrypted"].as_bool().unwrap_or(true)
}
