//! Stable tray children with interruptible size/opacity transitions.
use super::icons::Icon;
use serde_json::{json, Value};
use std::{
    cell::{Cell, RefCell},
    time::Instant,
};
use tjik_gtk::gtk::{self, glib, graphene, prelude::*, subclass::prelude::*};
fn ease(t: f64) -> f64 {
    let t = t.clamp(0., 1.);
    1. - (1. - t).powi(3)
}
struct Slot {
    id: String,
    icon: Icon,
    from: f64,
    target: f64,
    start: Instant,
    record: bool,
}
impl Slot {
    fn amount(&self) -> f64 {
        self.from + (self.target - self.from) * ease(self.start.elapsed().as_secs_f64() / 0.24)
    }
    fn moving(&self) -> bool {
        self.from != self.target && self.start.elapsed().as_secs_f64() < 0.24
    }
}
#[derive(Default)]
pub struct Model {
    slots: Vec<Slot>,
    vertical: bool,
    gap: f64,
    initialized: bool,
}
impl Model {
    fn lengths(&self) -> Vec<(f64, f64)> {
        let amounts: Vec<_> = self.slots.iter().map(Slot::amount).collect();
        self.slots
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let orientation = if self.vertical {
                    gtk::Orientation::Vertical
                } else {
                    gtk::Orientation::Horizontal
                };
                let full = s.icon.root.measure(orientation, -1).1 as f64;
                let gap = if i + 1 == amounts.len() {
                    0.
                } else {
                    self.gap * amounts[i] * amounts[i + 1..].iter().sum::<f64>().min(1.)
                };
                (full * amounts[i], gap)
            })
            .collect()
    }
    fn span(&self) -> f64 {
        self.lengths().iter().map(|(w, g)| w + g).sum()
    }
}
mod imp {
    use super::*;
    #[derive(Default)]
    pub struct Tray {
        pub model: RefCell<Model>,
        pub ticking: Cell<bool>,
    }
    #[glib::object_subclass]
    impl ObjectSubclass for Tray {
        const NAME: &'static str = "BartjikAnimatedTray";
        type Type = super::Tray;
        type ParentType = gtk::Widget;
    }
    impl ObjectImpl for Tray {
        fn dispose(&self) {
            for slot in self.model.borrow_mut().slots.drain(..) {
                slot.icon.root.unparent();
            }
        }
    }
    impl WidgetImpl for Tray {
        fn measure(&self, o: gtk::Orientation, _: i32) -> (i32, i32, i32, i32) {
            let m = self.model.borrow();
            let main = (o == gtk::Orientation::Vertical) == m.vertical;
            let size = if main { m.span().ceil() as i32 } else { 24 };
            (size, size, -1, -1)
        }
        fn size_allocate(&self, w: i32, h: i32, baseline: i32) {
            let m = self.model.borrow();
            let mut pos = 0.;
            for (slot, (length, gap)) in m.slots.iter().zip(m.lengths()) {
                let cw = slot.icon.root.measure(gtk::Orientation::Horizontal, -1).1;
                let ch = slot.icon.root.measure(gtk::Orientation::Vertical, -1).1;
                let (x, y) = if m.vertical {
                    ((w - cw) as f32 / 2., pos as f32)
                } else {
                    (pos as f32, (h - ch) as f32 / 2.)
                };
                slot.icon.root.allocate(
                    cw,
                    ch,
                    baseline,
                    Some(gtk::gsk::Transform::new().translate(&graphene::Point::new(x, y))),
                );
                pos += length + gap;
            }
        }
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let obj = self.obj();
            let m = self.model.borrow();
            let mut pos = 0.;
            for (slot, (length, gap)) in m.slots.iter().zip(m.lengths()) {
                if length > 0. {
                    let rect = if m.vertical {
                        graphene::Rect::new(-4., pos as f32, obj.width() as f32 + 8., length as f32)
                    } else {
                        graphene::Rect::new(
                            pos as f32 - if slot.amount() >= 0.999 { 2. } else { 0. },
                            -4.,
                            length as f32 + if slot.amount() >= 0.999 { 4. } else { 0. },
                            obj.height() as f32 + 8.,
                        )
                    };
                    snapshot.push_clip(&rect);
                    snapshot.push_opacity(if slot.record && slot.target > 0. {
                        1.
                    } else {
                        slot.amount()
                    });
                    obj.snapshot_child(&slot.icon.root, snapshot);
                    snapshot.pop();
                    snapshot.pop();
                }
                pos += length + gap;
            }
        }
    }
}
glib::wrapper! {pub struct Tray(ObjectSubclass<imp::Tray>) @extends gtk::Widget,@implements gtk::Accessible,gtk::Buildable,gtk::ConstraintTarget;}
impl Tray {
    pub fn new(vertical: bool) -> Self {
        let obj: Self = glib::Object::new();
        obj.imp().model.borrow_mut().vertical = vertical;
        obj
    }
    pub fn update(
        &self,
        desired: Vec<(String, Value, Value, bool, bool, bool)>,
        gap: f64,
        animate: bool,
    ) {
        let mut m = self.imp().model.borrow_mut();
        m.gap = gap;
        let first = !m.initialized;
        m.initialized = true;
        let ids: Vec<_> = desired.iter().map(|v| v.0.clone()).collect();
        for slot in &mut m.slots {
            if !ids.contains(&slot.id) && slot.target != 0. {
                slot.from = if animate { slot.amount() } else { 0. };
                slot.target = 0.;
                slot.start = Instant::now();
                slot.icon.root.set_can_target(false);
            }
        }
        for (index, (id, part, style, urgent, record, pulse)) in desired.into_iter().enumerate() {
            let found = m.slots.iter().position(|v| v.id == id);
            let slot = if let Some(i) = found {
                &mut m.slots[i]
            } else {
                let icon = Icon::new();
                icon.root.set_parent(self);
                let initial = if first || !animate { 1. } else { 0. };
                let position = index.min(m.slots.len());
                m.slots.insert(
                    position,
                    Slot {
                        id: id.clone(),
                        icon,
                        from: initial,
                        target: 1.,
                        start: Instant::now(),
                        record,
                    },
                );
                let i = m.slots.iter().position(|v| v.id == id).unwrap();
                &mut m.slots[i]
            };
            if slot.target != 1. {
                slot.from = if animate { slot.amount() } else { 1. };
                slot.target = 1.;
                slot.start = Instant::now();
            }
            slot.record = record;
            slot.icon.root.set_can_target(true);
            slot.icon
                .update(&part, &style, urgent, false, record, pulse);
            if record {
                let label = if pulse {
                    "Hyprvoice processing"
                } else {
                    "Recording"
                };
                slot.icon.root.set_tooltip_text(Some(label));
                slot.icon
                    .root
                    .update_property(&[gtk::accessible::Property::Label(label)]);
            }
        }
        drop(m);
        self.queue_resize();
        self.animate();
    }
    fn animate(&self) {
        if self.imp().ticking.replace(true) {
            return;
        }
        self.add_tick_callback(|obj, _| {
            let mut m = obj.imp().model.borrow_mut();
            let mut i = 0;
            while i < m.slots.len() {
                if m.slots[i].target == 0. && !m.slots[i].moving() {
                    m.slots.remove(i).icon.root.unparent();
                } else {
                    i += 1;
                }
            }
            let moving = m.slots.iter().any(Slot::moving);
            drop(m);
            obj.queue_resize();
            obj.queue_draw();
            if moving {
                glib::ControlFlow::Continue
            } else {
                obj.imp().ticking.set(false);
                glib::ControlFlow::Break
            }
        });
    }
    pub fn widgets(&self) -> Vec<(String, gtk::Widget)> {
        self.imp()
            .model
            .borrow()
            .slots
            .iter()
            .filter(|s| s.target > 0.)
            .map(|s| (s.id.clone(), s.icon.root.clone().upcast()))
            .collect()
    }
    pub fn wake(&self) {
        for slot in &self.imp().model.borrow().slots {
            slot.icon.wake();
        }
    }
    pub fn moving(&self) -> bool {
        self.imp().model.borrow().slots.iter().any(Slot::moving)
    }
    pub fn status(&self) -> Value {
        let m = self.imp().model.borrow();
        json!({"span":m.span(),"moving":self.moving(),"slots":m.slots.iter().map(|s|json!({"id":s.id,"amount":s.amount(),"target":s.target,"state":s.icon.status()})).collect::<Vec<_>>()})
    }
    pub fn icons(&self) -> Value {
        Value::Array(
            self.imp()
                .model
                .borrow()
                .slots
                .iter()
                .map(|s| json!({"id":s.id,"state":s.icon.status()}))
                .collect(),
        )
    }
}
