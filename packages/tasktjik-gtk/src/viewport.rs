//! Report allocated width only when it changes, without a permanent frame callback.
use tjik_gtk::gtk::{self, glib, prelude::*, subclass::prelude::*};
mod imp {
    use super::*;
    #[derive(Default)]
    pub struct Viewport {
        pub width: std::cell::Cell<i32>,
        pub queued: std::cell::Cell<bool>,
        pub changed: std::cell::RefCell<Option<Box<dyn Fn(i32)>>>,
    }
    #[glib::object_subclass]
    impl ObjectSubclass for Viewport {
        const NAME: &'static str = "TasktjikViewport";
        type Type = super::Viewport;
        type ParentType = gtk::Widget;
    }
    impl ObjectImpl for Viewport {
        fn dispose(&self) {
            self.changed.borrow_mut().take();
            while let Some(child) = self.obj().first_child() {
                child.unparent();
            }
        }
    }
    impl WidgetImpl for Viewport {
        fn request_mode(&self) -> gtk::SizeRequestMode {
            self.obj()
                .first_child()
                .map(|child| child.request_mode())
                .unwrap_or(gtk::SizeRequestMode::ConstantSize)
        }
        fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            let measured = self
                .obj()
                .first_child()
                .map(|child| child.measure(orientation, for_size))
                .unwrap_or((0, 0, -1, -1));
            // Existing multi-column rows must not prevent a narrower allocation;
            // the width callback rebuilds them at the new column count.
            if orientation == gtk::Orientation::Horizontal {
                (0, measured.1, -1, -1)
            } else {
                measured
            }
        }
        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            if let Some(child) = self.obj().first_child() {
                child.allocate(width, height, baseline, None);
            }
            if self.width.replace(width) == width || self.queued.replace(true) {
                return;
            }
            let weak = self.obj().downgrade();
            glib::idle_add_local_once(move || {
                if let Some(view) = weak.upgrade() {
                    view.imp().queued.set(false);
                    if let Some(changed) = view.imp().changed.borrow().as_ref() {
                        changed(view.imp().width.get());
                    }
                }
            });
        }
    }
}
glib::wrapper! {
    pub struct Viewport(ObjectSubclass<imp::Viewport>) @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}
impl Viewport {
    pub fn new(child: &impl IsA<gtk::Widget>) -> Self {
        let view: Self = glib::Object::new();
        child.set_parent(&view);
        view.set_vexpand(true);
        view
    }
    pub fn connect_width(&self, callback: impl Fn(i32) + 'static) {
        *self.imp().changed.borrow_mut() = Some(Box::new(callback));
    }
}
