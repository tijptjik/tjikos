//! Bounded local image loading. File I/O and decoding run off the GTK thread.
//! Remote URLs keep their fallback; recycled rows never receive stale completions.
use gtk::{gdk, gdk_pixbuf, gio, glib, prelude::*};
use std::{cell::RefCell, collections::VecDeque, io::Read};

const MAX_SOURCE: usize = 131072;
const MAX_FILE: u64 = 4 * 1024 * 1024;
const EDGE: i32 = 256;
const MAX_PENDING: usize = 64;

type Cache = VecDeque<(String, Option<gdk::Texture>)>;
struct Job {
    widget: glib::WeakRef<gtk::Widget>,
    source: String,
}
impl Job {
    fn target(&self) -> Option<gtk::Widget> {
        self.widget
            .upgrade()
            .filter(|image| image.widget_name() == self.source)
    }
}
#[derive(Default)]
struct Loader {
    pending: VecDeque<Job>,
    running: bool,
    cache: Cache,
}
thread_local! {
    static LOADER: RefCell<Loader> = RefCell::new(Loader::default());
}
struct Pixels {
    width: i32,
    height: i32,
    stride: usize,
    alpha: bool,
    bytes: glib::Bytes,
}
fn decode(source: &str) -> Option<Pixels> {
    let bytes = if source.starts_with("data:image/") {
        if source.len() > MAX_SOURCE {
            return None;
        }
        let (meta, body) = source.split_once(',')?;
        if meta.ends_with(";base64") {
            glib::base64_decode(body)
        } else {
            glib::uri_unescape_string(body, None::<&str>)?
                .as_bytes()
                .to_vec()
        }
    } else {
        let path = if source.starts_with('/') {
            std::path::PathBuf::from(source)
        } else {
            gio::File::for_uri(source).path()?
        };
        if !std::fs::metadata(&path).ok()?.is_file() {
            return None;
        }
        let file = std::fs::File::open(path).ok()?;
        if !file.metadata().ok()?.is_file() {
            return None;
        }
        let mut bytes = Vec::new();
        file.take(MAX_FILE + 1).read_to_end(&mut bytes).ok()?;
        if bytes.len() as u64 > MAX_FILE {
            return None;
        }
        bytes
    };
    let loader = gdk_pixbuf::PixbufLoader::new();
    loader.connect_size_prepared(|loader, width, height| {
        if width > 0 && height > 0 {
            let scale = (EDGE as f64 / width.max(height) as f64).min(1.);
            loader.set_size(
                (width as f64 * scale).round().max(1.) as i32,
                (height as f64 * scale).round().max(1.) as i32,
            );
        }
    });
    let written = loader.write(&bytes);
    let closed = loader.close();
    written.ok()?;
    closed.ok()?;
    let pixbuf = loader.pixbuf()?;
    let width = pixbuf.width();
    let height = pixbuf.height();
    if width <= 0 || height <= 0 || width > EDGE || height > EDGE {
        return None;
    }
    Some(Pixels {
        width,
        height,
        stride: pixbuf.rowstride() as usize,
        alpha: pixbuf.has_alpha(),
        bytes: pixbuf.read_pixel_bytes(),
    })
}
fn texture(pixels: Pixels) -> gdk::Texture {
    gdk::MemoryTexture::new(
        pixels.width,
        pixels.height,
        if pixels.alpha {
            gdk::MemoryFormat::R8g8b8a8
        } else {
            gdk::MemoryFormat::R8g8b8
        },
        &pixels.bytes,
        pixels.stride,
    )
    .upcast()
}
async fn drain() {
    loop {
        let job = LOADER.with(|loader| {
            let mut loader = loader.borrow_mut();
            let job = loader.pending.pop_front();
            if job.is_none() {
                loader.running = false;
            }
            job
        });
        let Some(job) = job else { return };
        if job.target().is_none() {
            continue;
        }
        let cached = LOADER.with(|loader| {
            let mut loader = loader.borrow_mut();
            let at = loader
                .cache
                .iter()
                .position(|(key, _)| key == &job.source)?;
            let entry = loader.cache.remove(at).unwrap();
            let value = entry.1.clone();
            loader.cache.push_back(entry);
            Some(value)
        });
        let loaded = match cached {
            Some(value) => value,
            None => {
                let source = job.source.clone();
                let loaded = gio::spawn_blocking(move || decode(&source))
                    .await
                    .ok()
                    .flatten()
                    .map(texture);
                // File contents can change at the same path: only embedded images are immutable.
                if job.source.starts_with("data:image/") {
                    LOADER.with(|loader| {
                        let mut loader = loader.borrow_mut();
                        if loader.cache.len() >= 32 {
                            loader.cache.pop_front();
                        }
                        loader.cache.push_back((job.source.clone(), loaded.clone()));
                    });
                }
                loaded
            }
        };
        if let (Some(image), Some(texture)) = (job.target(), loaded) {
            if let Some(image) = image.downcast_ref::<gtk::Image>() {
                image.set_paintable(Some(&texture));
            } else if let Some(picture) = image.downcast_ref::<gtk::Picture>() {
                picture.set_paintable(Some(&texture));
            }
        }
    }
}
pub fn set(image: &gtk::Image, source: &str, fallback: &str) {
    image.set_widget_name(source);
    if source.starts_with('/') || source.starts_with("file://") || source.starts_with("data:image/")
    {
        image.set_icon_name(Some(fallback));
        if source.len() > MAX_SOURCE {
            return;
        }
        enqueue(image.upcast_ref(), source);
    } else {
        image.set_icon_name(Some(if source.is_empty() || source.contains("://") {
            fallback
        } else {
            source
        }));
    }
}

