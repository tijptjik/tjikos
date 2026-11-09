use gtk::{glib, prelude::*};
use serde_json::Value;
fn text<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().unwrap_or("")
}
#[derive(Clone, Copy)]
pub enum Layout {
    Grid,
    List,
    Gallery,
}
/// Keep keyboard selection until the pointer actually moves in surface coordinates.
/// GTK can send crossing/motion callbacks when rows move under a stationary mouse.
pub fn track_pointer(content: &impl IsA<gtk::Widget>) {
    let motion = gtk::EventControllerMotion::new();
    motion.set_propagation_phase(gtk::PropagationPhase::Capture);
    let previous = std::cell::Cell::new(None);
    let weak = content.as_ref().downgrade();
    motion.connect_motion(move |controller, _, _| {
        let Some(event) = controller.current_event() else {
            return;
        };
        let point = event.position();
        let old = previous.replace(point);
        if event.event_type() == gtk::gdk::EventType::MotionNotify && old.is_some() && old != point
        {
            if let Some(content) = weak.upgrade() {
                content.remove_css_class("keyboard-navigation");
            }
        }
    });
    content.add_controller(motion);
}

/// Factories are shared by launchers, history and pickers; GTK virtualizes their widgets.
pub fn factory(
    layout: Layout,
    icon_size: i32,
    selected: impl Fn(u32) + 'static,
) -> gtk::SignalListItemFactory {
    let selected = std::rc::Rc::new(selected);
    let factory = gtk::SignalListItemFactory::new();
    let pointer = std::rc::Rc::new(std::cell::Cell::new(None));
    factory.connect_setup(move |_, object| {
        let item = object.downcast_ref::<gtk::ListItem>().unwrap();
        // Selection belongs to our keyboard/pointer handlers. GTK single-click
        // activation otherwise schedules its own delayed hover selection.
        item.set_selectable(false);
        let vertical = !matches!(layout, Layout::List);
        let row = gtk::Box::new(
            if vertical {
                gtk::Orientation::Vertical
            } else {
                gtk::Orientation::Horizontal
            },
            if vertical { 12 } else { 14 },
        );
        row.add_css_class("result");
        row.set_size_request(
            -1,
            if matches!(layout, Layout::Gallery) {
                180
            } else if vertical {
                122
            } else {
                64
            },
        );
        let icon = gtk::Image::new();
        let icon_size = if matches!(layout, Layout::Gallery) {
            120
        } else {
            icon_size
        };
        icon.set_pixel_size(if vertical { icon_size } else { 40 });
        icon.set_margin_top(if vertical { 12 } else { 0 });
        icon.set_margin_start(if vertical { 0 } else { 15 });
        if vertical {
            icon.set_height_request(icon_size);
        }
        let icon_slot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        icon_slot.set_hexpand(false);
        if vertical {
            icon_slot.set_halign(gtk::Align::Center);
        }
        if !vertical {
            icon_slot.set_size_request(104, 56);
            icon_slot.set_halign(gtk::Align::Start);
            icon.set_hexpand(true);
            icon.set_halign(gtk::Align::Center);
            icon.set_margin_start(0);
        }
        icon_slot.append(&icon);
        let emoji = gtk::Label::new(None);
        emoji.add_css_class("emoji-preview");
        emoji.set_halign(gtk::Align::Center);
        emoji.set_valign(gtk::Align::Center);
        emoji.set_size_request(icon_size, icon_size);
        emoji.set_visible(false);
        icon_slot.append(&emoji);
        row.append(&icon_slot);
        let labels = gtk::Box::new(gtk::Orientation::Vertical, 4);
        labels.set_hexpand(true);
        if !vertical {
            labels.set_valign(gtk::Align::Center);
            labels.set_margin_end(14);
        }
        let title = gtk::Label::new(None);
        title.add_css_class("result-title");
        title.set_wrap(true);
        title.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        title.set_lines(2);
        title.set_ellipsize(gtk::pango::EllipsizeMode::End);
        title.set_justify(if vertical {
            gtk::Justification::Center
        } else {
            gtk::Justification::Left
        });
        title.set_xalign(if vertical { 0.5 } else { 0.0 });
        title.set_margin_start(6);
        title.set_margin_end(6);
        let subtitle = gtk::Label::new(None);
        subtitle.add_css_class("muted");
        subtitle.set_xalign(0.0);
        subtitle.set_ellipsize(gtk::pango::EllipsizeMode::End);
        labels.append(&title);
        labels.append(&subtitle);
        row.append(&labels);
        let motion = gtk::EventControllerMotion::new();
        let weak = item.downgrade();
        let select = selected.clone();
        let pointer = pointer.clone();
        motion.connect_motion(move |motion, _, _| {
            let mut ancestor = motion.widget();
            while let Some(widget) = ancestor {
                if widget.has_css_class("scrolling") || widget.has_css_class("keyboard-navigation")
                {
                    return;
                }
                ancestor = widget.parent();
            }
            let point = motion.current_event().and_then(|event| event.position());
            if pointer.replace(point) == point {
                return;
            }
            if let Some(item) = weak.upgrade() {
                select(item.position());
            }
        });
        row.add_controller(motion);
        item.set_child(Some(&row));
    });
    factory.connect_unbind(|_, object| {
        let item = object.downcast_ref::<gtk::ListItem>().unwrap();
        if let Some(row) = item.child() {
            crate::motion::closing(&row, false);
            if let Some(icon) = row.first_child().and_then(|slot| slot.first_child()) {
                icon.set_widget_name("");
            }
        }
    });
    factory.connect_bind(move |_, object| {
        let item = object.downcast_ref::<gtk::ListItem>().unwrap();
        let string = item
            .item()
            .unwrap()
            .downcast::<gtk::StringObject>()
            .unwrap();
        let value: Value = serde_json::from_str(&string.string()).unwrap_or(Value::Null);
        let row = item.child().unwrap().downcast::<gtk::Box>().unwrap();
        row.set_widget_name(text(&value, "id"));
        crate::motion::closing(&row, value["closing"] == true);
        let icon = row
            .first_child()
            .unwrap()
            .first_child()
            .unwrap()
            .downcast::<gtk::Image>()
            .unwrap();
        let emoji = icon
            .next_sibling()
            .unwrap()
            .downcast::<gtk::Label>()
            .unwrap();
        let is_emoji = text(&value, "provider") == "Emoji";
        icon.set_visible(!is_emoji);
        emoji.set_text(text(&value, "preview"));
        emoji.set_visible(is_emoji);
        emoji.set_css_classes(&["emoji-preview"]);
        emoji.set_markup(&format!(
            "<span size=\"{}\">{}</span>",
            if matches!(layout, Layout::List) {
                30000
            } else {
                42000
            },
            glib::markup_escape_text(text(&value, "preview"))
        ));
        if matches!(layout, Layout::List) {
            let image = !text(&value, "thumbnail").is_empty();
            icon.set_pixel_size(if image { 80 } else { 40 });
            row.set_height_request(if image { 104 } else { 64 });
        }
        let clipboard_icon = std::env::var("RUNTJIK_HOME")
            .map(|home| format!("{home}/qml/icons/commands/runtjik-cliphist.svg"))
            .unwrap_or_default();
        let source = if !text(&value, "thumbnail").is_empty() {
            text(&value, "thumbnail")
        } else if text(&value, "provider") == "Clipboard" && !clipboard_icon.is_empty() {
            &clipboard_icon
        } else if !text(&value, "favicon").is_empty() {
            text(&value, "favicon")
        } else {
            text(&value, "icon")
        };
        crate::images::set(
            &icon,
            source,
            if text(&value, "provider") == "Browser" {
                "web-browser"
            } else {
                "application-x-executable"
            },
        );
        let labels = row.last_child().unwrap().downcast::<gtk::Box>().unwrap();
        let title = labels
            .first_child()
            .unwrap()
            .downcast::<gtk::Label>()
            .unwrap();
        let window_marker = if value["windows"].as_u64().unwrap_or(0) > 0 {
            "<span foreground=\"#ff53a6\" size=\"8000\">●</span> "
        } else {
            ""
        };
        let tone_marker = if is_emoji && value["has_tones"] == true {
            "<span foreground=\"#ff53a6\" size=\"8000\">●</span> "
        } else {
            ""
        };
        title.set_markup(&format!(
            "{window_marker}{tone_marker}{}",
            glib::markup_escape_text(text(&value, "name"))
        ));
        let subtitle = labels
            .last_child()
            .unwrap()
            .downcast::<gtk::Label>()
            .unwrap();
        subtitle.set_text(text(&value, "subtitle"));
        subtitle.set_visible(matches!(layout, Layout::List));
        // Informational rows remain readable; activation is guarded by the backend.
        row.set_sensitive(true);
    });
    factory
}
