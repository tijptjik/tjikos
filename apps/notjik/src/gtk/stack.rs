//! Fixed-height popup scene with measured flow and independent departing faces.
//! Footer allocation, sibling positions and input regions share one GTK frame.
use gtk::{glib, graphene, gsk, prelude::*, subclass::prelude::*};
use gtk4_layer_shell::LayerShell;
use std::{
    cell::{Cell, RefCell},
    time::Instant,
};

struct Face {
    node: gsk::RenderNode,
    y: f64,
    height: f64,
    at: Instant,
}
struct Slot {
    widget: gtk::Widget,
    height: f64,
    y: f64,
    extent: f64,
    entrance: Option<Instant>,
    retirement: Option<(Instant, f64)>,
    node: Option<gsk::RenderNode>,
    lifted: Option<(f64, f64)>,
}
// Recede early, then accelerate downward along a depth/drop arc. Keep the
// face visible through the bend; only then collapse its reserved space.
fn departure(progress: f64) -> (f64, f64, f64, f64) {
    let face = (progress / 0.65).clamp(0., 1.);
    let depth = 1. - (1. - face).powi(2);
    let flow = ((progress - 0.65) / 0.35).clamp(0., 1.);
    let flow = if flow < 0.5 {
        4. * flow.powi(3)
    } else {
        1. - (-2. * flow + 2.).powi(3) / 2.
    };
    (
        1. - face.powi(3),
        1. - 0.18 * depth,
        64. * face.powi(2),
        1. - flow,
    )
}
mod imp {
    use super::*;
    pub struct Stack {
        pub(super) slots: RefCell<Vec<Slot>>,
        pub(super) faces: RefCell<Vec<Face>>,
        pub size: Cell<(i32, i32)>,
        pub gap: Cell<f64>,
        pub offset: Cell<(i32, i32)>,
        pub grabbed: Cell<bool>,
        pub demo_scroll: Cell<bool>,
        pub scroll: Cell<f64>,
        pub scroll_max: Cell<f64>,
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
                faces: RefCell::new(vec![]),
                size: Cell::new((420, 800)),
                gap: Cell::new(10.),
                offset: Cell::new((0, 0)),
                grabbed: Cell::new(false),
                demo_scroll: Cell::new(false),
                scroll: Cell::new(0.),
                scroll_max: Cell::new(0.),
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
            let wheel = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
            let weak = self.obj().downgrade();
            wheel.connect_scroll(move |_, _, dy| {
                let Some(stack) = weak.upgrade() else {
                    return glib::Propagation::Proceed;
                };
                if !stack.imp().demo_scroll.get() {
                    return glib::Propagation::Proceed;
                }
                stack.imp().scroll.set(
                    (stack.imp().scroll.get() + dy * 48.).clamp(0., stack.imp().scroll_max.get()),
                );
                stack.queue_allocate();
                stack.queue_draw();
                glib::Propagation::Stop
            });
            self.obj().add_controller(wheel);
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
            self.faces
                .borrow_mut()
                .retain(|f| f.at.elapsed().as_secs_f64() < duration);
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
                    .filter(|s| s.lifted.is_none())
                    .map(|s| (s.height + gap) * (1. - s.extent))
                    .sum();
                let target = (width as f64 * 0.45).max(120.) * 2. / duration.max(0.001);
                let target = target.min(remaining * 8. / duration.max(0.001));
                let speed = self.speed.get()
                    + (target - self.speed.get())
                        * (1. - (-dt / (duration.max(0.001) * 0.06)).exp());
                self.speed.set(speed);
                let mut distance = speed * dt;
                let mut admitted = slots
                    .iter()
                    .filter(|s| s.entrance.is_some() && s.lifted.is_none())
                    .count();
                let mut reserved: f64 = slots
                    .iter()
                    .filter(|s| s.entrance.is_some() && s.lifted.is_none())
                    .map(|s| s.height + gap)
                    .sum();
                for s in slots.iter_mut().filter(|s| s.lifted.is_none()) {
                    if !self.demo_scroll.get()
                        && s.entrance.is_none()
                        && admitted > 0
                        && reserved + s.height > height as f64
                    {
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
            for s in slots.iter().filter(|s| s.lifted.is_none()) {
                total += gap * previous.min(s.extent) + s.height * s.extent;
                if s.extent > 0. {
                    previous = s.extent;
                }
            }
            self.scroll_max.set((total - height as f64).max(0.));
            self.scroll
                .set(self.scroll.get().clamp(0., self.scroll_max.get()));
            let mut y = (height as f64 - total).max(0.) * self.alignment.get() - self.scroll.get();
            previous = 0.;
            let region = gtk::cairo::Region::create();
            for s in slots.iter_mut().filter(|s| s.lifted.is_none()) {
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
            // Lift the same widget above the reflowed stack, retaining its pointer grab.
            for s in slots.iter_mut().filter(|s| s.lifted.is_some()) {
                let (x, y) = s.lifted.unwrap();
                s.y = y;
                s.widget.allocate(
                    width,
                    s.height.ceil() as i32,
                    -1,
                    Some(
                        gsk::Transform::new().translate(&graphene::Point::new(x as f32, y as f32)),
                    ),
                );
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
                    let rows = slots.iter().map(|s| serde_json::json!({"id":s.widget.widget_name().as_str(),"y":s.y,"height":s.height,"extent":s.extent,"retiring":s.retirement.is_some(),"lifted":s.lifted.is_some(),"widget":format!("{:p}",s.widget.as_ptr()),"liftX":s.lifted.map(|p|p.0),"departure":s.retirement.map(|(at,_)| departure(at.elapsed().as_secs_f64()/duration.max(0.001)))})).collect::<Vec<_>>();
                    let _ = writeln!(
                        file,
                        "{}",
                        serde_json::json!({"groupDepartures":self.faces.borrow().iter().map(|f|serde_json::json!({"y":f.y,"height":f.height,"progress":f.at.elapsed().as_secs_f64()/duration.max(0.001)})).collect::<Vec<_>>(),"scroll":self.scroll.get(),"surface":self.obj().widget_name().as_str(),"monitor":self.obj().root().and_then(|r| r.downcast::<gtk::Window>().ok()).and_then(|w| w.monitor()).and_then(|m|m.connector()).map(|s|s.to_string()),"time":super::super::host::now(),"width":width,"height":height,"x":self.offset.get().0,"y":self.offset.get().1,"rows":rows})
                    );
                }
            }
            self.motion.set(
                !self.faces.borrow().is_empty()
                    || slots.iter().any(|s| {
                        s.retirement.is_some()
                            || (s.entrance.is_some()
                                && (s.extent < 1.
                                    || s.entrance.is_some_and(|at| {
                                        at.elapsed().as_secs_f64() < duration * 0.4
                                    })))
                    }),
            );
        }
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            // An empty render tree may leave the native surface's previous buffer
            // attached. Submit an explicit transparent node when the last card leaves.
            let clear = snapshot.append_cairo(&graphene::Rect::new(
                0.,
                0.,
                self.obj().width() as f32,
                self.obj().height() as f32,
            ));
            clear.set_operator(gtk::cairo::Operator::Source);
            clear.set_source_rgba(0., 0., 0., 0.);
            let _ = clear.paint();
            drop(clear);
            let mut slots = self.slots.borrow_mut();
            for face in self.faces.borrow().iter() {
                let (opacity, scale, drop, _) =
                    departure(face.at.elapsed().as_secs_f64() / self.duration.get().max(0.001));
                snapshot.save();
                snapshot.push_opacity(opacity);
                let cx = self.obj().width() as f32 / 2.;
                let cy = (face.y + face.height / 2.) as f32;
                snapshot.translate(&graphene::Point::new(cx, cy + drop as f32));
                snapshot.scale(scale as f32, scale as f32);
                snapshot.translate(&graphene::Point::new(-cx, -cy));
                snapshot.append_node(&face.node);
                snapshot.pop();
                snapshot.restore();
            }
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
                .filter(|s| s.retirement.is_none() && s.extent > 0. && s.lifted.is_none())
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
            for s in slots.iter().filter(|s| s.lifted.is_some()) {
                self.obj().snapshot_child(&s.widget, snapshot);
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
    pub fn set_demo_scroll(&self, enabled: bool) {
        if self.imp().demo_scroll.replace(enabled) != enabled {
            self.imp().scroll.set(0.);
            self.queue_allocate();
        }
    }
    pub fn grabbed(&self) -> bool {
        self.imp().grabbed.get()
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
    /// Remove a card from flow without reparenting during its active gesture.
    pub fn lift(&self, child: &gtk::Widget, position: Option<(i32, i32)>) {
        let offset = self.imp().offset.get();
        if let Some(slot) = self
            .imp()
            .slots
            .borrow_mut()
            .iter_mut()
            .find(|s| s.widget == *child)
        {
            slot.lifted = position.map(|(x, y)| ((x - offset.0) as f64, (y - offset.1) as f64));
        }
        self.set_overflow(if position.is_some() {
            gtk::Overflow::Visible
        } else {
            gtk::Overflow::Hidden
        });
        self.queue_allocate();
        self.queue_draw();
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
            lifted: None,
        });
        self.imp().motion.set(true);
        self.queue_allocate();
    }
    /// Keep a group's occupied slot and reveal its successor at full opacity.
    /// Only the cached outgoing face departs; it never reserves a second gap.
    pub fn replace_group_face(&self, child: &impl IsA<gtk::Widget>) {
        let mut slots = self.imp().slots.borrow_mut();
        if let Some(slot) = slots.iter_mut().find(|s| s.widget == *child.as_ref()) {
            if let Some(node) = slot.node.take() {
                if self.imp().duration.get() > 0. {
                    self.imp().faces.borrow_mut().push(Face {
                        node,
                        y: slot.y,
                        height: slot.height,
                        at: Instant::now(),
                    });
                }
            }
            slot.extent = 1.;
            slot.entrance = Some(Instant::now() - std::time::Duration::from_secs(60));
        }
        self.imp().motion.set(true);
        self.queue_allocate();
        self.queue_draw();
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
        self.imp().faces.borrow_mut().clear();
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
    fn departure_bends_from_recession_into_a_fall() {
        let early = departure(0.65 * 0.25);
        let middle = departure(0.65 * 0.5);
        let late = departure(0.65 * 0.75);
        // Depth advances most at the start; downward travel grows toward the end.
        assert!(early.1 - middle.1 > middle.1 - late.1);
        assert!(middle.2 - early.2 < late.2 - middle.2);
        assert!(middle.0 > 0.8);
    }
    #[test]
    fn departure_preserves_space_until_face_has_receded() {
        assert_eq!(departure(0.), (1., 1., 0., 1.));
        let mid = departure(0.325);
        assert_eq!(mid, (0.875, 0.865, 16., 1.));
        let hidden = departure(0.65);
        assert_eq!(hidden, (0., 1. - 0.18, 64., 1.));
        assert!((departure(0.825).3 - 0.5).abs() < 1e-10);
        assert_eq!(departure(1.), (0., 1. - 0.18, 64., 0.));
    }
}
