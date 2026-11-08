//! A finite rotation keeps pin state visible without adding a status row.
use gtk::{glib, graphene, prelude::*, subclass::prelude::*};
use std::cell::{Cell, RefCell};
mod imp {
    use super::*;
    #[derive(Default)]
    pub struct Mark {
        pub label: gtk::Label,
        pub angle: Cell<f32>,
        pub tick: RefCell<Option<gtk::TickCallbackId>>,
    }
    #[glib::object_subclass]
    impl ObjectSubclass for Mark {
        const NAME: &'static str = "TjikPinMark";
        type Type = super::Mark;
        type ParentType = gtk::Widget;
    }
    impl ObjectImpl for Mark {
        fn constructed(&self) {
            self.parent_constructed();
            self.label.set_text("⌘");
            self.label.add_css_class("command-symbol");
            self.label.set_parent(&*self.obj());
        }
        fn dispose(&self) {
            if let Some(t) = self.tick.borrow_mut().take() {
                t.remove();
            }
            self.label.unparent();
        }
    }
    impl WidgetImpl for Mark {
        fn measure(&self, _: gtk::Orientation, _: i32) -> (i32, i32, i32, i32) {
            (36, 36, -1, -1)
        }
        fn size_allocate(&self, w: i32, h: i32, b: i32) {
            self.label.allocate(w, h, b, None);
        }
        fn snapshot(&self, s: &gtk::Snapshot) {
            let obj = self.obj();
            s.save();
            s.translate(&graphene::Point::new(
                obj.width() as f32 / 2.,
                obj.height() as f32 / 2.,
            ));
            s.rotate(self.angle.get());
            s.translate(&graphene::Point::new(
                -obj.width() as f32 / 2.,
                -obj.height() as f32 / 2.,
            ));
            obj.snapshot_child(&self.label, s);
            s.restore();
        }
    }
}
glib::wrapper! {pub struct Mark(ObjectSubclass<imp::Mark>) @extends gtk::Widget, @implements gtk::Accessible,gtk::Buildable,gtk::ConstraintTarget;}
impl Default for Mark {
    fn default() -> Self {
        glib::Object::new()
    }
}
impl Mark {
    pub fn angle(&self) -> f32 {
        self.imp().angle.get()
    }
    pub fn pinned(&self, pinned: bool) {
        if let Some(t) = self.imp().tick.borrow_mut().take() {
            t.remove();
        }
        let target = if pinned { 45. } else { 0. };
        let from = self.imp().angle.get();
        self.set_tooltip_text(Some(if pinned { "Pinned" } else { "Launcher" }));
        if !self.is_mapped() || !crate::motion::enabled(self) {
            self.imp().angle.set(target);
            self.queue_draw();
            return;
        }
        let start = Cell::new(None);
        *self.imp().tick.borrow_mut() = Some(self.add_tick_callback(move |mark, clock| {
            let initial = start.get().unwrap_or_else(|| {
                start.set(Some(clock.frame_time()));
                clock.frame_time()
            });
            let p = ((clock.frame_time() - initial) as f32 / 180000.).clamp(0., 1.);
            mark.imp()
                .angle
                .set(from + (target - from) * (1. - (1. - p).powi(3)));
            mark.queue_draw();
            if p >= 1. {
                mark.imp().tick.borrow_mut().take();
                glib::ControlFlow::Break
            } else {
                glib::ControlFlow::Continue
            }
        }));
    }
}
