//! Snapshot fragments retain logical coordinates and are rasterized at output scale.
use gtk::{glib, graphene, gsk, prelude::*, subclass::prelude::*};
use std::cell::{Cell, RefCell};

mod imp {
    use super::*;
    #[derive(Default)]
    pub struct Fragments {
        pub node: RefCell<Option<gsk::RenderNode>>,
        pub progress: Cell<f32>,
        pub dissolve: Cell<bool>,
    }
    #[glib::object_subclass]
    impl ObjectSubclass for Fragments {
        const NAME: &'static str = "TjikRemovalFragments";
        type Type = super::Fragments;
        type ParentType = gtk::Widget;
    }
    impl ObjectImpl for Fragments {}
    impl WidgetImpl for Fragments {
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let Some(node) = self.node.borrow().clone() else {
                return;
            };
            let p = self.progress.get();
            let bounds = node.bounds();
            if !self.dissolve.get() {
                snapshot.push_opacity((1. - p) as f64);
                snapshot.translate(&graphene::Point::new(
                    bounds.width() * p * 0.175,
                    bounds.height() * p * 0.175,
                ));
                snapshot.scale(1. - p * 0.35, 1. - p * 0.35);
                snapshot.append_node(&node);
                snapshot.pop();
                return;
            }
            for y in (0..bounds.height().ceil() as i32).step_by(8) {
                for x in (0..bounds.width().ceil() as i32).step_by(8) {
                    let noise = ((x * 73 + y * 151 + 19) % 257) as f32 / 257.;
                    let phase = (p * 1.5 - noise * 0.35 - (1. - y as f32 / bounds.height()) * 0.15)
                        .clamp(0., 1.);
                    if phase >= 1. {
                        continue;
                    }
                    snapshot.save();
                    snapshot.push_opacity((1. - phase) as f64);
                    snapshot.translate(&graphene::Point::new(
                        x as f32 + (noise - 0.5) * 26. * phase,
                        y as f32 + phase * phase * (8. + noise * 16.),
                    ));
                    snapshot.scale(1. - phase * 0.8, 1. - phase * 0.8);
                    snapshot.push_clip(&graphene::Rect::new(0., 0., 8., 8.));
                    snapshot.translate(&graphene::Point::new(-(x as f32), -(y as f32)));
                    snapshot.append_node(&node);
                    snapshot.pop();
                    snapshot.pop();
                    snapshot.restore();
                }
            }
        }
    }
}
glib::wrapper! {
    pub struct Fragments(ObjectSubclass<imp::Fragments>) @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

pub fn cancel(overlay: &gtk::Overlay) {
    let mut child = overlay.first_child();
    while let Some(widget) = child {
        child = widget.next_sibling();
        if widget.is::<Fragments>() {
            overlay.remove_overlay(&widget);
        }
    }
}

/// Capture only confirmed removals, before recycling virtualized row widgets.
pub fn animate(overlay: &gtk::Overlay, row: &gtk::Widget, dissolve: bool) {
    if !crate::motion::enabled(row) || !row.is_mapped() {
        return;
    }
    let Some(bounds) = row.compute_bounds(overlay) else {
        return;
    };
    if bounds.y() + bounds.height() < 0. || bounds.y() > overlay.height() as f32 {
        return;
    }
    let snapshot = gtk::Snapshot::new();
    gtk::WidgetPaintable::new(Some(row)).snapshot(
        &snapshot,
        row.width() as f64,
        row.height() as f64,
    );
    let Some(node) = snapshot.to_node() else {
        return;
    };
    let effect: Fragments = glib::Object::new();
    *effect.imp().node.borrow_mut() = Some(node);
    effect.imp().dissolve.set(dissolve);
    effect.set_can_target(false);
    effect.set_halign(gtk::Align::Start);
    effect.set_valign(gtk::Align::Start);
    effect.set_margin_start(bounds.x().max(0.) as i32);
    effect.set_margin_top(bounds.y().max(0.) as i32);
    effect.set_size_request(bounds.width() as i32, bounds.height() as i32);
    overlay.add_overlay(&effect);
    let parent = overlay.downgrade();
    let start = Cell::new(None);
    effect.add_tick_callback(move |effect, clock| {
        let t = start.get().unwrap_or_else(|| {
            start.set(Some(clock.frame_time()));
            clock.frame_time()
        });
        let p =
            ((clock.frame_time() - t) as f32 / if dissolve { 380_000. } else { 180_000. }).min(1.);
        effect.imp().progress.set(p);
        effect.queue_draw();
        if p >= 1. {
            if let Some(parent) = parent.upgrade() {
                parent.remove_overlay(effect);
            }
            glib::ControlFlow::Break
        } else {
            glib::ControlFlow::Continue
        }
    });
}

pub fn rows(root: &impl IsA<gtk::Widget>) -> Vec<gtk::Widget> {
    let mut found = Vec::new();
    let mut child = root.first_child();
    while let Some(widget) = child {
        child = widget.next_sibling();
        if widget.has_css_class("result") {
            found.push(widget);
        } else {
            found.extend(rows(&widget));
        }
    }
    found
}
