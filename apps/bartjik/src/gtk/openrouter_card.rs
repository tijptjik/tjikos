//! OpenRouter presentation; balances and daily usage remain provider-owned.
use crate::{
    panel_view::Content,
    util::{arr, s},
};
use serde_json::Value;
use tjik_gtk::gtk::{self, prelude::*};

fn label(text: &str, class: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.set_xalign(0.);
    label.add_css_class(class);
    label
}
fn text<'a>(panel: &'a Value, key: &str, fallback: &'a str) -> &'a str {
    panel[key]
        .as_str()
        .filter(|v| !v.is_empty())
        .unwrap_or(fallback)
}
pub fn content(row: &Value) -> Content {
    let panel = &row["panel"];
    let body = gtk::Box::new(gtk::Orientation::Vertical, 14);
    let balance = gtk::Box::new(gtk::Orientation::Vertical, 4);
    balance.append(&label(
        text(panel, "balanceLabel", "Credits remaining"),
        "router-muted",
    ));
    let amount = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    for (value, class) in [
        (text(panel, "balance", "—"), "router-balance"),
        ("USD", "router-muted"),
    ] {
        let value = label(value, class);
        value.set_valign(gtk::Align::Baseline);
        amount.append(&value);
    }
    balance.append(&amount);
    if row["state"]["stale"] == true {
        balance.append(&label(
            "Cached balance · waiting for an update",
            "panel-muted",
        ));
    }
    body.append(&balance);
    let divider = gtk::Separator::new(gtk::Orientation::Horizontal);
    divider.add_css_class("router-divider");
    body.append(&divider);
    let usage = gtk::Box::new(gtk::Orientation::Vertical, 10);
    let heading = label("DAILY TOKEN USAGE", "router-caption");
    let attrs = gtk::pango::AttrList::new();
    attrs.insert(gtk::pango::AttrInt::new_letter_spacing(gtk::pango::SCALE));
    heading.set_attributes(Some(&attrs));
    usage.append(&heading);
    let note = label(
        text(
            panel,
            "activityNote",
            "Balance and usage are currently unavailable.",
        ),
        "router-muted",
    );
    note.set_wrap(true);
    note.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    usage.append(&note);
    let days = gtk::Box::new(gtk::Orientation::Vertical, 4);
    for field in arr(&panel["rows"])
        .iter()
        .filter(|f| f["tokens"].is_number())
        .take(30)
    {
        let day = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        day.set_height_request(26);
        let date = label(s(&field["label"]), "router-date");
        date.set_hexpand(true);
        date.set_valign(gtk::Align::Center);
        let tokens = label(s(&field["value"]), "router-tokens");
        tokens.set_valign(gtk::Align::Center);
        day.append(&date);
        day.append(&tokens);
        days.append(&day);
    }
    if days.first_child().is_some() {
        usage.append(&days);
    }
    body.append(&usage);
    Content {
        title: text(panel, "title", "OpenRouter").into(),
        body: body.upcast(),
        footer: None,
        preferred_width: 420.,
        needs_keyboard: false,
    }
}
