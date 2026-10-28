//! Telegram 7.3.3 global theme recovery; never reads account keys.
use crate::util::*;
use aes::{
    cipher::{generic_array::GenericArray, BlockDecrypt, BlockEncrypt, KeyInit},
    Aes256,
};
use anyhow::{ensure, Context, Result};
use md5::Md5;
use sha1::{Digest, Sha1};
use std::{collections::BTreeMap, io::Read, path::Path};
struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}
impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        ensure!(n <= self.data.len() - self.pos, "Truncated Telegram record");
        let p = self.pos;
        self.pos += n;
        Ok(&self.data[p..self.pos])
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into()?))
    }
    fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_be_bytes(self.take(8)?.try_into()?))
    }
    fn array(&mut self) -> Result<&'a [u8]> {
        let n = self.u32()?;
        if n == u32::MAX {
            Ok(&[])
        } else {
            self.take(n as usize)
        }
    }
    fn string(&mut self) -> Result<String> {
        let b = self.array()?;
        ensure!(b.len() % 2 == 0, "Invalid Telegram string");
        Ok(String::from_utf16(
            &b.chunks_exact(2)
                .map(|b| u16::from_be_bytes([b[0], b[1]]))
                .collect::<Vec<_>>(),
        )?)
    }
    fn done(&self) -> bool {
        self.pos == self.data.len()
    }
}
fn tdf(path: &Path, limit: usize) -> Result<(Vec<u8>, String, Vec<u8>)> {
    let f = read_file(path)?;
    ensure!(
        f.metadata()?.len() <= limit as u64,
        "Telegram file exceeds limit"
    );
    let mut raw = Vec::new();
    f.take(limit as u64 + 1).read_to_end(&mut raw)?;
    ensure!(
        raw.len() >= 24 && raw.len() <= limit && &raw[..4] == b"TDF$",
        "Invalid Telegram file"
    );
    let data = &raw[8..raw.len() - 16];
    let mut h = Md5::new();
    h.update(data);
    h.update((data.len() as u32).to_le_bytes());
    h.update(&raw[4..8]);
    h.update(b"TDF$");
    ensure!(
        h.finalize().as_slice() == &raw[raw.len() - 16..],
        "Telegram checksum mismatch"
    );
    Ok((data.into(), hash(path)?, raw[4..8].into()))
}
fn parameters(msg: &[u8], key: &[u8]) -> (Vec<u8>, Vec<u8>) {
    fn h(parts: &[&[u8]]) -> Vec<u8> {
        let mut s = Sha1::new();
        for p in parts {
            s.update(p)
        }
        s.finalize().to_vec()
    }
    let a = h(&[msg, &key[8..40]]);
    let b = h(&[&key[40..56], msg, &key[56..72]]);
    let c = h(&[&key[72..104], msg]);
    let d = h(&[msg, &key[104..136]]);
    (
        [&a[..8], &b[8..20], &c[4..16]].concat(),
        [&a[8..20], &b[..8], &c[16..20], &d[..8]].concat(),
    )
}
fn crypt(data: &[u8], key: &[u8], encrypt: bool) -> Result<Vec<u8>> {
    let mut plain;
    if encrypt {
        plain = (data.len() as u32 + 4).to_le_bytes().to_vec();
        plain.extend(data);
        plain.resize(plain.len().div_ceil(16) * 16, 0);
    } else {
        ensure!(
            data.len() > 16 && data.len() % 16 == 0,
            "Invalid Telegram ciphertext"
        );
        plain = data[16..].to_vec();
    }
    let msg = if encrypt {
        Sha1::digest(&plain)[..16].to_vec()
    } else {
        data[..16].to_vec()
    };
    let (k, iv) = parameters(&msg, key);
    let cipher = Aes256::new_from_slice(&k)?;
    let (mut pc, mut pp) = (iv[..16].to_vec(), iv[16..].to_vec());
    let mut out = Vec::new();
    for block in plain.chunks_exact(16) {
        let xor = if encrypt { &pc } else { &pp };
        let input: Vec<u8> = block.iter().zip(xor).map(|(a, b)| a ^ b).collect();
        let mut value = GenericArray::clone_from_slice(&input);
        if encrypt {
            cipher.encrypt_block(&mut value)
        } else {
            cipher.decrypt_block(&mut value)
        }
        let xor = if encrypt { &pp } else { &pc };
        let result: Vec<u8> = value.iter().zip(xor).map(|(a, b)| a ^ b).collect();
        if encrypt {
            pc = result.clone();
            pp = block.into();
        } else {
            pc = block.into();
            pp = result.clone();
        }
        out.extend(result);
    }
    if encrypt {
        Ok([msg, out].concat())
    } else {
        ensure!(
            Sha1::digest(&out)[..16] == msg,
            "Telegram global settings decryption failed"
        );
        let n = u32::from_le_bytes(out[..4].try_into()?) as usize;
        ensure!(
            n > 3.max(out.len().saturating_sub(16)) && n <= out.len(),
            "Invalid Telegram plaintext size"
        );
        Ok(out[4..n].into())
    }
}
fn compact(data: &[u8]) -> Result<Vec<u8>> {
    let mut r = Reader::new(data);
    ensure!(r.array()?.len() <= 5 * 1024 * 1024, "Theme exceeds limit");
    ensure!(
        r.string()? == "special://new_tag",
        "Unsupported Telegram theme"
    );
    r.string()?;
    r.string()?;
    r.take(16)?;
    r.string()?;
    r.string()?;
    r.take(12)?;
    let start = r.pos;
    r.take(8)?;
    r.array()?;
    r.array()?;
    let end = r.pos;
    r.take(4)?;
    if !r.done() {
        r.array()?;
    }
    ensure!(r.done(), "Trailing Telegram theme data");
    Ok([&data[..start], &[0; 16], &data[end..]].concat())
}
pub fn recovery(root: &Path) -> Result<(BTreeMap<String, String>, BTreeMap<String, Vec<u8>>)> {
    let (data, h, _) = tdf(&root.join("settingss"), 1024 * 1024)?;
    let mut outer = Reader::new(&data);
    let salt = outer.array()?;
    let encrypted = outer.array()?;
    ensure!(
        salt.len() == 32 && outer.done(),
        "Unsupported global settings envelope"
    );
    let mut key = [0; 256];
    pbkdf2::pbkdf2_hmac::<Sha1>(b"", salt, 4, &mut key);
    let plain = crypt(encrypted, &key, false)?;
    let mut r = Reader::new(&plain);
    let mut themes = None;
    while !r.done() {
        match r.u32()? {
            0x06 | 0x07 | 0x1d | 0x0a | 0x0c | 0x0d | 0x58 | 0x57 => {
                r.take(4)?;
            }
            0x60 | 0x5e | 0x23 => {
                r.array()?;
            }
            0x54 => {
                ensure!(themes.is_none(), "Duplicate Telegram theme selection");
                let day = r.u64()?;
                let night = r.u64()?;
                let mode = r.u32()?;
                ensure!(mode <= 1 && (mode == 0 || night != 0), "Invalid night mode");
                themes = Some((day, night));
            }
            0x4e | 0x5a | 0x55 => {
                r.take(8)?;
            }
            0x61 => {
                r.take(16)?;
            }
            _ => anyhow::bail!("Unknown Telegram settings record; review format before backup"),
        }
    }
    let (day, night) = themes.context("Missing Telegram theme selection")?;
    let mut hashes = BTreeMap::from([("settingss".into(), h)]);
    let mut contents = BTreeMap::new();
    for value in std::collections::BTreeSet::from([day, night]) {
        if value == 0 {
            continue;
        }
        let name = format!(
            "{}s",
            format!("{value:016X}").chars().rev().collect::<String>()
        );
        let (data, h, version) = tdf(&root.join(&name), 64 * 1024 * 1024)?;
        let mut r = Reader::new(&data);
        let encrypted = r.array()?;
        ensure!(r.done(), "Unsupported theme envelope");
        let encrypted = crypt(&compact(&crypt(encrypted, &key, false)?)?, &key, true)?;
        let data = [(encrypted.len() as u32).to_be_bytes().to_vec(), encrypted].concat();
        let mut md = Md5::new();
        md.update(&data);
        md.update((data.len() as u32).to_le_bytes());
        md.update(&version);
        md.update(b"TDF$");
        let raw = [b"TDF$".to_vec(), version, data, md.finalize().to_vec()].concat();
        ensure!(
            raw.len() <= 8 * 1024 * 1024,
            "Compacted theme exceeds limit"
        );
        hashes.insert(name.clone(), h);
        contents.insert(name, raw);
    }
    Ok((hashes, contents))
}
