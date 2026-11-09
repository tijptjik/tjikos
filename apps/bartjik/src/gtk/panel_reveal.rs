//! Animated clipping shell. Child layout is always allocated at destination size.
use std::cell::{Cell, RefCell};
use tjik_gtk::gtk::{self, gdk, glib, graphene, gsk, prelude::*, subclass::prelude::*};

mod imp {
    use super::*;
    pub struct Reveal {
        pub child: RefCell<Option<gtk::Widget>>,
        pub destination: Cell<(i32, i32)>,
        pub extent: Cell<(i32, i32)>,
        pub progress: Cell<f64>,
        pub outgoing: RefCell<Option<gsk::RenderNode>>,
        pub rendered: RefCell<Option<gsk::RenderNode>>,
        pub background: Cell<gdk::RGBA>,
        pub border: Cell<gdk::RGBA>,
    }
    impl Default for Reveal {
        fn default() -> Self {
            Self {
                child: RefCell::new(None),
                destination: Cell::new((1, 1)),
                extent: Cell::new((1, 1)),
                progress: Cell::new(1.),
                outgoing: RefCell::new(None),
                rendered: RefCell::new(None),
                background: Cell::new(gdk::RGBA::BLACK),
                border: Cell::new(gdk::RGBA::BLACK),
            }
        }
    }
    #[glib::object_subclass]
    impl ObjectSubclass for Reveal {
        const NAME: &'static str = "BartjikPanelReveal";
        type Type = super::Reveal;
        type ParentType = gtk::Widget;
    }
    impl ObjectImpl for Reveal {
        fn dispose(&self) {
            if let Some(child) = self.child.borrow_mut().take() {
                child.unparent();
            }
            self.outgoing.borrow_mut().take();
            self.rendered.borrow_mut().take();
        }
    }
    impl WidgetImpl for Reveal {
        fn measure(&self, orientation: gtk::Orientation, _: i32) -> (i32, i32, i32, i32) {
            let (w, h) = self.extent.get();
            let size = if orientation == gtk::Orientation::Horizontal {
                w
            } else {
                h
            };
            (size, size, -1, -1)
        }
        fn size_allocate(&self, _: i32, _: i32, _: i32) {
            let (w, h) = self.destination.get();
            if let Some(child) = self.child.borrow().as_ref() {
                child.allocate(w, h, -1, None);
            }
        }
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let obj = self.obj();
            let rect = graphene::Rect::new(0., 0., obj.width() as f32, obj.height() as f32);
            let outline = gsk::RoundedRect::from_rect(rect, 12.);
            snapshot.push_rounded_clip(&outline);
            snapshot.append_color(&self.background.get(), &rect);
            snapshot.pop();
            let contents = gtk::Snapshot::new();
            self.content(&contents);
            let rendered = contents.to_node();
            if let Some(node) = rendered.as_ref() {
                snapshot.append_node(node);
            }
            *self.rendered.borrow_mut() = rendered;
            snapshot.append_border(&outline, &[1.; 4], &[self.border.get(); 4]);
        }
    }
    impl Reveal {
        pub fn content(&self, snapshot: &gtk::Snapshot) {
            let obj = self.obj();
            let rect = graphene::Rect::new(
                1.,
                1.,
                (obj.width() - 2).max(0) as f32,
                (obj.height() - 2).max(0) as f32,
            );
            snapshot.push_rounded_clip(&gsk::RoundedRect::from_rect(rect, 11.));
            let outgoing = self.outgoing.borrow();
            let progress = if outgoing.is_some() {
                self.progress.get()
            } else {
                1.
            };
            if let Some(node) = outgoing.as_ref().filter(|_| progress < 1.) {
                snapshot.push_opacity(1. - progress);
                snapshot.append_node(node);
                snapshot.pop();
            }
            if progress > 0. {
                if let Some(child) = self.child.borrow().as_ref() {
                    snapshot.push_opacity(progress);
                    obj.snapshot_child(child, snapshot);
                    snapshot.pop();
                }
            }
            snapshot.pop();
        }
    }
}
glib::wrapper! { pub struct Reveal(ObjectSubclass<imp::Reveal>) @extends gtk::Widget, @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget; }
impl Reveal {
    pub fn new(child: &impl IsA<gtk::Widget>) -> Self {
        let obj: Self = glib::Object::new();
        obj.set_overflow(gtk::Overflow::Hidden);
        child.add_css_class("panel-reveal-content");
        child.set_parent(&obj);
        *obj.imp().child.borrow_mut() = Some(child.clone().upcast());
        obj
    }
    pub fn colors(&self, background: gdk::RGBA, border: gdk::RGBA) {
        self.imp().background.set(background);
        self.imp().border.set(border);
        self.queue_draw();
    }
    /// Capture the current composite before replacing content. Rapid handoffs keep
    /// the on-screen blend, rather than flashing an obsolete card at full opacity.
    pub fn freeze(&self) {
        // Use the last painted composite: a rapid handoff may arrive while newly
        // replaced children still await allocation, and must not snapshot them.
        let mut node = self.imp().rendered.borrow().clone();
        if self.imp().outgoing.borrow().is_some() {
            // Flatten interrupted blends so repeated handoffs cannot accumulate an
            // ever-deeper render-node history. Preserve output-scale text sharpness.
            if let (Some(current), Some(renderer)) =
                (node.as_ref(), self.native().and_then(|n| n.renderer()))
            {
                let scale = self.scale_factor().max(1) as f32;
                let snapshot = gtk::Snapshot::new();
                snapshot.scale(scale, scale);
                snapshot.append_node(current);
                let bounds = graphene::Rect::new(0., 0., self.width() as f32, self.height() as f32);
                let pixels =
                    graphene::Rect::new(0., 0., bounds.width() * scale, bounds.height() * scale);
                let texture = renderer.render_texture(&snapshot.to_node().unwrap(), Some(&pixels));
                let frozen = gtk::Snapshot::new();
                frozen.append_texture(&texture, &bounds);
                node = frozen.to_node();
            }
        }
        self.imp()
            .progress
            .set(if node.is_some() { 0. } else { 1. });
        *self.imp().outgoing.borrow_mut() = node;
    }
    pub fn layout(&self, width: i32, height: i32) {
        let size = (width.max(1), height.max(1));
        if self.imp().destination.replace(size) != size {
            self.queue_allocate();
        }
    }
    pub fn frame(&self, width: i32, height: i32, progress: f64) {
        let size = (width.max(1), height.max(1));
        if self.imp().extent.replace(size) != size {
            self.queue_resize();
        }
        self.imp().progress.set(progress.clamp(0., 1.));
        if progress >= 1. {
            self.imp().outgoing.borrow_mut().take();
        }
        self.queue_draw();
    }
    pub fn layout_size(&self) -> (i32, i32) {
        self.imp().destination.get()
    }
    #[cfg(test)]
    pub fn has_outgoing(&self) -> bool {
        self.imp().outgoing.borrow().is_some()
    }
}
