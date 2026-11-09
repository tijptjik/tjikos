//! QML-compatible paged action footer: sender indexes survive visual pagination.
use super::{model::*, primitives::*, Context};
use gtk::{glib, prelude::*};
use std::{cell::Cell, rc::Rc};
pub(super) fn footer(ctx: &Rc<Context>) -> Option<gtk::Box> {
    let n = ctx.notice.borrow().clone();
    if n.archived() || ctx.state.borrow().blocked {
        return None;
    }
    let actions = n.0["actions"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
        .filter(|(_, a)| !text(&a["label"]).trim().is_empty())
        .map(|(i, a)| (i, text(&a["label"]), a["identifier"] == "default"))
        .collect::<Vec<_>>();
    if actions.is_empty() {
        return None;
    }
    let primary = actions.iter().find(|a| a.2).unwrap_or(&actions[0]).0;
    let bar = row(2);
    let out = action_footer(&bar);
    let previous = button("‹", "actions-previous");
    let next = button("›", "actions-next");
    for b in [&previous, &next] {
        b.add_css_class("quiet");
        b.add_css_class("page-action");
        b.set_width_request(32);
    }
    // Keep the navigation hit areas, but bring their glyphs toward the labels.
    for (button, alignment) in [(&previous, 1.), (&next, 0.)] {
        if let Some(label) = button.child().and_then(|w| w.downcast::<gtk::Label>().ok()) {
            label.set_xalign(alignment);
        }
    }
    let stack = gtk::Stack::new();
    stack.set_hexpand(true);
    stack.set_hhomogeneous(false);
    stack.set_vhomogeneous(true);
    stack.set_transition_type(gtk::StackTransitionType::SlideLeftRight);
    stack.set_transition_duration(180);
    bar.append(&previous);
    bar.append(&stack);
    bar.append(&next);
    let pages = Rc::new(Cell::new(1usize));
    for (b, forward) in [(&previous, false), (&next, true)] {
        let stack = stack.clone();
        let c = ctx.clone();
        let pages = pages.clone();
        let prev = previous.clone();
        let next = next.clone();
        b.connect_clicked(move |_| {
            let page = if forward {
                (c.action_page.get() + 1).min(pages.get() - 1)
            } else {
                c.action_page.get().saturating_sub(1)
            };
            c.action_page.set(page);
            stack.set_visible_child_name(&page.to_string());
            prev.set_opacity(if page > 0 { 1. } else { 0. });
            prev.set_sensitive(page > 0);
            next.set_opacity(if page + 1 < pages.get() { 1. } else { 0. });
            next.set_sensitive(page + 1 < pages.get());
        });
    }
    let last = Cell::new(0);
    let c = ctx.clone();
    bar.add_tick_callback(move |bar, _| {
        let width = bar.width();
        if width <= 0 || last.replace(width) == width {
            return glib::ControlFlow::Continue;
        }
        let widths = actions
            .iter()
            .map(|a| {
                let layout = bar.create_pango_layout(Some(&a.1));
                if let Some(mut font) = layout
                    .font_description()
                    .or_else(|| bar.pango_context().font_description())
                {
                    font.set_size(
                        (font.size() - 2 * gtk::pango::SCALE).max(10 * gtk::pango::SCALE),
                    );
                    layout.set_font_description(Some(&font));
                }
                layout.pixel_size().0 + 4
            })
            .collect::<Vec<_>>();
        let overflow = actions.len() > 1
            && widths.iter().sum::<i32>() + 8 * (actions.len().saturating_sub(1) as i32) > width;
        previous.set_visible(overflow);
        next.set_visible(overflow);
        let available = (width - if overflow { 68 } else { 0 }).max(1);
        let mut groups: Vec<Vec<usize>> = vec![vec![]];
        let mut used = 0;
        for (i, size) in widths.iter().enumerate() {
            let size = (*size).min(available);
            if used > 0 && used + 8 + size > available {
                groups.push(vec![]);
                used = 0;
            }
            if used > 0 {
                used += 8;
            }
            used += size;
            groups.last_mut().unwrap().push(i);
        }
        while let Some(child) = stack.first_child() {
            stack.remove(&child);
        }
        for (page, group) in groups.iter().enumerate() {
            let r = row(8);
            for i in group
                .iter()
                .filter(|i| actions[**i].0 != primary)
                .chain(group.iter().filter(|i| actions[**i].0 == primary))
            {
                let a = &actions[*i];
                let b = button(&a.1, &format!("card-action-{}", a.0));
                b.add_css_class("quiet");
                if let Some(l) = b.child().and_then(|w| w.downcast::<gtk::Label>().ok()) {
                    // Labels must negotiate within the card, never widen its body.
                    l.set_max_width_chars(1);
                    if a.0 == primary {
                        l.set_xalign(1.);
                    }
                    b.set_hexpand(true);
                    b.set_halign(gtk::Align::Fill);
                }
                let c = c.clone();
                let index = a.0;
                b.connect_clicked(move |_| c.emit(Intent::Invoke(index)));
                r.append(&b);
            }
            stack.add_named(&r, Some(&page.to_string()));
        }
        pages.set(groups.len());
        let page = c.action_page.get().min(groups.len() - 1);
        c.action_page.set(page);
        stack.set_visible_child_name(&page.to_string());
        previous.set_opacity(if page > 0 { 1. } else { 0. });
        previous.set_sensitive(page > 0);
        next.set_opacity(if page + 1 < pages.get() { 1. } else { 0. });
        next.set_sensitive(page + 1 < pages.get());
        glib::ControlFlow::Continue
    });
    Some(out)
}
