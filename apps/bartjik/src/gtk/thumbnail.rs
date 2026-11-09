//! Exact-footprint window previews. Downsample on the capture worker, not the GTK thread.
use std::cell::RefCell;
use tjik_gtk::gtk::{self, gdk, glib, prelude::*, subclass::prelude::*};
mod imp {
    use super::*;
    #[derive(Default)]
    pub struct Thumbnail {
        pub texture: RefCell<Option<gdk::Texture>>,
    }
    #[glib::object_subclass]
    impl ObjectSubclass for Thumbnail {
        const NAME: &'static str = "BartjikWindowThumbnail";
        type Type = super::Thumbnail;
        type Interfaces = (gdk::Paintable,);
    }
    impl ObjectImpl for Thumbnail {}
    impl PaintableImpl for Thumbnail {
        fn flags(&self) -> gdk::PaintableFlags {
            gdk::PaintableFlags::SIZE | gdk::PaintableFlags::CONTENTS
        }
        fn intrinsic_width(&self) -> i32 {
            self.texture.borrow().as_ref().map_or(0, |t| t.width())
        }
        fn intrinsic_height(&self) -> i32 {
            self.texture.borrow().as_ref().map_or(0, |t| t.height())
        }
        fn snapshot(&self, snapshot: &gdk::Snapshot, width: f64, height: f64) {
            if let (Some(snapshot), Some(texture)) = (
                snapshot.downcast_ref::<gtk::Snapshot>(),
                self.texture.borrow().as_ref(),
            ) {
                snapshot.append_scaled_texture(
                    texture,
                    gtk::gsk::ScalingFilter::Linear,
                    &gtk::graphene::Rect::new(0., 0., width as f32, height as f32),
                );
            }
        }
    }
}
glib::wrapper! { pub struct Thumbnail(ObjectSubclass<imp::Thumbnail>) @implements gdk::Paintable; }
impl Thumbnail {
    pub fn new(texture: &impl IsA<gdk::Texture>) -> Self {
        let value: Self = glib::Object::new();
        *value.imp().texture.borrow_mut() = Some(texture.as_ref().clone());
        value
    }
}

