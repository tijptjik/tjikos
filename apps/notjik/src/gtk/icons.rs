//! The same SVG paths as QML, rendered as alpha masks in the current theme ink.
use gtk::{cairo, gdk_pixbuf, prelude::*};
pub fn glyph(name: &str, size: i32) -> gtk::DrawingArea {
    let svg: &[u8] = match name {
        "read" => include_bytes!("../../qml/icons/actions/read.svg"),
        "record" => include_bytes!("../../qml/icons/actions/record.svg"),
        "quote" => include_bytes!("../../qml/icons/actions/quote.svg"),
        "reply" => include_bytes!("../../qml/icons/actions/reply.svg"),
        "open" => include_bytes!("../../qml/icons/actions/open.svg"),
        "reset" => include_bytes!("../../qml/icons/actions/reset.svg"),
        "send" => include_bytes!("../../qml/icons/actions/send.svg"),
        "transcribe" => include_bytes!("../../qml/icons/actions/transcribe.svg"),
        _ => include_bytes!("../../qml/icons/actions/play.svg"),
    };
    let loader = gdk_pixbuf::PixbufLoader::with_type("svg").expect("SVG loader");
    loader.set_size(size, size);
    loader.write(svg).expect("bundled SVG");
    loader.close().expect("bundled SVG");
    let pixbuf = loader.pixbuf().unwrap();
    let bytes = pixbuf.read_pixel_bytes();
    let mut mask = cairo::ImageSurface::create(cairo::Format::ARgb32, size, size).unwrap();
    let stride = mask.stride() as usize;
    {
        let mut pixels = mask.data().unwrap();
        for y in 0..size as usize {
            for x in 0..size as usize {
                let alpha = bytes[y * pixbuf.rowstride() as usize + x * 4 + 3];
                pixels[y * stride + x * 4..y * stride + x * 4 + 4]
                    .copy_from_slice(&((alpha as u32) << 24).to_ne_bytes());
            }
        }
    }
    let area = gtk::DrawingArea::new();
    area.set_content_width(size);
    area.set_content_height(size);
    area.set_draw_func(move |widget, cr, w, h| {
        let ink = widget.color();
        cr.set_source_rgba(
            ink.red() as f64,
            ink.green() as f64,
            ink.blue() as f64,
            ink.alpha() as f64,
        );
        let _ = cr.mask_surface(&mask, (w - size) as f64 / 2., (h - size) as f64 / 2.);
    });
    area
}
pub fn button(title: &str, name: &str, icon: &str) -> gtk::Button {
    let b = gtk::Button::new();
    b.set_widget_name(name);
    b.add_css_class("quiet");
    b.add_css_class("icon-action");
    b.set_child(Some(&glyph(icon, 20)));
    b.set_tooltip_text(Some(title));
    b.update_property(&[gtk::accessible::Property::Label(title)]);
    b
}
pub fn play(playing: bool) -> gtk::Button {
    let b = gtk::Button::new();
    b.set_widget_name("voice-play");
    b.add_css_class("voice-play");
    b.set_halign(gtk::Align::Start);
    b.set_valign(gtk::Align::Start);
    let title = if playing {
        "Pause voice message"
    } else {
        "Play voice message"
    };
    b.update_property(&[gtk::accessible::Property::Label(title)]);
    let area = gtk::DrawingArea::new();
    area.set_content_width(18);
    area.set_content_height(18);
    area.set_draw_func(move |_, cr, w, h| {
        cr.set_source_rgb(25. / 255., 25. / 255., 25. / 255.);
        let (cx, cy) = (w as f64 / 2., h as f64 / 2.);
        if playing {
            cr.rectangle(cx - 6., cy - 7., 4., 14.);
            cr.rectangle(cx + 2., cy - 7., 4., 14.);
        } else {
            cr.move_to(cx - 4., cy - 8.);
            cr.line_to(cx + 8., cy);
            cr.line_to(cx - 4., cy + 8.);
            cr.close_path();
        }
        let _ = cr.fill();
    });
    b.set_child(Some(&area));
    b
}
