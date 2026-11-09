//! Read-only Codex presentation. Data remains owned by the existing quota provider.
use crate::{
    panel_view::Content,
    util::{arr, s},
};
use serde_json::Value;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use tjik_gtk::gtk::{self, glib, prelude::*, subclass::prelude::*};

#[derive(Debug, PartialEq)]
pub struct Countdown {
    pub hours: Option<f64>,
    pub label: String,
}
pub fn countdown(reset: Option<f64>, now: f64) -> Countdown {
    let Some(reset) = reset.filter(|n| n.is_finite()) else {
        return Countdown {
            hours: None,
            label: "Unknown".into(),
        };
    };
    let seconds = (reset - now).max(0.);
    let minutes = (seconds / 60.).ceil() as u64;
    let (days, hours, rest) = (minutes / 1440, minutes % 1440 / 60, minutes % 60);
    let label = if seconds <= 0. {
        "Due now".into()
    } else if days > 0 {
        format!("{days}d {hours}h")
    } else if hours > 0 {
        format!("{hours}h {rest}m")
    } else {
        format!("{rest}m")
    };
    Countdown {
        hours: Some((seconds / 3600.).min(168.)),
        label,
    }
}
/// Exact weekly-scale semantics of CodexTimeline.js; None is unknown, not zero.
pub fn portions(hours: Option<f64>, credit: Option<f64>, day: usize) -> Option<[f64; 3]> {
    let hours = hours.filter(|n| n.is_finite())?;
    let credit = credit.filter(|n| n.is_finite() && *n >= 0.)?;
    let time = ((hours - day as f64 * 24.) / 24.).clamp(0., 1.);
    let capacity = (credit.min(1.) * 7. - day as f64).clamp(0., 1.);
    Some([
        time.min(capacity),
        (time - capacity).max(0.),
        (capacity - time).max(0.),
    ])
}
pub fn remaining(credit: Option<f64>) -> String {
    credit
        .filter(|n| n.is_finite() && *n >= 0.)
        .map(|n| format!("{}%", (n.min(1.) * 1000.).round() / 10.))
        .unwrap_or_else(|| "—".into())
}
pub fn compact(number: Option<f64>) -> String {
    let Some(n) = number.filter(|n| n.is_finite()) else {
        return "—".into();
    };
    for (scale, suffix) in [(1e9, "B"), (1e6, "M"), (1e3, "k")] {
        if n.abs() >= scale {
            return format!("{:.1}{suffix}", n / scale);
        }
    }
    n.to_string()
}
fn now() -> f64 {
    chrono::Utc::now().timestamp() as f64
}
fn label(text: &str, class: &str, align: f32) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.add_css_class(class);
    label.set_xalign(align);
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    label
}
fn caption(text: &str) -> gtk::Label {
    let label = label(text, "codex-caption", 0.);
    let attrs = gtk::pango::AttrList::new();
    attrs.insert(gtk::pango::AttrInt::new_letter_spacing(gtk::pango::SCALE));
    label.set_attributes(Some(&attrs));
    label
}
// Native labels with the reference's explicit account-row geometry. Keeping each
// label as a child preserves accessibility while allowing the raised plan caption.
mod account_imp {
    use super::*;
    #[derive(Default)]
    pub struct Account {
        pub children: RefCell<Vec<gtk::Widget>>,
    }
    #[glib::object_subclass]
    impl ObjectSubclass for Account {
        const NAME: &'static str = "BartjikCodexAccount";
        type Type = super::Account;
        type ParentType = gtk::Widget;
    }
    impl ObjectImpl for Account {
        fn dispose(&self) {
            for child in self.children.borrow_mut().drain(..) {
                child.unparent();
            }
        }
    }
    impl WidgetImpl for Account {
        fn measure(&self, orientation: gtk::Orientation, _: i32) -> (i32, i32, i32, i32) {
            if orientation == gtk::Orientation::Vertical {
                (52, 52, -1, -1)
            } else {
                (120, 408, -1, -1)
            }
        }
        fn size_allocate(&self, width: i32, _: i32, _: i32) {
            let children = self.children.borrow();
            if children.len() != 5 {
                return;
            }
            let natural = |i: usize| children[i].measure(gtk::Orientation::Horizontal, -1).1;
            let plan = natural(1);
            let usage = natural(2);
            let name = natural(0).min((width - plan - usage - 24).max(0));
            for (index, x, y, w) in [
                (0, 0, 1, name),
                (1, name + 4, -4, plan),
                (2, width - usage, 1, usage),
                (3, width - 90, 27, 90),
                (4, 0, 32, (width - 96).max(0)),
            ] {
                let child = &children[index];
                let height = if index == 4 {
                    10
                } else {
                    child.measure(gtk::Orientation::Vertical, w).1
                };
                child.allocate(
                    w,
                    height,
                    -1,
                    Some(
                        gtk::gsk::Transform::new()
                            .translate(&gtk::graphene::Point::new(x as f32, y as f32)),
                    ),
                );
            }
        }
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            for child in self.children.borrow().iter() {
                self.obj().snapshot_child(child, snapshot);
            }
        }
    }
}
glib::wrapper! { pub struct Account(ObjectSubclass<account_imp::Account>) @extends gtk::Widget, @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget; }
impl Account {
    fn new(children: Vec<gtk::Widget>) -> Self {
        let row: Self = glib::Object::new();
        for child in &children {
            child.set_parent(&row);
        }
        *row.imp().children.borrow_mut() = children;
        row
    }
}
fn meter(row: &Value, instant: Rc<Cell<f64>>, style: &Value) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.set_content_height(10);
    area.set_hexpand(true);
    area.set_valign(gtk::Align::Center);
    let reset = row["resetAt"].as_f64();
    let credit = row["progress"].as_f64();
    let surface = gtk::gdk::RGBA::parse(s(&style["surface"])).unwrap_or(gtk::gdk::RGBA::BLACK);
    area.set_draw_func(move |_, cr, width, _| {
        let day_width = ((width as f64 - 36.) / 7.).max(0.);
        let hours = countdown(reset, instant.get()).hours;
        for day in 0..7 {
            let segments = portions(hours, credit, day);
            let x = day as f64 * (day_width + 6.);
            let r = 2f64.min(day_width / 2.);
            let _ = cr.save();
            cr.new_sub_path();
            cr.arc(x + day_width - r, r, r, -std::f64::consts::FRAC_PI_2, 0.);
            cr.arc(
                x + day_width - r,
                10. - r,
                r,
                0.,
                std::f64::consts::FRAC_PI_2,
            );
            cr.arc(
                x + r,
                10. - r,
                r,
                std::f64::consts::FRAC_PI_2,
                std::f64::consts::PI,
            );
            cr.arc(
                x + r,
                r,
                r,
                std::f64::consts::PI,
                std::f64::consts::PI * 1.5,
            );
            cr.close_path();
            cr.clip();
            if segments.is_some() {
                cr.set_source_rgb(0., 0., 0.);
            } else {
                cr.set_source_rgba(
                    surface.red() as f64,
                    surface.green() as f64,
                    surface.blue() as f64,
                    surface.alpha() as f64,
                );
            }
            let _ = cr.paint();
            let mut offset = x;
            for (amount, color) in segments.unwrap_or([0.; 3]).into_iter().zip([
                [235., 111., 146.],
                [113., 136., 255.],
                [47., 135., 171.],
            ]) {
                cr.set_source_rgb(color[0] / 255., color[1] / 255., color[2] / 255.);
                let width = day_width * amount;
                if width > 0. {
                    // Only the joined colored edges are square; the end of a partial
                    // day remains rounded just like the QML segment rectangles.
                    let left = if offset == x {
                        2f64.min(width / 2.)
                    } else {
                        0.
                    };
                    let right = if offset == x
                        && segments.is_some_and(|p| p[1] > 0. || p[2] > 0.)
                        && amount == segments.unwrap()[0]
                    {
                        0.
                    } else {
                        2f64.min(width / 2.)
                    };
                    cr.new_sub_path();
                    cr.move_to(offset + left, 0.);
                    cr.line_to(offset + width - right, 0.);
                    cr.arc(
                        offset + width - right,
                        right,
                        right,
                        -std::f64::consts::FRAC_PI_2,
                        0.,
                    );
                    cr.line_to(offset + width, 10. - right);
                    cr.arc(
                        offset + width - right,
                        10. - right,
                        right,
                        0.,
                        std::f64::consts::FRAC_PI_2,
                    );
                    cr.line_to(offset + left, 10.);
                    cr.arc(
                        offset + left,
                        10. - left,
                        left,
                        std::f64::consts::FRAC_PI_2,
                        std::f64::consts::PI,
                    );
                    cr.line_to(offset, left);
                    cr.arc(
                        offset + left,
                        left,
                        left,
                        std::f64::consts::PI,
                        std::f64::consts::PI * 1.5,
                    );
                    cr.close_path();
                    let _ = cr.fill();
                }
                offset += day_width * amount;
            }
            let _ = cr.restore();
        }
    });
    area
}
pub fn content(row: &Value, style: &Value) -> Content {
    content_at(row, style, now(), true)
}
pub fn content_at(row: &Value, style: &Value, initial: f64, live: bool) -> Content {
    let fields = arr(&row["panel"]["rows"]);
    let field = |id: &str| {
        fields
            .iter()
            .find(|f| f["id"] == id)
            .unwrap_or(&Value::Null)
    };
    let body = gtk::Box::new(gtk::Orientation::Vertical, 16);
    body.add_css_class("codex-body");
    let heading = gtk::Box::new(gtk::Orientation::Vertical, 6);
    heading.set_height_request(if row["state"]["stale"] == true {
        36
    } else {
        16
    });
    let title = caption("WEEKLY ACCOUNTS");
    title.set_height_request(16);
    heading.append(&title);
    if row["state"]["stale"] == true {
        heading.append(&label("Cached · offline", "codex-muted", 0.));
    }
    body.append(&heading);
    let instant = Rc::new(Cell::new(initial));
    let mut refresh_rows = vec![];
    let accounts: Vec<_> = fields
        .iter()
        .filter(|f| f["group"] == "account" || f["progress"].is_number())
        .take(128)
        .collect();
    if accounts.is_empty() {
        body.append(&label("No accounts available", "codex-muted", 0.));
    }
    for account in accounts {
        let name = label(s(&account["label"]), "codex-account", 0.);
        let plan = label(s(&account["plan"]), "codex-plan", 0.);
        let percent = label(
            &remaining(account["progress"].as_f64()),
            "codex-percent",
            1.,
        );
        let track = meter(account, instant.clone(), style);
        let reset = countdown(account["resetAt"].as_f64(), initial);
        let remaining = label(&reset.label, "codex-reset", 1.);
        remaining.update_property(&[gtk::accessible::Property::Label(&format!(
            "Reset {}",
            reset.label
        ))]);
        let item = Account::new(vec![
            name.upcast(),
            plan.upcast(),
            percent.upcast(),
            remaining.clone().upcast(),
            track.clone().upcast(),
        ]);
        body.append(&item);
        refresh_rows.push((account["resetAt"].as_f64(), remaining, track));
    }
    let summary = gtk::Box::new(gtk::Orientation::Vertical, 12);
    summary.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    let summary_heading = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    summary_heading.set_height_request(16);
    let title = caption("LAST 24 HOURS");
    title.set_hexpand(true);
    summary_heading.append(&title);
    let failures = field("requests")["failures"].as_f64().unwrap_or(0.);
    if failures > 0. {
        summary_heading.append(&label(
            &format!("{failures:.0} failed requests"),
            "codex-muted",
            1.,
        ));
    }
    summary.append(&summary_heading);
    let grid = gtk::Grid::new();
    grid.set_column_homogeneous(true);
    grid.set_column_spacing(24);
    grid.set_row_spacing(6);
    let text = |id| {
        let value = s(&field(id)["value"]);
        if value.is_empty() {
            "—".to_owned()
        } else {
            value.to_owned()
        }
    };
    for (index, (name, value)) in [
        ("Requests", compact(field("requests")["amount"].as_f64())),
        ("Recorded cost", text("cost")),
        ("Input tokens", compact(field("tokens")["input"].as_f64())),
        ("Output tokens", compact(field("tokens")["output"].as_f64())),
        ("Cached tokens", compact(field("cached")["amount"].as_f64())),
        ("Avg. latency", text("latency")),
    ]
    .into_iter()
    .enumerate()
    {
        let cell = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        cell.set_height_request(28);
        cell.set_hexpand(true);
        let title = label(name, "codex-muted", 0.);
        title.set_hexpand(true);
        cell.append(&title);
        cell.append(&label(&value, "codex-stat", 1.));
        grid.attach(&cell, index as i32 % 2, index as i32 / 2, 1, 1);
    }
    summary.append(&grid);
    body.append(&summary);
    if live {
        // One minute timer per mapped card, torn down immediately when hidden/replaced.
        let refresh = Rc::new(move || {
            instant.set(now());
            for (reset, label, track) in &refresh_rows {
                let reset = countdown(*reset, instant.get());
                label.set_text(&reset.label);
                label.update_property(&[gtk::accessible::Property::Label(&format!(
                    "Reset {}",
                    reset.label
                ))]);
                track.queue_draw();
            }
        });
        let timer = Rc::new(RefCell::new(None::<glib::SourceId>));
        let active = timer.clone();
        body.connect_map(move |_| {
            refresh();
            if active.borrow().is_some() {
                return;
            }
            let refresh = refresh.clone();
            *active.borrow_mut() = Some(glib::timeout_add_local(
                std::time::Duration::from_secs(60),
                move || {
                    refresh();
                    glib::ControlFlow::Continue
                },
            ));
        });
        body.connect_unmap(move |_| {
            if let Some(timer) = timer.borrow_mut().take() {
                timer.remove();
            }
        });
    }
    Content {
        title: "Codex".into(),
        body: body.upcast(),
        footer: None,
        preferred_width: 440.,
        needs_keyboard: false,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn timeline_preserves_unknown_expired_and_separate_resources() {
        assert_eq!(countdown(None, 0.).label, "Unknown");
        assert_eq!(countdown(Some(0.), 1.).label, "Due now");
        assert_eq!(countdown(Some(44. * 3600.), 0.).label, "1d 20h");
        assert_eq!(countdown(Some(3601.), 0.).label, "1h 1m");
        assert_eq!(countdown(Some(1.), 0.).label, "1m");
        assert_eq!(portions(None, Some(0.3), 0), None);
        assert_eq!(portions(Some(44.), Some(-1.), 0), None);
        assert_eq!(portions(Some(44.), Some(0.), 0), Some([0., 1., 0.]));
        assert_eq!(portions(Some(0.), Some(1.), 6), Some([0., 0., 1.]));
        let p = portions(Some(44.), Some(0.2), 1).unwrap();
        assert!((p[0] - 0.4).abs() < 1e-10 && p[1] > 0.4 && p[2] == 0.);
        assert_eq!(remaining(Some(0.271)), "27.1%");
        assert_eq!(remaining(None), "—");
        assert_eq!(compact(Some(8853.)), "8.9k");
        assert_eq!(compact(Some(808798795.)), "808.8M");
        assert_eq!(compact(None), "—");
    }
}
