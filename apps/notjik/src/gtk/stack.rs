//! Fixed-height popup scene, matching NotificationPanel.qml's measured flow.
//! Footer allocation, sibling positions and input regions share one GTK frame.
use gtk::{glib, graphene, gsk, prelude::*, subclass::prelude::*};
use std::{
    cell::{Cell, RefCell},
    time::Instant,
};

struct Slot {
    widget: gtk::Widget,
    height: f64,
    y: f64,
    extent: f64,
    entrance: Option<Instant>,
    retirement: Option<(Instant, f64)>,
    node: Option<gsk::RenderNode>,
}
// The face recedes first; only then does its reserved space collapse.
fn departure(progress: f64) -> (f64, f64, f64, f64) {
    let face = (progress / 0.65).clamp(0., 1.).powi(2);
    let flow = ((progress - 0.65) / 0.35).clamp(0., 1.);
    let flow = if flow < 0.5 {
        4. * flow.powi(3)
    } else {
        1. - (-2. * flow + 2.).powi(3) / 2.
    };
    (1. - face, 1. - 0.1 * face, 28. * face, 1. - flow)
}
mod imp {
    use super::*;
    pub struct Stack {
        pub(super) slots: RefCell<Vec<Slot>>,
        pub size: Cell<(i32, i32)>,
        pub gap: Cell<f64>,
        pub offset: Cell<(i32, i32)>,
        pub grabbed: Cell<bool>,
        pub input_enabled: Cell<bool>,
        pub outro: Cell<i32>,
        pub alignment: Cell<f64>,
        pub duration: Cell<f64>,
        pub speed: Cell<f64>,
        pub last: Cell<Instant>,
        pub motion: Cell<bool>,
    }
    impl Default for Stack {
        fn default() -> Self {
            Self {
                slots: RefCell::new(vec![]),
                size: Cell::new((420, 800)),
                gap: Cell::new(10.),
                offset: Cell::new((0, 0)),
                grabbed: Cell::new(false),
                input_enabled: Cell::new(true),
                outro: Cell::new(0),
                alignment: Cell::new(1.),
                duration: Cell::new(0.220),
                speed: Cell::new(0.),
                last: Cell::new(Instant::now()),
                motion: Cell::new(false),
            }
        }
    }
    #[glib::object_subclass]
    impl ObjectSubclass for Stack {
        const NAME: &'static str = "NotjikPopupStack";
        type Type = super::Stack;
        type ParentType = gtk::Widget;
    }
    impl ObjectImpl for Stack {
        fn constructed(&self) {
            self.parent_constructed();
            self.obj().set_overflow(gtk::Overflow::Hidden);
            self.obj().add_tick_callback(|obj, _| {
                if obj.imp().motion.get() {
                    obj.queue_allocate();
                    obj.queue_draw();
                }
                glib::ControlFlow::Continue
            });
        }
        fn dispose(&self) {
            for slot in self.slots.borrow_mut().drain(..) {
                slot.widget.unparent();
            }
        }
    }
    impl WidgetImpl for Stack {
        fn measure(&self, orientation: gtk::Orientation, _: i32) -> (i32, i32, i32, i32) {
            let size = self.size.get();
            let n = if orientation == gtk::Orientation::Horizontal {
                size.0
            } else {
                size.1
            };
            (n, n, -1, -1)
        }
        fn size_allocate(&self, width: i32, height: i32, _: i32) {
            let height = (height - self.outro.get()).max(1);
            let now = Instant::now();
            let dt = now
                .duration_since(self.last.replace(now))
                .as_secs_f64()
                .min(0.032);
            let duration = self.duration.get();
            let gap = self.gap.get();
            let mut slots = self.slots.borrow_mut();
            slots.retain(|s| {
                if s.retirement
                    .is_some_and(|(at, _)| now.duration_since(at).as_secs_f64() >= duration)
                {
                    s.widget.unparent();
                    false
                } else {
                    true
                }
            });
            for s in slots.iter_mut() {
                if let Some((at, extent)) = s.retirement {
                    s.extent = extent
                        * departure(now.duration_since(at).as_secs_f64() / duration.max(0.001)).3;
                } else {
                    s.height = s.widget.measure(gtk::Orientation::Vertical, width).1.max(1) as f64;
                }
            }
            let departing = slots.iter().any(|s| s.retirement.is_some());
            if !departing {
                let remaining: f64 = slots
                    .iter()
                    .map(|s| (s.height + gap) * (1. - s.extent))
                    .sum();
                let target = (width as f64 * 0.45).max(120.) * 2. / duration.max(0.001);
                let target = target.min(remaining * 8. / duration.max(0.001));
                let speed = self.speed.get()
                    + (target - self.speed.get())
                        * (1. - (-dt / (duration.max(0.001) * 0.06)).exp());
                self.speed.set(speed);
                let mut distance = speed * dt;
                let mut admitted = slots.iter().filter(|s| s.entrance.is_some()).count();
                let mut reserved: f64 = slots
                    .iter()
                    .filter(|s| s.entrance.is_some())
                    .map(|s| s.height + gap)
                    .sum();
                for s in slots.iter_mut() {
                    if s.entrance.is_none() && admitted > 0 && reserved + s.height > height as f64 {
                        continue;
                    }
                    if s.extent < 1. && (distance > 0. || admitted == 0 || duration == 0.) {
                        if s.entrance.is_none() {
                            s.entrance = Some(now);
                            reserved += s.height + gap;
                            if admitted == 0 {
                                s.extent = 1.;
                            }
                            admitted += 1;
                        }
                        let cost = s.height + gap;
                        let travel = distance.min(cost * (1. - s.extent));
                        s.extent += travel / cost;
                        distance -= travel;
                        if cost * (1. - s.extent) < 0.2 || duration == 0. {
                            s.extent = 1.;
                        }
                    }
                }
            } else {
                self.speed.set(0.);
            }
            let mut total = 0.;
            let mut previous: f64 = 0.;
            for s in slots.iter() {
                total += gap * previous.min(s.extent) + s.height * s.extent;
                if s.extent > 0. {
                    previous = s.extent;
                }
            }
            let mut y = (height as f64 - total).max(0.) * self.alignment.get();
            previous = 0.;
            let region = gtk::cairo::Region::create();
            for s in slots.iter_mut() {
                y += gap * previous.min(s.extent);
                if s.retirement.is_none() {
                    s.y = y + s.height * s.extent - s.height;
                    s.widget.set_child_visible(s.extent > 0.);
                    if s.extent > 0. {
                        s.widget.allocate(
                            width,
                            s.height.ceil() as i32,
                            -1,
                            Some(
                                gsk::Transform::new()
                                    .translate(&graphene::Point::new(0., s.y as f32)),
                            ),
                        );
                        let top = s.y.max(0.).round() as i32;
                        let bottom = (s.y + s.height).min(height as f64).round() as i32;
                        if bottom > top {
                            let _ = region.union_rectangle(&gtk::cairo::RectangleInt::new(
                                self.offset.get().0,
                                top + self.offset.get().1,
                                width,
                                bottom - top,
                            ));
                        }
                    }
                }
                y += s.height * s.extent;
                if s.extent > 0. {
                    previous = s.extent;
                }
            }
            if let Some(surface) = self.obj().native().and_then(|n| n.surface()) {
                if !self.input_enabled.get() {
                    surface.set_input_region(&gtk::cairo::Region::create());
                } else if self.grabbed.get() {
                    surface.set_input_region(&gtk::cairo::Region::create_rectangle(
                        &gtk::cairo::RectangleInt::new(0, 0, surface.width(), surface.height()),
                    ));
                } else {
                    surface.set_input_region(&region);
                }
            }
            // Opt-in diagnostics for synthetic compositor acceptance tests.
            if let Some(path) = std::env::var_os("NOTJIK_GTK_LAYOUT_TRACE") {
                use std::io::Write;
                if let Ok(mut file) = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
                {
                    let rows = slots.iter().map(|s| serde_json::json!({"id":s.widget.widget_name().as_str(),"y":s.y,"height":s.height,"extent":s.extent,"retiring":s.retirement.is_some(),"departure":s.retirement.map(|(at,_)| departure(at.elapsed().as_secs_f64()/duration.max(0.001)))})).collect::<Vec<_>>();
                    let _ = writeln!(
                        file,
                        "{}",
                        serde_json::json!({"time":super::super::host::now(),"width":width,"height":height,"x":self.offset.get().0,"y":self.offset.get().1,"rows":rows})
                    );
                }
            }
            self.motion.set(slots.iter().any(|s| {
                s.retirement.is_some()
                    || (s.entrance.is_some()
                        && (s.extent < 1.
                            || s.entrance
                                .is_some_and(|at| at.elapsed().as_secs_f64() < duration * 0.4)))
            }));
        }
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let mut slots = self.slots.borrow_mut();
            // Retiring faces are behind the live cards and retain their last allocation.
            for s in slots.iter().filter(|s| s.retirement.is_some()) {
                let Some(node) = &s.node else {
                    continue;
                };
                let (at, _) = s.retirement.unwrap();
                let (opacity, scale, drop, _) =
                    departure(at.elapsed().as_secs_f64() / self.duration.get().max(0.001));
                snapshot.save();
                snapshot.push_opacity(opacity);
                let cx = self.obj().width() as f32 / 2.;
                let cy = (s.y + s.height / 2.) as f32;
                snapshot.translate(&graphene::Point::new(cx, cy + drop as f32));
                snapshot.scale(scale as f32, scale as f32);
                snapshot.translate(&graphene::Point::new(-cx, -cy));
                snapshot.append_node(node);
                snapshot.pop();
                snapshot.restore();
            }
            for s in slots
                .iter_mut()
                .filter(|s| s.retirement.is_none() && s.extent > 0.)
            {
                let contents = gtk::Snapshot::new();
                self.obj().snapshot_child(&s.widget, &contents);
                s.node = contents.to_node();
                if let Some(node) = &s.node {
                    let p = s
                        .entrance
                        .map(|at| {
                            (at.elapsed().as_secs_f64() / (self.duration.get() * 0.4).max(0.001))
                                .min(1.)
                        })
                        .unwrap_or(1.);
                    let reveal = 1. - (1. - p).powi(3);
                    snapshot.push_opacity(reveal);
                    snapshot.append_node(node);
                    snapshot.pop();
                }
            }
        }
    }
}
glib::wrapper! {
    pub struct Stack(ObjectSubclass<imp::Stack>) @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}
