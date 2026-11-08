//! Local and embedded favicon loading shared by virtualized result views.
//! No network requests are made. Remote URLs retain the provider fallback.
use gtk::{gdk, gio, glib, prelude::*};
use std::{cell::RefCell, collections::VecDeque};

thread_local! {
    static CACHE: RefCell<VecDeque<(String, Option<gdk::Texture>)>> = const { RefCell::new(VecDeque::new()) };
}
fn embedded(source: &str) -> Option<gdk::Texture> {
    if source.len() > 131072 {
        return None;
    }
    let (meta, body) = source.split_once(',')?;
    let bytes = if meta.ends_with(";base64") {
        glib::base64_decode(body)
    } else {
        glib::uri_unescape_string(body, None::<&str>)?
            .as_bytes()
            .to_vec()
    };
    gdk::Texture::from_bytes(&glib::Bytes::from_owned(bytes)).ok()
}
pub fn set(image: &gtk::Image, source: &str, fallback: &str) {
    image.set_widget_name(source);
    if source.starts_with('/') {
        image.set_from_file(Some(source));
    } else if source.starts_with("file://") {
        image.set_from_file(gio::File::for_uri(source).path());
    } else if source.starts_with("data:image/") {
        image.set_icon_name(Some(fallback));
        let source = source.to_owned();
        let weak = image.downgrade();
        glib::idle_add_local_once(move || {
            let Some(image) = weak.upgrade() else { return };
            if image.widget_name() != source {
                return;
            }
            let texture = CACHE.with(|cache| {
                let mut cache = cache.borrow_mut();
                if let Some(at) = cache.iter().position(|(key, _)| key == &source) {
                    let entry = cache.remove(at).unwrap();
                    let texture = entry.1.clone();
                    cache.push_back(entry);
                    return texture;
                }
                let texture = embedded(&source);
                if cache.len() >= 32 {
                    cache.pop_front();
                }
                cache.push_back((source, texture.clone()));
                texture
            });
            if let Some(texture) = texture {
                image.set_paintable(Some(&texture));
            }
        });
    } else {
        image.set_icon_name(Some(if source.is_empty() || source.contains("://") {
            fallback
        } else {
            source
        }));
    }
}
