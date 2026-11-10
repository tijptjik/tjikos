//! Wheel navigation in complete rendered rows, independent of viewport height.
use gtk::{glib, prelude::*};

pub fn row_pitch(content: &impl IsA<gtk::Widget>, viewport: &impl IsA<gtk::Widget>) -> Option<f64> {
    // Use allocated list-item slots, not their differently sized contents or
    // unmapped recycled widgets (whose zero bounds create tiny false pitches).
    let bounds: Vec<_> = crate::removal::rows(content)
        .iter()
        .filter_map(|row| row.parent())
        .filter(|slot| slot.is_mapped() && slot.width() > 0 && slot.height() > 0)
        .filter_map(|slot| slot.compute_bounds(viewport))
        .collect();
    let min_height = bounds
        .iter()
        .map(|b| b.height() as f64)
        .filter(|height| *height > 1.)
        .min_by(f64::total_cmp)?;
    let mut positions: Vec<f64> = bounds.iter().map(|b| b.y() as f64).collect();
    positions.sort_by(f64::total_cmp);
    positions.dedup_by(|a, b| (*a - *b).abs() < 1.);
    positions
        .windows(2)
        .map(|pair| pair[1] - pair[0])
        .filter(|step| *step >= min_height - 1.)
        .min_by(f64::total_cmp)
}

/// Capture each vertical scroll event before GTK's viewport-sized default.
/// DISCRETE would suppress fractional deltas before this handler sees them.
pub fn by_row(
    scroll: &gtk::ScrolledWindow,
    content: &impl IsA<gtk::Widget>,
    selection: &gtk::SingleSelection,
) {
    let pointer = std::rc::Rc::new(std::cell::Cell::new(None::<(f64, f64)>));
    let location = std::rc::Rc::new(std::cell::Cell::new(None::<(f64, f64)>));
    let motion = gtk::EventControllerMotion::new();
    motion.set_propagation_phase(gtk::PropagationPhase::Capture);
    let latest = location.clone();
    motion.connect_motion(move |_, x, y| latest.set(Some((x, y))));
    scroll.add_controller(motion);
    let settled_pointer = pointer.clone();
    let selection = selection.clone();
    let settled = std::rc::Rc::new(std::cell::RefCell::new(None::<glib::SourceId>));
    let weak = scroll.downgrade();
    scroll.vadjustment().connect_value_changed(move |_| {
        let Some(scroll) = weak.upgrade() else { return };
        let mut ancestor = scroll.parent();
        while let Some(widget) = ancestor {
            if widget.has_css_class("keyboard-navigation") {
                scroll.remove_css_class("scrolling");
                return;
            }
            ancestor = widget.parent();
        }
        scroll.add_css_class("scrolling");
        if let Some(timer) = settled.borrow_mut().take() {
            timer.remove();
        }
        let weak = scroll.downgrade();
        let settled_again = settled.clone();
        let pointer = settled_pointer.clone();
        let selection = selection.clone();
        *settled.borrow_mut() = Some(glib::timeout_add_local_once(
            std::time::Duration::from_millis(160),
            move || {
                settled_again.borrow_mut().take();
                if let Some(scroll) = weak.upgrade() {
                    let mut ancestor = scroll.parent();
                    while let Some(widget) = ancestor {
                        if widget.has_css_class("keyboard-navigation") {
                            pointer.take();
                            break;
                        }
                        ancestor = widget.parent();
                    }
                    if let Some((x, y)) = pointer.take() {
                        let mut picked = scroll.pick(x, y, gtk::PickFlags::DEFAULT);
                        while let Some(widget) = picked {
                            if widget.has_css_class("result") {
                                if let Some(position) = widget
                                    .downcast_ref::<crate::result_row::ResultRow>()
                                    .and_then(|row| row.position())
                                {
                                    selection.set_selected(position);
                                }
                                break;
                            }
                            picked = widget.parent();
                        }
                    }
                    scroll.remove_css_class("scrolling");
                }
            },
        ));
    });
    let controller = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
    controller.set_propagation_phase(gtk::PropagationPhase::Capture);
    let viewport = scroll.downgrade();
    let content = content.as_ref().downgrade();
    controller.connect_scroll(move |controller, _, dy| {
        let (Some(scroll), Some(content)) = (viewport.upgrade(), content.upgrade()) else {
            return glib::Propagation::Proceed;
        };
        let Some(step) = row_pitch(&content, &scroll) else {
            return glib::Propagation::Proceed;
        };
        if dy == 0. {
            return glib::Propagation::Proceed;
        }
        let mut ancestor = scroll.parent();
        while let Some(widget) = ancestor {
            widget.remove_css_class("keyboard-navigation");
            ancestor = widget.parent();
        }
        pointer.set(location.get());
        let adjustment = scroll.vadjustment();
        let value = adjustment.value() - adjustment.lower();
        let row = if dy > 0. {
            (value / step + 0.001).floor()
        } else {
            (value / step - 0.001).ceil()
        };
        let notches = if controller.unit() == gtk::gdk::ScrollUnit::Wheel {
            dy.signum() * dy.abs().ceil()
        } else {
            dy.signum()
        };
        let target = adjustment.lower() + (row + notches) * step;
        adjustment.set_value(target.clamp(
            adjustment.lower(),
            (adjustment.upper() - adjustment.page_size()).max(adjustment.lower()),
        ));
        glib::Propagation::Stop
    });
    scroll.add_controller(controller);
}
