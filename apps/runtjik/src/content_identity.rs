//! Session-only, bounded background fingerprints for history display deduplication.
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    io::{Cursor, Read},
    path::Path,
    thread::JoinHandle,
};

#[derive(Default)]
pub struct Fingerprints {
    ready: HashMap<String, Option<String>>,
    workers: HashMap<String, JoinHandle<Option<String>>>,
}

pub fn fingerprint(bytes: &[u8]) -> String {
    let mut hash = Sha256::new();
    let decoded = (|| {
        let mut reader = image::ImageReader::new(Cursor::new(bytes))
            .with_guessed_format()
            .ok()?;
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(8192);
        limits.max_image_height = Some(8192);
        limits.max_alloc = Some(128 * 1024 * 1024);
        reader.limits(limits);
        reader.decode().ok()
    })();
    if let Some(image) = decoded {
        hash.update(b"pixels");
        hash.update(image.width().to_le_bytes());
        hash.update(image.height().to_le_bytes());
        hash.update(image.to_rgba8().as_raw());
    } else {
        hash.update(b"bytes");
        hash.update(bytes);
    }
    format!("{:x}", hash.finalize())
}

impl Fingerprints {
    fn resolve(
        &mut self,
        key: String,
        read: impl FnOnce() -> Option<Vec<u8>> + Send + 'static,
    ) -> (Option<String>, bool) {
        let finished: Vec<_> = self
            .workers
            .iter()
            .filter(|(_, w)| w.is_finished())
            .map(|(k, _)| k.clone())
            .collect();
        for key in finished {
            let result = self.workers.remove(&key).unwrap().join().unwrap_or(None);
            self.ready.insert(key, result);
        }
        if let Some(result) = self.ready.get(&key) {
            return (result.clone(), false);
        }
        if self.workers.len() < 2 && !self.workers.contains_key(&key) {
            self.workers.insert(
                key,
                std::thread::spawn(move || read().map(|bytes| fingerprint(&bytes))),
            );
        }
        (None, true)
    }

    pub fn clipboard(&mut self, id: &str) -> (Option<String>, bool) {
        let id = id.to_owned();
        self.resolve(format!("clip:{id}"), move || {
            crate::util::binary(
                &["cliphist".into(), "decode".into()],
                id.into_bytes(),
                5,
                false,
            )
            .ok()
            .filter(|bytes| bytes.len() <= 64 * 1024 * 1024)
        })
    }

    pub fn file(&mut self, path: &Path) -> (Option<String>, bool) {
        use std::os::unix::fs::MetadataExt;
        let Ok(meta) = path.metadata() else {
            return (None, false);
        };
        if meta.len() > 64 * 1024 * 1024 {
            return (None, false);
        }
        let key = format!(
            "file:{}:{}:{}:{}:{}:{}",
            path.display(),
            meta.ino(),
            meta.len(),
            meta.mtime_nsec(),
            meta.ctime(),
            meta.ctime_nsec()
        );
        let path = path.to_owned();
        self.resolve(key, move || {
            let mut bytes = Vec::new();
            std::fs::File::open(path)
                .ok()?
                .take(64 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)
                .ok()?;
            (bytes.len() <= 64 * 1024 * 1024).then_some(bytes)
        })
    }
}
