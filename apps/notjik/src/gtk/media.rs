//! Local-only media surfaces. Playback follows visibility and privacy state.
use super::{model::*, primitives::*};
use gtk::{gdk, gio, glib, prelude::*};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::{Duration, SystemTime},
};

#[derive(Clone, Debug)]
pub struct MediaStatus {
    pub ready: bool,
    pub frames: u64,
    pub error: String,
}
#[derive(Default)]
struct Status {
    ready: Cell<bool>,
    frames: Cell<u64>,
    error: RefCell<String>,
}
pub struct Media {
    pub widget: gtk::Box,
    playing: Rc<Cell<bool>>,
    video: Option<gtk::MediaFile>,
    status: Rc<Status>,
}
impl Media {
    pub fn status(&self) -> MediaStatus {
        MediaStatus {
            ready: self.status.ready.get(),
            frames: self.status.frames.get(),
            error: self.status.error.borrow().clone(),
        }
    }
    pub fn aspect(&self, ratio: f64, cap: i32) {
        let previous = Cell::new(0);
        self.widget.add_tick_callback(move |widget, _| {
            let width = widget.width();
            if width > 0 && previous.replace(width) != width {
                let height = ((width as f64 * ratio).round() as i32).min(cap).max(1);
                widget.set_height_request(height);
                if let Some(overlay) = widget
                    .first_child()
                    .and_then(|w| w.downcast::<gtk::Overlay>().ok())
                {
                    if let Some(spacer) = overlay
                        .child()
                        .and_then(|w| w.downcast::<gtk::DrawingArea>().ok())
                    {
                        spacer.set_content_height(height);
                    }
                }
            }
            glib::ControlFlow::Continue
        });
    }
    pub fn set_active(&self, active: bool) {
        self.playing.set(active);
        if let Some(v) = &self.video {
            if active && self.widget.is_mapped() {
                v.play()
            } else {
                v.pause()
            }
        }
    }
}
impl Drop for Media {
    fn drop(&mut self) {
        self.set_active(false);
    }
}
pub fn visual(source: &str, format: &str, fallback_text: &str, height: i32) -> Media {
    let out = column(0);
    out.set_height_request(height);
    out.set_overflow(gtk::Overflow::Hidden);
    let playing = Rc::new(Cell::new(false));
    let status = Rc::new(Status::default());
    let Some(path) = local_path(source).filter(|p| p.is_file()) else {
        let l = label(
            if fallback_text.is_empty() {
                "Media no longer cached"
            } else {
                fallback_text
            },
            "muted",
        );
        l.set_xalign(0.5);
        out.append(&l);
        *status.error.borrow_mut() = "Media no longer cached".into();
        return Media {
            widget: out,
            playing,
            video: None,
            status,
        };
    };
    if format == "video" {
        let stream = gtk::MediaFile::for_file(&gio::File::for_path(path));
        stream.set_muted(true);
        stream.set_loop(true);
        let picture = gtk::Picture::for_paintable(&stream);
        picture.set_can_shrink(true);
        picture.set_content_fit(if format == "photo" {
            gtk::ContentFit::Cover
        } else {
            gtk::ContentFit::Contain
        });
        picture.set_size_request(-1, height);
        out.append(&surface(&picture, height));
        let error = label("Animation unavailable", "muted");
        error.set_visible(false);
        out.append(&error);
        let info = status.clone();
        stream.connect_error_notify(move |s| {
            error.set_visible(s.error().is_some());
            if let Some(e) = s.error() {
                *info.error.borrow_mut() = e.to_string();
            }
        });
        let info = status.clone();
        stream.connect_prepared_notify(move |s| {
            info.ready.set(s.is_prepared());
        });
        let info = status.clone();
        stream.connect_timestamp_notify(move |_| info.frames.set(info.frames.get() + 1));
        let active = playing.clone();
        let mapped = stream.clone();
        out.connect_map(move |_| {
            if active.get() {
                mapped.play();
            }
        });
        let unmapped = stream.clone();
        out.connect_unmap(move |_| unmapped.pause());
        return Media {
            widget: out,
            playing,
            video: Some(stream),
            status,
        };
    }
    let picture = gtk::Picture::new();
    picture.set_can_shrink(true);
    picture.set_content_fit(if format == "photo" {
        gtk::ContentFit::Cover
    } else {
        gtk::ContentFit::Contain
    });
    picture.set_size_request(-1, height);
    out.append(&surface(&picture, height));
    if format == "gif" {
        if let Ok(animation) = gtk::gdk_pixbuf::PixbufAnimation::from_file(&path) {
            let iter = animation.iter(None);
            status.ready.set(true);
            status.frames.set(1);
            picture.set_paintable(Some(&gdk::Texture::for_pixbuf(&iter.pixbuf())));
            let weak = picture.downgrade();
            let active = playing.clone();
            let info = status.clone();
            glib::timeout_add_local(Duration::from_millis(30), move || {
                let Some(picture) = weak.upgrade() else {
                    return glib::ControlFlow::Break;
                };
                if active.get() && picture.is_mapped() && iter.advance(SystemTime::now()) {
                    picture.set_paintable(Some(&gdk::Texture::for_pixbuf(&iter.pixbuf())));
                    info.frames.set(info.frames.get() + 1);
                }
                glib::ControlFlow::Continue
            });
        } else {
            *status.error.borrow_mut() = "Animation unavailable".into();
            out.append(&label("Animation unavailable", "muted"));
        }
    } else {
        match gdk::Texture::from_file(&gio::File::for_path(path)) {
            Ok(texture) => {
                status.ready.set(true);
                status.frames.set(1);
                picture.set_paintable(Some(&texture));
            }
            Err(e) => {
                *status.error.borrow_mut() = e.to_string();
                out.append(&label(
                    if fallback_text.is_empty() {
                        "Image unavailable"
                    } else {
                        fallback_text
                    },
                    "muted",
                ));
            }
        }
    }
    Media {
        widget: out,
        playing,
        video: None,
        status,
    }
}

fn surface(picture: &gtk::Picture, height: i32) -> gtk::Overlay {
    let overlay = gtk::Overlay::new();
    let spacer = gtk::DrawingArea::new();
    spacer.set_content_height(height);
    overlay.set_child(Some(&spacer));
    overlay.add_overlay(picture);
    overlay.set_measure_overlay(picture, false);
    overlay.set_clip_overlay(picture, true);
    overlay
}