/// Integrate each destination pixel's exact source footprint. Unlike mip-level
/// blending this never samples a coarser level than the requested display size.
/// Nonnegative weights preserve fine strokes without ringing or sharpen halos.
/// Called on the capture worker; GTK only uploads the display-sized result.
pub fn resample(
    frame: tjik_desktop::CapturedFrame,
    width: u32,
    height: u32,
) -> tjik_desktop::CapturedFrame {
    let width = width.max(1).min(frame.width);
    let height = height.max(1).min(frame.height);
    if width == frame.width && height == frame.height {
        return frame;
    }
    fn weights(source: u32, target: u32) -> Vec<Vec<(usize, f32)>> {
        let scale = source as f64 / target as f64;
        (0..target)
            .map(|i| {
                let start = i as f64 * scale;
                let end = (i + 1) as f64 * scale;
                (start.floor() as u32..(end.ceil() as u32).min(source))
                    .map(|j| {
                        (
                            j as usize,
                            ((end.min((j + 1) as f64) - start.max(j as f64)) / scale) as f32,
                        )
                    })
                    .collect()
            })
            .collect()
    }
    let xs = weights(frame.width, width);
    let ys = weights(frame.height, height);
    // Premultiply before filtering so transparent edges cannot introduce fringes.
    let mut horizontal = vec![[0f32; 4]; width as usize * frame.height as usize];
    for y in 0..frame.height as usize {
        for (x, taps) in xs.iter().enumerate() {
            let out = &mut horizontal[y * width as usize + x];
            for &(sx, weight) in taps {
                let p = &frame.rgba[y * frame.stride + sx * 4..][..4];
                let alpha = p[3] as f32 / 255.;
                for c in 0..3 {
                    out[c] += p[c] as f32 * alpha * weight;
                }
                out[3] += p[3] as f32 * weight;
            }
        }
    }
    let mut rgba = vec![0; width as usize * height as usize * 4];
    for (y, taps) in ys.iter().enumerate() {
        for x in 0..width as usize {
            let mut pixel = [0f32; 4];
            for &(sy, weight) in taps {
                let source = horizontal[sy * width as usize + x];
                for c in 0..4 {
                    pixel[c] += source[c] * weight;
                }
            }
            let out = &mut rgba[(y * width as usize + x) * 4..][..4];
            if pixel[3] > 0. {
                for c in 0..3 {
                    out[c] = (pixel[c] * 255. / pixel[3]).round().clamp(0., 255.) as u8;
                }
            }
            out[3] = pixel[3].round().clamp(0., 255.) as u8;
        }
    }
    tjik_desktop::CapturedFrame {
        width,
        height,
        stride: width as usize * 4,
        rgba,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_footprints_preserve_thin_lines_without_halos() {
        let frame = tjik_desktop::CapturedFrame {
            width: 5,
            height: 1,
            stride: 20,
            rgba: [20, 20, 220, 20, 20]
                .into_iter()
                .flat_map(|v| [v, v, v, 255])
                .collect(),
        };
        let result = resample(frame, 2, 1);
        assert_eq!(result.rgba, vec![60, 60, 60, 255, 60, 60, 60, 255]);
    }
    #[test]
    fn transparent_colors_do_not_bleed_and_padding_is_ignored() {
        let frame = tjik_desktop::CapturedFrame {
            width: 1,
            height: 2,
            stride: 8,
            rgba: vec![
                255, 0, 0, 0, 99, 99, 99, 99, 40, 80, 120, 255, 99, 99, 99, 99,
            ],
        };
        assert_eq!(resample(frame, 1, 1).rgba, vec![40, 80, 120, 128]);
    }
    #[test]
    fn constant_fields_stay_constant_at_fractional_ratios() {
        let frame = tjik_desktop::CapturedFrame {
            width: 17,
            height: 11,
            stride: 68,
            rgba: [31, 29, 46, 255].repeat(17 * 11),
        };
        assert_eq!(resample(frame, 7, 4).rgba, [31, 29, 46, 255].repeat(7 * 4));
    }
}

/// Both rows use the same synthetic native-resolution source and renderer.
/// Top: former trilinear path. Bottom: exact footprint plus final linear sampling.
#[cfg(test)]
pub fn comparison(renderer: &gtk::gsk::Renderer) {
    let mut source =
        gtk::cairo::ImageSurface::create(gtk::cairo::Format::ARgb32, 1920, 900).unwrap();
    {
        let cr = gtk::cairo::Context::new(&source).unwrap();
        cr.set_source_rgb(0.075, 0.067, 0.11);
        cr.paint().unwrap();
        cr.select_font_face(
            "monospace",
            gtk::cairo::FontSlant::Normal,
            gtk::cairo::FontWeight::Normal,
        );
        for row in 0..36 {
            cr.set_font_size(if row % 3 == 0 { 20. } else { 16. });
            cr.set_source_rgb(0.72, 0.68, 0.83);
            cr.move_to(24., 30. + row as f64 * 24.);
            cr.show_text(
                "let preview = desktop.capture(&window)?; // Fine text: 0123456789 abcdefg",
            )
            .unwrap();
            cr.set_source_rgb(0.35, 0.75, 0.65);
            cr.set_line_width(1.);
            cr.move_to(1040., 24. + row as f64 * 24.);
            cr.line_to(1850., 24. + row as f64 * 24.);
            cr.stroke().unwrap();
        }
    }
    let stride = source.stride() as usize;
    let mut rgba = source.data().unwrap().to_vec();
    for p in rgba.chunks_exact_mut(4) {
        p.swap(0, 2);
    }
    let frame = tjik_desktop::CapturedFrame {
        width: 1920,
        height: 900,
        stride,
        rgba,
    };
    fn texture(frame: tjik_desktop::CapturedFrame) -> gdk::MemoryTexture {
        gdk::MemoryTexture::new(
            frame.width as i32,
            frame.height as i32,
            gdk::MemoryFormat::R8g8b8a8,
            &glib::Bytes::from_owned(frame.rgba),
            frame.stride,
        )
    }
    let original = texture(frame.clone());
    for scale in [1., 1.5, 2.] {
        let w = (640. * scale) as u32;
        let h = (300. * scale) as u32;
        let started = std::time::Instant::now();
        let reduced = texture(resample(frame.clone(), w, h));
        eprintln!("thumbnail {scale}x resample: {:?}", started.elapsed());
        let snap = gtk::Snapshot::new();
        snap.append_scaled_texture(
            &original,
            gtk::gsk::ScalingFilter::Trilinear,
            &gtk::graphene::Rect::new(0., 0., w as f32, h as f32),
        );
        snap.append_scaled_texture(
            &reduced,
            gtk::gsk::ScalingFilter::Linear,
            &gtk::graphene::Rect::new(0., h as f32 + 12., w as f32, h as f32),
        );
        renderer
            .render_texture(&snap.to_node().unwrap(), None)
            .save_to_png(format!("/tmp/bartjik-thumbnail-{scale}x.png"))
            .unwrap();
    }
}
