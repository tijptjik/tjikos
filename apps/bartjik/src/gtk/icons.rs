fn dot_levels(from: [f64; 3], on: bool, elapsed: f64) -> [f64; 3] {
    std::array::from_fn(|i| {
        let delay = if on { i } else { 2 - i } as f64 * 0.09;
        let t = smooth((elapsed - delay) / 0.14);
        from[i] + ((if on { 1. } else { 0. }) - from[i]) * t
    })
}
// Persistent icon artwork, reference path morphs, and tray decorations.
use crate::util::s;
use serde_json::{json, Value};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::OnceLock,
    time::Instant,
};
use tjik_gtk::gtk::{self, gdk, glib, prelude::*};
fn smooth(t: f64) -> f64 {
    let t = t.clamp(0., 1.);
    if t < 0.5 {
        4. * t * t * t
    } else {
        1. - (-2. * t + 2.).powi(3) / 2.
    }
}
fn shapes() -> &'static Value {
    static DATA: OnceLock<Value> = OnceLock::new();
    DATA.get_or_init(|| {
        serde_json::from_str(
            include_str!("../../qml/MorphData.js")
                .split_once("var shapes=")
                .unwrap()
                .1
                .trim()
                .trim_end_matches(';'),
        )
        .expect("generated morph data")
    })
}
fn compatible(a: &str, b: &str) -> bool {
    a != b
        && [
            ["notifications", "notifications_off"],
            ["coffee", "hotel"],
            ["nightlight", "light_mode"],
            ["desktop_windows", "sports_esports"],
            ["volume_up", "headphones"],
        ]
        .iter()
        .any(|g| g.contains(&a) && g.contains(&b))
}
fn scalar(v: &Value, default: f64) -> f64 {
    v.as_f64().unwrap_or_else(|| {
        v.as_bool()
            .map(|b| if b { 1. } else { 0. })
            .unwrap_or(default)
    })
}
fn interpolate(a: &Value, b: &Value, t: f64) -> Value {
    let empty = vec![];
    let aa = a.as_array().unwrap_or(&empty);
    let bb = b.as_array().unwrap_or(&empty);
    Value::Array((0..aa.len().max(bb.len())).map(|i|{
 let x=aa.get(i).unwrap_or(&Value::Null);let y=bb.get(i).unwrap_or(&Value::Null);let r=if x.is_null(){y}else{x};let points=r["points"].as_array().unwrap();
 let center=[0,1].map(|axis|points.iter().map(|p|p[axis].as_f64().unwrap()).sum::<f64>()/points.len() as f64);
 let points:Vec<_>=points.iter().enumerate().map(|(j,_)|[0,1].map(|axis|{let p=x["points"][j][axis].as_f64().unwrap_or(center[axis]);let q=y["points"][j][axis].as_f64().unwrap_or(center[axis]);p+(q-p)*t})).collect();
 let lerp=|key:&str,default:f64|scalar(&x[key],default)*(1.-t)+scalar(&y[key],default)*t;
 json!({"points":points,"closed":if t<0.5{x["closed"].clone()}else{y["closed"].clone()},"fill":lerp("fill",0.),"stroke":lerp("stroke",0.),"opacity":scalar(&x["opacity"],if x.is_null(){0.}else{1.})*(1.-t)+scalar(&y["opacity"],if y.is_null(){0.}else{1.})*t})
 }).collect())
}
struct State {
    symbol: String,
    source: Value,
    target: Value,
    start: Instant,
    morph: bool,
    urgent: bool,
    tailnet: bool,
    dots_from: [f64; 3],
    dots_start: Instant,
    record: bool,
    pulse: bool,
    size: f64,
    slot_height: f64,
    ink: gdk::RGBA,
    accent: gdk::RGBA,
    background: gdk::RGBA,
    muted: gdk::RGBA,
}
impl State {
    fn dots(&self) -> [f64; 3] {
        dot_levels(
            self.dots_from,
            self.tailnet,
            self.dots_start.elapsed().as_secs_f64(),
        )
    }
    fn dots_moving(&self) -> bool {
        self.dots_start.elapsed().as_secs_f64() < 0.32
            && self.dots_from != [if self.tailnet { 1. } else { 0. }; 3]
    }
    fn progress(&self) -> f64 {
        smooth(self.start.elapsed().as_secs_f64() / 0.22)
    }
    fn active(&self) -> bool {
        self.morph && self.start.elapsed().as_secs_f64() < 0.22
    }
}
#[derive(Clone)]
pub struct Icon {
    pub root: gtk::Overlay,
    art: gtk::Box,
    draw: gtk::DrawingArea,
    state: Rc<RefCell<State>>,
    ticking: Rc<Cell<bool>>,
}
fn color(style: &Value, key: &str, fallback: &str) -> gdk::RGBA {
    gdk::RGBA::parse(s(&style[key])).unwrap_or_else(|_| gdk::RGBA::parse(fallback).unwrap())
}
impl Icon {
    pub fn new() -> Self {
        let root = gtk::Overlay::new();
        root.set_overflow(gtk::Overflow::Visible);
        let art = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        // Artwork can be replaced on state changes; only the stable overlay is an input target.
        art.set_can_target(false);
        art.set_halign(gtk::Align::Center);
        art.set_valign(gtk::Align::Center);
        root.set_child(Some(&art));
        let draw = gtk::DrawingArea::new();
        draw.set_can_target(false);
        draw.set_overflow(gtk::Overflow::Visible);
        root.add_overlay(&draw);
        let state = Rc::new(RefCell::new(State {
            symbol: String::new(),
            source: Value::Null,
            target: Value::Null,
            start: Instant::now(),
            morph: false,
            urgent: false,
            tailnet: false,
            dots_from: [0.; 3],
            dots_start: Instant::now(),
            record: false,
            pulse: false,
            size: 18.,
            slot_height: 20.,
            ink: gdk::RGBA::WHITE,
            accent: gdk::RGBA::WHITE,
            background: gdk::RGBA::BLACK,
            muted: gdk::RGBA::WHITE,
        }));
        let data = state.clone();
        draw.set_draw_func(move |_, cr, w, h| {
            let m = data.borrow();
            let x = (w as f64 - m.size) / 2.;
            let y = (h as f64 - m.size) / 2.;
            let rgba = |c: &gdk::RGBA, a: f64| {
                cr.set_source_rgba(
                    c.red() as f64,
                    c.green() as f64,
                    c.blue() as f64,
                    c.alpha() as f64 * a,
                )
            };
            if m.active() {
                let _ = cr.save();
                cr.translate(x, y);
                cr.scale(m.size / 24., m.size / 24.);
                cr.set_line_width(1.5);
                cr.set_line_cap(gtk::cairo::LineCap::Round);
                cr.set_line_join(gtk::cairo::LineJoin::Round);
                for path in interpolate(&m.source, &m.target, m.progress())
                    .as_array()
                    .unwrap()
                {
                    cr.new_path();
                    for (i, p) in path["points"].as_array().unwrap().iter().enumerate() {
                        if i == 0 {
                            cr.move_to(p[0].as_f64().unwrap(), p[1].as_f64().unwrap());
                        } else {
                            cr.line_to(p[0].as_f64().unwrap(), p[1].as_f64().unwrap());
                        }
                    }
                    if path["closed"] == true {
                        cr.close_path();
                    }
                    let opacity = scalar(&path["opacity"], 1.);
                    rgba(&m.ink, opacity * scalar(&path["fill"], 0.));
                    let _ = cr.fill_preserve();
                    rgba(&m.ink, opacity * scalar(&path["stroke"], 0.));
                    let _ = cr.stroke();
                }
                let _ = cr.restore();
            }
            if m.urgent {
                cr.arc(x + m.size - 1.5, y + 1.5, 3.5, 0., std::f64::consts::TAU);
                rgba(&m.accent, 1.);
                let _ = cr.fill_preserve();
                rgba(&m.background, 1.);
                cr.set_line_width(1.);
                let _ = cr.stroke();
            }
            if m.tailnet || m.dots_moving() {
                for i in 0..3 {
                    cr.arc(
                        x + 1.5 + i as f64 * (m.size - 3.) / 2.,
                        h as f64 / 2. + m.slot_height / 2. + 1.5,
                        1.5,
                        0.,
                        std::f64::consts::TAU,
                    );
                    rgba(&m.muted, m.dots()[i]);
                    let _ = cr.fill();
                }
            }
            if m.record {
                let alpha = if m.pulse {
                    0.65 + 0.35
                        * (m.start.elapsed().as_secs_f64() * std::f64::consts::TAU / 1.1).cos()
                } else {
                    1.
                };
                cr.set_source_rgba(1., 77. / 255., 94. / 255., alpha);
                cr.arc(w as f64 / 2., h as f64 / 2., 5., 0., std::f64::consts::TAU);
                let _ = cr.fill();
            }
        });
        Self {
            root,
            art,
            draw,
            state,
            ticking: Rc::new(Cell::new(false)),
        }
    }
    pub fn update(
        &self,
        part: &Value,
        style: &Value,
        urgent: bool,
        tailnet: bool,
        record: bool,
        pulse: bool,
    ) {
        let symbol = if part["kind"] == "symbol" {
            s(&part["value"])
        } else {
            ""
        };
        let size = style["iconSize"].as_f64().unwrap_or(18.);
        let mut m = self.state.borrow_mut();
        if symbol != m.symbol {
            let animate = style["animateIcons"] != false
                && style["iconSet"] != "material"
                && compatible(&m.symbol, symbol);
            m.source = if m.active() {
                interpolate(&m.source, &m.target, m.progress())
            } else {
                shapes()[&m.symbol].clone()
            };
            m.target = shapes()[symbol].clone();
            m.morph = animate;
            m.symbol = symbol.into();
            m.start = Instant::now();
        }
        if tailnet != m.tailnet {
            m.dots_from = if style["animateIcons"] == false {
                [if tailnet { 1. } else { 0. }; 3]
            } else {
                m.dots()
            };
            m.dots_start = Instant::now();
            m.tailnet = tailnet;
        }
        m.size = size;
        m.slot_height = size
            .max(style["fontSize"].as_f64().unwrap_or(14.) * 1.3)
            .max(style["minItemSize"].as_f64().unwrap_or(20.));
        // QML places the three-dot row below the slot, outside its layout bounds.
        // Give only the overlay extra drawing room; keep the flag centered.
        self.draw.set_valign(gtk::Align::Center);
        self.draw.set_size_request(
            -1,
            if tailnet || m.dots_moving() {
                (m.slot_height + 6.).ceil() as i32
            } else {
                m.slot_height.ceil() as i32
            },
        );
        m.urgent = urgent;
        m.tailnet = tailnet;
        m.record = record;
        m.pulse = pulse && style["animateIcons"] != false;
        m.ink = color(style, "color", s(&style["text"]).if_empty());
        m.accent = color(style, "accent", "#ff53a6");
        m.background = color(style, "background", "transparent");
        m.muted = color(style, "muted", "#999999");
        let active = m.active();
        drop(m);
        while let Some(c) = self.art.first_child() {
            self.art.remove(&c);
        }
        if !record {
            self.art.append(&artwork(part, style, size as i32));
        }
        self.art.set_opacity(if active { 0. } else { 1. });
        self.root.set_size_request(
            size.max(style["minItemSize"].as_f64().unwrap_or(20.)) as i32,
            24,
        );
        self.draw.queue_draw();
        self.wake();
    }
    pub fn wake(&self) {
        if self.ticking.replace(true) {
            return;
        }
        let state = self.state.clone();
        let art = self.art.clone();
        let ticking = self.ticking.clone();
        self.draw.add_tick_callback(move |w, _| {
            let m = state.borrow();
            let active = m.active();
            art.set_opacity(if active { 0. } else { 1. });
            w.queue_draw();
            let visible = w.is_mapped()
                && !w
                    .root()
                    .and_downcast::<gtk::Window>()
                    .is_some_and(|v| v.opacity() < 0.01);
            if visible && (active || m.pulse || m.dots_moving()) {
                glib::ControlFlow::Continue
            } else {
                ticking.set(false);
                glib::ControlFlow::Break
            }
        });
    }
    pub fn status(&self) -> Value {
        let m = self.state.borrow();
        json!({"symbol":m.symbol,"morphing":m.active(),"progress":m.progress(),"urgent":m.urgent,"tailnet":m.tailnet,"dots":m.dots(),"dotsMoving":m.dots_moving(),"recording":m.record,"pulsing":m.pulse,"ticking":self.ticking.get()})
    }
}
trait DefaultColor {
    fn if_empty(&self) -> &str;
}
impl DefaultColor for str {
    fn if_empty(&self) -> &str {
        if self.is_empty() {
            "#ffffff"
        } else {
            self
        }
    }
}
fn artwork(part: &Value, style: &Value, size: i32) -> gtk::Widget {
    if part["overlay"].is_array() || part["overlayName"].as_str().is_some_and(|s| !s.is_empty()) {
        let mut base = part.clone();
        base.as_object_mut().unwrap().remove("overlay");
        base.as_object_mut().unwrap().remove("overlayName");
        let container = gtk::Overlay::new();
        container.set_child(Some(&artwork(&base, style, size)));
        let overlay = if part["overlay"].is_array() {
            serde_json::json!({"kind":"pixmap", "pixmap":part["overlay"]})
        } else {
            serde_json::json!({"kind":"theme", "value":part["overlayName"]})
        };
        let image = artwork(&overlay, style, (size / 2).max(1));
        image.set_halign(gtk::Align::End);
        image.set_valign(gtk::Align::End);
        image.set_can_target(false);
        container.add_overlay(&image);
        return container.upcast();
    }
    if part["kind"] == "pixmap" {
        let pix = &part["pixmap"];
        let w = pix[0].as_i64().unwrap_or(0) as i32;
        let h = pix[1].as_i64().unwrap_or(0) as i32;
        if w > 0 && h > 0 && w <= 256 && h <= 256 {
            if let Some(bytes) = pix[2]
                .as_array()
                .filter(|v| v.len() == w as usize * h as usize * 4)
            {
                let mut rgba = Vec::with_capacity(bytes.len());
                for p in bytes.chunks_exact(4) {
                    rgba.extend([
                        p[1].as_u64().unwrap_or(0) as u8,
                        p[2].as_u64().unwrap_or(0) as u8,
                        p[3].as_u64().unwrap_or(0) as u8,
                        p[0].as_u64().unwrap_or(0) as u8,
                    ]);
                }
                let texture = gdk::MemoryTexture::new(
                    w,
                    h,
                    gdk::MemoryFormat::R8g8b8a8,
                    &glib::Bytes::from_owned(rgba),
                    w as usize * 4,
                );
                let image = gtk::Image::from_paintable(Some(&texture));
                image.set_pixel_size(size);
                return image.upcast();
            }
        }
    }
    let value = s(&part["value"]);
    let mut path = value.trim_start_matches("file://").to_owned();
    if part["kind"] == "symbol" && style["iconSet"] != "material" {
        let source = include_str!("../../qml/IconRegistry.js");
        let mapping: Value = serde_json::from_str(
            source
                .split_once("var proicons = ")
                .unwrap()
                .1
                .split_once(';')
                .unwrap()
                .0,
        )
        .unwrap();
        if let Some(name) = mapping[value].as_str() {
            path = crate::util::root()
                .join(format!("qml/icons/proicons/{name}.svg"))
                .to_string_lossy()
                .into();
        } else if value == "hotel" {
            path = crate::util::root()
                .join("qml/icons/bed.svg")
                .to_string_lossy()
                .into();
        }
    }
    if part["kind"] == "theme" {
        let v = gtk::Image::from_icon_name(value);
        v.set_pixel_size(size);
        v.upcast()
    } else if path.starts_with('/') {
        let v = gtk::Image::from_file(path);
        v.set_pixel_size(size);
        v.upcast()
    } else {
        gtk::Label::new(Some(value)).upcast()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dots_order_and_reversal() {
        let on = dot_levels([0.; 3], true, 0.1);
        assert!(on[0] > on[1] && on[1] > on[2]);
        let off = dot_levels([1.; 3], false, 0.1);
        assert!(off[2] < off[1] && off[1] < off[0]);
        assert_eq!(dot_levels(on, false, 0.), on);
        assert_eq!(dot_levels([0.; 3], true, 0.32), [1.; 3]);
        assert_eq!(dot_levels([1.; 3], false, 0.32), [0.; 3]);
    }
    #[test]
    fn reference_shapes_and_reversal() {
        for pair in [
            ["notifications", "notifications_off"],
            ["coffee", "hotel"],
            ["nightlight", "light_mode"],
            ["desktop_windows", "sports_esports"],
            ["volume_up", "headphones"],
        ] {
            assert!(compatible(pair[0], pair[1]));
            let a = &shapes()[pair[0]];
            let b = &shapes()[pair[1]];
            assert!(a.is_array() && b.is_array());
            let mid = interpolate(a, b, 0.5);
            assert_eq!(interpolate(&mid, a, 0.)[0]["points"], mid[0]["points"]);
            let end = interpolate(a, b, 1.);
            for (actual, expected) in end.as_array().unwrap().iter().zip(b.as_array().unwrap()) {
                for (p, q) in actual["points"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .zip(expected["points"].as_array().unwrap())
                {
                    for axis in 0..2 {
                        assert!(
                            (p[axis].as_f64().unwrap() - q[axis].as_f64().unwrap()).abs() < 1e-9
                        );
                    }
                }
            }
        }
        assert!(!compatible("coffee", "notifications"));
    }
}
