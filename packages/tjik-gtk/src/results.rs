use gtk::{glib, prelude::*};
/// Application-supplied presentation of one model item. No provider or transport schema
/// is imposed by the widget factory. Text is escaped before use in markup.
#[derive(Clone, Debug, Default)]
pub struct Presentation {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub image: String,
    pub fallback_icon: String,
    /// Nonempty text replaces the image (for example, a glyph or emoji).
    pub text_preview: String,
    /// Use thumbnail dimensions in list layout.
    pub large_image: bool,
    pub closing: bool,
    /// Leading status dots, capped at eight to keep the title readable.
    pub markers: u8,
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
/// `present` runs on the GTK thread when an item is bound. Keep it free of blocking I/O.
/// The adapter accepts any GObject model item, including application-defined types.
///
/// ```no_run
/// use tjik_gtk::{gtk::{self, prelude::*}, results::{self, Layout, Presentation}};
/// gtk::init().unwrap();
/// let factory = results::factory(Layout::List, 40, |_| {}, |object| {
///     let title = object.downcast_ref::<gtk::StringObject>()
///         .map(|item| item.string().to_string()).unwrap_or_default();
///     Presentation { title, fallback_icon: "text-x-generic".into(), ..Default::default() }
/// });
/// ```
pub fn factory(
    layout: Layout,
    icon_size: i32,
    selected: impl Fn(u32) + 'static,
    present: impl Fn(&glib::Object) -> Presentation + 'static,
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
        let row = crate::result_row::ResultRow::new(
            item,
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
        emoji.set_xalign(0.5);
        emoji.set_yalign(0.5);
        emoji.set_size_request(
            if vertical { icon_size } else { 104 },
            if vertical { icon_size } else { 56 },
        );
        if !vertical {
            emoji.set_hexpand(true);
            emoji.set_vexpand(true);
        }
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
            if let Some(row) = row.downcast_ref::<crate::result_row::ResultRow>() {
                row.set_closing(false);
            }
            if let Some(icon) = row.first_child().and_then(|slot| slot.first_child()) {
                icon.set_widget_name("");
            }
        }
    });
    factory.connect_bind(move |_, object| {
        let item = object.downcast_ref::<gtk::ListItem>().unwrap();
        let value = item.item().as_ref().map(&present).unwrap_or_default();
        let row = item
            .child()
            .unwrap()
            .downcast::<crate::result_row::ResultRow>()
            .unwrap();
        row.set_widget_name(&value.id);
        row.set_closing(value.closing);
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
        let preview = &value.text_preview;
        let is_emoji = !preview.is_empty();
        icon.set_visible(!is_emoji);
        emoji.set_visible(is_emoji);
        emoji.set_css_classes(&["emoji-preview"]);
        emoji.set_markup(&format!(
            "<span size=\"{}\">{}</span>",
            if matches!(layout, Layout::List) {
                30000
            } else {
                42000
            },
            glib::markup_escape_text(preview)
        ));
        if matches!(layout, Layout::List) {
            let image = value.large_image;
            icon.set_pixel_size(if image { 80 } else { 40 });
            row.set_height_request(if image { 104 } else { 64 });
        }
        crate::images::set_initial(&icon, &value.image, &value.fallback_icon);
        let labels = row.last_child().unwrap().downcast::<gtk::Box>().unwrap();
        let title = labels
            .first_child()
            .unwrap()
            .downcast::<gtk::Label>()
            .unwrap();
        let markers = "<span foreground=\"#ff53a6\" size=\"8000\">●</span> "
            .repeat(value.markers.min(8) as usize);
        title.set_markup(&format!(
            "{markers}{}",
            glib::markup_escape_text(&value.title)
        ));
        let subtitle = labels
            .last_child()
            .unwrap()
            .downcast::<gtk::Label>()
            .unwrap();
        subtitle.set_text(&value.subtitle);
        subtitle.set_visible(matches!(layout, Layout::List));
        // Informational rows remain readable; activation is guarded by the backend.
        row.set_sensitive(true);
    });
    factory
}