/// Load a local or embedded picture with the same worker, bounds and stale-target
/// protection as `set`. Unsupported sources leave the picture empty.
pub fn set_picture(picture: &gtk::Picture, source: &str) {
    picture.set_widget_name(source);
    picture.set_paintable(None::<&gdk::Paintable>);
    if source.len() <= MAX_SOURCE
        && (source.starts_with('/')
            || source.starts_with("file://")
            || source.starts_with("data:image/"))
    {
        enqueue(picture.upcast_ref(), source);
    }
}
fn enqueue(widget: &gtk::Widget, source: &str) {
    let start = LOADER.with(|loader| {
        let mut loader = loader.borrow_mut();
        loader
            .pending
            .retain(|job| job.target().is_some_and(|target| target != *widget));
        if loader.pending.len() >= MAX_PENDING {
            loader.pending.pop_front();
        }
        loader.pending.push_back(Job {
            widget: widget.downgrade(),
            source: source.into(),
        });
        let start = !loader.running;
        loader.running = true;
        start
    });
    if start {
        glib::spawn_future_local(drain());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn svg(width: u32, height: u32) -> String {
        let xml = format!("<svg xmlns='http://www.w3.org/2000/svg' width='{width}' height='{height}'><rect width='100%' height='100%' fill='red'/></svg>");
        format!(
            "data:image/svg+xml;base64,{}",
            glib::base64_encode(xml.as_bytes())
        )
    }

    #[test]
    fn decoding_on_worker_bounds_output_and_preserves_aspect() {
        let pixels = std::thread::spawn(|| decode(&svg(4096, 2048)).unwrap())
            .join()
            .unwrap();
        assert_eq!((pixels.width, pixels.height), (256, 128));
        assert!(pixels.bytes.len() <= 256 * 128 * 4);
        assert!(pixels.bytes[0] > 240);
        assert_eq!(pixels.bytes[1], 0);
        let small = decode(&svg(16, 8)).unwrap();
        assert_eq!((small.width, small.height), (16, 8));
    }

    #[test]
    fn rejects_bad_and_oversized_embedded_sources() {
        assert!(decode("data:image/png;base64,not-an-image").is_none());
        assert!(decode(&format!("data:image/png,{}", "x".repeat(MAX_SOURCE))).is_none());
    }

    #[test]
    fn file_sources_share_decoder_and_have_an_encoded_size_limit() {
        let path = std::env::temp_dir().join(format!(
            "tjik-image-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let source = svg(2048, 4096);
        let (_, body) = source.split_once(',').unwrap();
        std::fs::write(&path, glib::base64_decode(body)).unwrap();
        let pixels = decode(path.to_str().unwrap()).unwrap();
        assert_eq!((pixels.width, pixels.height), (128, 256));
        let uri = gio::File::for_path(&path).uri();
        assert_eq!(decode(&uri).unwrap().height, 256);
        std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_len(MAX_FILE + 1)
            .unwrap();
        assert!(decode(path.to_str().unwrap()).is_none());
        std::fs::remove_file(path).unwrap();
    }
}
