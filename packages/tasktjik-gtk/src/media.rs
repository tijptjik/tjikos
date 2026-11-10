//! Explicit attachment work; byte limits apply before encoding or decoding.
use super::*;
use base64::Engine;
use std::{io::Read, path::PathBuf};
const MAX_BYTES: usize = 5 * 1024 * 1024;

pub(crate) fn encode(bytes: &[u8]) -> Result<String, String> {
    if bytes.len() > MAX_BYTES {
        return Err("Image exceeds 5 MiB".into());
    }
    Ok(base64::engine::general_purpose::STANDARD.encode(bytes))
}
pub(crate) fn read_file(path: PathBuf) -> Result<String, String> {
    let read = || -> std::io::Result<Vec<u8>> {
        if !std::fs::metadata(&path)?.is_file() {
            return Err(std::io::Error::other("Select a regular image file"));
        }
        let file = std::fs::File::open(path)?;
        if !file.metadata()?.is_file() {
            return Err(std::io::Error::other("Select a regular image file"));
        }
        let mut bytes = Vec::new();
        file.take((MAX_BYTES + 1) as u64).read_to_end(&mut bytes)?;
        Ok(bytes)
    };
    encode(&read().map_err(|e| e.to_string())?)
}
pub(crate) fn decode(source: String) -> Result<gdk::Texture, String> {
    let data = source
        .split_once(',')
        .map(|(_, data)| data)
        .ok_or("Invalid image response")?;
    if data.len() > MAX_BYTES.div_ceil(3) * 4 {
        return Err("Image exceeds 5 MiB".into());
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data)
        .map_err(|_| "Invalid image response")?;
    if bytes.len() > MAX_BYTES {
        return Err("Image exceeds 5 MiB".into());
    }
    gdk::Texture::from_bytes(&glib::Bytes::from_owned(bytes))
        .map_err(|_| "Could not decode image".into())
}
impl TaskView {
    /// One worker per view. Navigation and unmap invalidate completion without
    /// retaining the view or attempting to cancel a partially completed mutation.
    pub(crate) fn media<T: Send + 'static>(
        self: &Rc<Self>,
        work: impl FnOnce() -> Result<T, String> + Send + 'static,
        apply: impl FnOnce(&Rc<Self>, T) + 'static,
    ) {
        if self.pending.get() || self.media_busy.replace(true) {
            return;
        }
        let epoch = self.media_epoch.get();
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let result = gtk::gio::spawn_blocking(work)
                .await
                .unwrap_or_else(|_| Err("Image worker failed".into()));
            if let Some(view) = weak.upgrade() {
                view.media_busy.set(false);
                if view.media_epoch.get() != epoch || view.pending.get() {
                    return;
                }
                match result {
                    Ok(value) => apply(&view, value),
                    Err(error) => view.error(&error),
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn attachment_reads_are_bounded_and_reject_non_files() {
        let path = std::env::temp_dir().join(format!(
            "tjik-media-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::write(&path, b"synthetic").unwrap();
        assert_eq!(read_file(path.clone()).unwrap(), "c3ludGhldGlj");
        std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_len((MAX_BYTES + 1) as u64)
            .unwrap();
        assert_eq!(read_file(path.clone()).unwrap_err(), "Image exceeds 5 MiB");
        std::fs::remove_file(path).unwrap();
        assert!(read_file(std::env::temp_dir()).is_err());
        assert!(decode("not an image".into()).is_err());
        assert!(decode(format!(
            "data:image/png;base64,{}",
            "A".repeat(MAX_BYTES.div_ceil(3) * 4 + 1)
        ))
        .is_err());
    }
    fn until(mut ready: impl FnMut() -> bool) {
        let context = glib::MainContext::default();
        for _ in 0..1000 {
            while context.pending() {
                context.iteration(false);
            }
            if ready() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        panic!("media completion timed out");
    }
    #[test]
    #[ignore = "requires an isolated GTK display; scripts/test-previews.py"]
    fn media_workers_preserve_responsiveness_and_reject_stale_completion() {
        gtk::init().unwrap();
        let view = TaskView::new(|_| {}, || {}, None);
        let panel = gtk::Box::new(gtk::Orientation::Vertical, 0);
        view.stack.add_named(&panel, Some("other"));
        let applied = Rc::new(Cell::new(0));
        let count = applied.clone();
        let (release, wait) = std::sync::mpsc::channel();
        let (started, entered) = std::sync::mpsc::channel();
        view.media(
            move || {
                started.send(()).unwrap();
                wait.recv().unwrap();
                Ok(())
            },
            move |_, _| count.set(count.get() + 1),
        );
        until(|| entered.try_recv().is_ok());
        let duplicate = Rc::new(Cell::new(false));
        let flag = duplicate.clone();
        view.media(|| Ok(()), move |_, _| flag.set(true));
        // This GTK state change must be possible while the worker is still blocked.
        view.stack.set_visible_child_name("other");
        release.send(()).unwrap();
        until(|| !view.media_busy.get());
        assert_eq!(applied.get(), 0);
        assert!(!duplicate.get());
        let count = applied.clone();
        view.media(|| Ok(()), move |_, _| count.set(count.get() + 1));
        until(|| !view.media_busy.get());
        assert_eq!(applied.get(), 1);

        // Immutable GDK textures may be encoded and decoded by the worker.
        let bytes = glib::Bytes::from_owned(vec![255u8, 0, 0, 255]);
        let texture: gdk::Texture =
            gdk::MemoryTexture::new(1, 1, gdk::MemoryFormat::R8g8b8a8, &bytes, 4).upcast();
        let count = applied.clone();
        view.media(
            move || {
                let data = encode(&texture.save_to_png_bytes())?;
                decode(format!("data:image/png;base64,{data}"))
            },
            move |_, texture| {
                assert_eq!((texture.width(), texture.height()), (1, 1));
                count.set(count.get() + 1);
            },
        );
        until(|| !view.media_busy.get());
        assert_eq!(applied.get(), 2);
    }
}