impl Stack {
    pub fn new() -> Self {
        glib::Object::new()
    }
    pub fn position(&self, x: i32, y: i32, input_enabled: bool) {
        self.imp().offset.set((x, y));
        self.imp().input_enabled.set(input_enabled);
        self.queue_allocate();
    }
    pub fn grab(&self, grabbed: bool) {
        self.imp().grabbed.set(grabbed);
        if grabbed {
            if let Some(surface) = self.native().and_then(|n| n.surface()) {
                surface.set_input_region(&gtk::cairo::Region::create_rectangle(
                    &gtk::cairo::RectangleInt::new(0, 0, surface.width(), surface.height()),
                ));
            }
        }
        self.queue_allocate();
    }
    pub fn configure(
        &self,
        width: i32,
        height: i32,
        gap: i32,
        alignment: f64,
        duration_ms: u64,
        outro: i32,
    ) {
        let imp = self.imp();
        imp.outro.set(outro);
        let size = (width.max(1), (height + outro).max(1));
        if imp.size.replace(size) != size {
            self.queue_resize();
        }
        imp.gap.set(gap as f64);
        imp.alignment.set(alignment);
        imp.duration.set(duration_ms as f64 / 1000.);
        self.queue_allocate();
    }
    pub fn append(&self, child: &impl IsA<gtk::Widget>) {
        let child = child.as_ref();
        child.set_parent(self);
        self.imp().slots.borrow_mut().push(Slot {
            widget: child.clone(),
            height: 1.,
            y: 0.,
            extent: 0.,
            entrance: None,
            retirement: None,
            node: None,
        });
        self.imp().motion.set(true);
        self.queue_allocate();
    }
    pub fn remove(&self, child: &impl IsA<gtk::Widget>, animate: bool) {
        let mut slots = self.imp().slots.borrow_mut();
        if let Some(i) = slots.iter().position(|s| s.widget == *child.as_ref()) {
            if animate && slots[i].node.is_some() && self.imp().duration.get() > 0. {
                let s = &mut slots[i];
                s.retirement = Some((Instant::now(), s.extent));
                s.widget.set_can_target(false);
                self.imp().motion.set(true);
            } else {
                slots.remove(i).widget.unparent();
            }
        }
        self.queue_allocate();
        self.queue_draw();
    }
    pub fn clear(&self) {
        self.imp().grabbed.set(false);
        for slot in self.imp().slots.borrow_mut().drain(..) {
            slot.widget.unparent();
        }
        self.queue_allocate();
        self.queue_draw();
    }
    pub fn has_faces(&self) -> bool {
        !self.imp().slots.borrow().is_empty()
    }
    pub fn reorder_child_after(
        &self,
        child: &impl IsA<gtk::Widget>,
        previous: Option<&gtk::Widget>,
    ) {
        let mut slots = self.imp().slots.borrow_mut();
        let Some(i) = slots.iter().position(|s| s.widget == *child.as_ref()) else {
            return;
        };
        let target = previous
            .and_then(|p| slots.iter().position(|s| s.widget == *p))
            .map(|i| i + 1)
            .unwrap_or(0);
        // Preserve retiring slots until their space has collapsed.
        if i < target {
            let slot = slots.remove(i);
            slots.insert(target - 1, slot);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn departure_preserves_space_until_face_has_receded() {
        assert_eq!(departure(0.), (1., 1., 0., 1.));
        let mid = departure(0.325);
        assert_eq!(mid, (0.75, 0.975, 7., 1.));
        let hidden = departure(0.65);
        assert_eq!(hidden, (0., 0.9, 28., 1.));
        assert!((departure(0.825).3 - 0.5).abs() < 1e-10);
        assert_eq!(departure(1.), (0., 0.9, 28., 0.));
    }
}
