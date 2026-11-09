//! Rendering primitives with explicit clocks; unchanged data never restarts motion.
use std::{
    cell::{Cell, RefCell},
    time::Instant,
};
use tjik_gtk::gtk::{self, gdk, glib, graphene, pango, prelude::*, subclass::prelude::*};

pub fn ease(t: f64) -> f64 {
    1. - (1. - t.clamp(0., 1.)).powi(3)
}
fn smooth(t: f64) -> f64 {
    let t = t.clamp(0., 1.);
    if t < 0.5 {
        4. * t * t * t
    } else {
        1. - (-2. * t + 2.).powi(3) / 2.
    }
}
pub fn ticker_offset(elapsed: f64, overflow: f64) -> f64 {
    if overflow <= 1. {
        return 0.;
    }
    let travel = (overflow / 35.).max(0.8);
    let t = elapsed % (1.2 + travel + 1.4 + 0.3);
    if t < 1.2 {
        0.
    } else if t < 1.2 + travel {
        -overflow * (t - 1.2) / travel
    } else if t < 1.2 + travel + 1.4 {
        -overflow
    } else {
        -overflow * (1. - smooth((t - 1.2 - travel - 1.4) / 0.3))
    }
}
#[derive(Clone)]
pub struct Entry {
    pub id: String,
    pub text: String,
    pub layout: pango::Layout,
    pub width: f64,
    pub color: gdk::RGBA,
    pub active: bool,
}
pub struct Model {
    pub entries: Vec<Entry>,
    pub gap: f64,
    pub height: f64,
    pub vertical: bool,
    pub ring: bool,
    pub accent: gdk::RGBA,
    pub start: Instant,
    pub from: f64,
    pub target: f64,
    pub previous_active: Option<String>,
    pub marquee: bool,
    pub natural: f64,
    pub viewport: f64,
}
impl Default for Model {
    fn default() -> Self {
        Self {
            entries: vec![],
            gap: 0.,
            height: 24.,
            vertical: false,
            ring: false,
            accent: gdk::RGBA::WHITE,
            start: Instant::now(),
            from: 0.,
            target: 0.,
            previous_active: None,
            marquee: false,
            natural: 0.,
            viewport: 0.,
        }
    }
}
impl Model {
    fn center(&self, index: usize) -> f64 {
        self.entries
            .iter()
            .take(index)
            .map(|e| if self.vertical { self.height } else { e.width } + self.gap)
            .sum::<f64>()
            + if self.vertical {
                self.height / 2.
            } else {
                self.entries[index].width / 2.
            }
    }
    fn current(&self) -> f64 {
        self.from + (self.target - self.from) * ease(self.start.elapsed().as_secs_f64() / 0.24)
    }
    fn span(&self) -> f64 {
        self.entries
            .iter()
            .map(|e| if self.vertical { self.height } else { e.width })
            .sum::<f64>()
            + self.gap * self.entries.len().saturating_sub(1) as f64
    }
}
mod imp {
    use super::*;
    #[derive(Default)]
    pub struct Visual {
        pub model: RefCell<Model>,
        pub ticking: Cell<bool>,
        pub offset: Cell<f64>,
    }
    #[glib::object_subclass]
    impl ObjectSubclass for Visual {
        const NAME: &'static str = "BartjikMotionVisual";
        type Type = super::Visual;
        type ParentType = gtk::Widget;
    }
    impl ObjectImpl for Visual {}
    impl WidgetImpl for Visual {
        fn measure(&self, orientation: gtk::Orientation, _: i32) -> (i32, i32, i32, i32) {
            let m = self.model.borrow();
            let width = if m.ring {
                if m.vertical {
                    m.entries.iter().map(|e| e.width).fold(0., f64::max)
                } else {
                    m.span()
                }
            } else {
                m.viewport
            };
            let height = if m.ring && m.vertical {
                m.span()
            } else {
                m.height
            };
            let size = if orientation == gtk::Orientation::Horizontal {
                width
            } else {
                height
            }
            .ceil() as i32;
            (size, size, -1, -1)
        }
        fn map(&self) {
            self.parent_map();
            self.obj().animate();
        }
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let obj = self.obj();
            let m = self.model.borrow();
            let elapsed = m.start.elapsed().as_secs_f64();
            let overflow = (m.natural - m.viewport).max(0.);
            let offset = if m.marquee {
                ticker_offset(elapsed, overflow)
            } else {
                0.
            };
            self.offset.set(offset);
            snapshot.push_clip(&graphene::Rect::new(
                0.,
                0.,
                obj.width() as f32,
                obj.height() as f32,
            ));
            for (i, e) in m.entries.iter().enumerate() {
                let (lw, lh) = e.layout.pixel_size();
                let center = if m.ring { m.center(i) } else { m.viewport / 2. };
                let (cx, cy) = if m.vertical {
                    (obj.width() as f64 / 2., center)
                } else {
                    (
                        if m.ring { center } else { m.viewport / 2. },
                        obj.height() as f64 / 2.,
                    )
                };
                let mix = if e.active && m.previous_active.as_ref() == Some(&e.id) {
                    1.
                } else if e.active {
                    smooth(elapsed / 0.2)
                } else if m.previous_active.as_ref() == Some(&e.id) {
                    1. - smooth(elapsed / 0.2)
                } else {
                    0.
                };
                let alpha = if m.ring {
                    (if e.active { 1. } else { 0.65 }) * (1. - mix)
                } else {
                    1.
                };
                snapshot.save();
                snapshot.push_opacity(alpha);
                let x = if m.ring {
                    cx - lw as f64 / 2.
                } else if m.marquee && overflow > 1. {
                    offset
                } else {
                    (m.viewport - lw as f64) / 2.
                };
                if m.ring {
                    snapshot.translate(&graphene::Point::new(cx as f32, cy as f32));
                    snapshot.scale((1. - 0.65 * mix) as f32, (1. - 0.65 * mix) as f32);
                    let (ink, _) = e.layout.pixel_extents();
                    snapshot.translate(&graphene::Point::new(
                        -(ink.x() as f32 + ink.width() as f32 / 2.),
                        -(ink.y() as f32 + ink.height() as f32 / 2.),
                    ));
                } else {
                    snapshot.translate(&graphene::Point::new(
                        x as f32,
                        (cy - lh as f64 / 2.) as f32,
                    ));
                }
                snapshot.append_layout(&e.layout, &e.color);
                snapshot.pop();
                snapshot.restore();
                if m.ring && mix > 0. {
                    let cr = snapshot.append_cairo(&graphene::Rect::new(
                        0.,
                        0.,
                        obj.width() as f32,
                        obj.height() as f32,
                    ));
                    cr.set_source_rgba(1., 1., 1., mix);
                    cr.arc(cx, cy, 3. * (0.35 + 0.65 * mix), 0., std::f64::consts::TAU);
                    let _ = cr.fill();
                }
            }
            if m.ring && m.entries.iter().any(|e| e.active) {
                let center = m.current();
                let (x, y) = if m.vertical {
                    (obj.width() as f64 / 2., center)
                } else {
                    (center, obj.height() as f64 / 2.)
                };
                let cr = snapshot.append_cairo(&graphene::Rect::new(
                    0.,
                    0.,
                    obj.width() as f32,
                    obj.height() as f32,
                ));
                cr.set_source_rgba(
                    m.accent.red() as f64,
                    m.accent.green() as f64,
                    m.accent.blue() as f64,
                    m.accent.alpha() as f64,
                );
                cr.set_line_width(1.5);
                cr.arc(x, y, 10.25, 0., std::f64::consts::TAU);
                let _ = cr.stroke();
            }
            snapshot.pop();
        }
    }
}
glib::wrapper! {pub struct Visual(ObjectSubclass<imp::Visual>) @extends gtk::Widget, @implements gtk::Accessible,gtk::Buildable,gtk::ConstraintTarget;}
impl Visual {
    pub fn new() -> Self {
        glib::Object::new()
    }
    pub fn regions(&self) -> Vec<(String, tjik_desktop::Rect)> {
        let m = self.imp().model.borrow();
        let mut offset = 0.;
        m.entries
            .iter()
            .map(|e| {
                let r = if m.vertical {
                    tjik_desktop::Rect {
                        x: 0.,
                        y: offset,
                        width: self.width() as f64,
                        height: 24.,
                    }
                } else {
                    tjik_desktop::Rect {
                        x: offset,
                        y: 0.,
                        width: e.width,
                        height: 24.,
                    }
                };
                offset += if m.vertical {
                    24. + m.gap
                } else {
                    e.width + m.gap
                };
                (e.id.clone(), r)
            })
            .collect()
    }
    pub fn set_text(
        &self,
        text: &str,
        font: &str,
        size: f64,
        color: gdk::RGBA,
        max: f64,
        min: f64,
        height: f64,
        marquee: bool,
    ) {
        let layout = self.create_pango_layout(Some(text));
        let mut desc = pango::FontDescription::from_string(font);
        desc.set_absolute_size(size * pango::SCALE as f64);
        layout.set_font_description(Some(&desc));
        let natural = layout.pixel_size().0 as f64;
        let viewport = natural.min(max).max(min);
        if !marquee {
            layout.set_width((viewport * pango::SCALE as f64) as i32);
            layout.set_ellipsize(pango::EllipsizeMode::End);
        }
        {
            let m = self.imp().model.borrow();
            if m.entries.first().is_some_and(|e| {
                e.text == text
                    && e.color == color
                    && e.layout.font_description().as_ref() == Some(&desc)
            }) && m.viewport == viewport
                && m.natural == natural
                && m.marquee == marquee
                && m.height == height
            {
                return;
            }
        }
        *self.imp().model.borrow_mut() = Model {
            entries: vec![Entry {
                id: String::new(),
                text: text.into(),
                layout,
                width: viewport,
                color,
                active: false,
            }],
            height,
            marquee,
            natural,
            viewport,
            ..Model::default()
        };
        self.queue_resize();
        self.animate();
    }
    pub fn workspaces(&self, mut entries: Vec<Entry>, gap: f64, vertical: bool, accent: gdk::RGBA) {
        let mut m = self.imp().model.borrow_mut();
        if m.entries
            .iter()
            .map(|e| (&e.id, &e.text, e.active, e.width))
            .eq(entries.iter().map(|e| (&e.id, &e.text, e.active, e.width)))
        {
            return;
        }
        let previous = m.entries.iter().find(|e| e.active).map(|e| e.id.clone());
        let old = m.current();
        let first = m.entries.is_empty();
        m.entries = std::mem::take(&mut entries);
        m.gap = gap;
        m.vertical = vertical;
        m.ring = true;
        m.accent = accent;
        m.height = 24.;
        m.target = m
            .entries
            .iter()
            .position(|e| e.active)
            .map(|i| m.center(i))
            .unwrap_or(old);
        m.from = if first { m.target } else { old };
        m.previous_active = if first {
            m.entries.iter().find(|e| e.active).map(|e| e.id.clone())
        } else {
            previous
        };
        m.start = Instant::now();
        drop(m);
        self.queue_resize();
        self.animate();
    }
    fn animate(&self) {
        self.queue_draw();
        if self.imp().ticking.replace(true) {
            return;
        }
        self.add_tick_callback(|obj, _| {
            let hidden = obj
                .root()
                .and_downcast::<gtk::Window>()
                .is_some_and(|w| w.opacity() <= 0.01);
            let m = obj.imp().model.borrow();
            let running = !hidden
                && ((m.ring && m.start.elapsed().as_secs_f64() < 0.25)
                    || (m.marquee && m.natural > m.viewport + 1.));
            drop(m);
            if running {
                obj.queue_draw();
                glib::ControlFlow::Continue
            } else {
                obj.imp().ticking.set(false);
                glib::ControlFlow::Break
            }
        });
    }
    pub fn wake(&self) {
        if self.is_mapped() {
            self.animate();
        }
    }
    pub fn hit(&self, x: f64, y: f64) -> String {
        let m = self.imp().model.borrow();
        let at = if m.vertical { y } else { x };
        let mut edge = 0.;
        for e in &m.entries {
            edge += if m.vertical { m.height } else { e.width };
            if at <= edge {
                return e.id.clone();
            }
            edge += m.gap;
        }
        String::new()
    }
    pub fn status(&self) -> serde_json::Value {
        let m = self.imp().model.borrow();
        serde_json::json!({"texts":m.entries.iter().map(|e|e.text.clone()).collect::<Vec<_>>(),"ring":m.ring,"position":m.current(),"target":m.target,"offset":self.imp().offset.get(),"natural":m.natural,"viewport":m.viewport,"ticking":self.imp().ticking.get()})
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ticker_timing() {
        assert_eq!(ticker_offset(1., 70.), 0.);
        assert!((ticker_offset(2.2, 70.) + 35.).abs() < 0.001);
        assert_eq!(ticker_offset(4., 70.), -70.);
        assert!((ticker_offset(4.75, 70.) + 35.).abs() < 0.001);
        assert_eq!(ticker_offset(4.9, 70.), 0.);
        assert_eq!(ticker_offset(4., 1.), 0.);
    }
    #[test]
    fn ring_easing() {
        assert_eq!(ease(0.), 0.);
        assert_eq!(ease(1.), 1.);
        assert_eq!(ease(0.5), 0.875);
    }
}
