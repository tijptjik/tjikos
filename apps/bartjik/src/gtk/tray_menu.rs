//! DBusMenu presentation; the backend owns revisions, AboutToShow and validation.
use crate::util::{arr, s};
use serde_json::{json, Value};
use tjik_gtk::gtk::{self, gdk, prelude::*};
pub struct Menu {
    pub popover: gtk::Popover,
    pub context: Value,
    pub owner: String,
    pub path: String,
    pub id: i32,
}
impl Drop for Menu {
    fn drop(&mut self) {
        self.popover.popdown();
        self.popover.unparent();
    }
}
fn parent_id(node: &Value, id: i32) -> Option<i32> {
    for child in arr(&node[2]) {
        if child[0].as_i64() == Some(id as i64) {
            return node[0].as_i64().map(|i| i as i32);
        }
        if let Some(parent) = parent_id(child, id) {
            return Some(parent);
        }
    }
    None
}
fn find(node: &Value, id: i32) -> Option<&Value> {
    if node[0].as_i64() == Some(id as i64) {
        Some(node)
    } else {
        arr(&node[2]).iter().find_map(|n| find(n, id))
    }
}
impl Menu {
    pub fn new(parent: &gtk::Widget, value: &Value, runtime: crate::runtime::Runtime) -> Self {
        let context = value["context"].clone();
        let id = value["menuId"].as_i64().unwrap_or(0) as i32;
        let menu = &value["menu"];
        let popover = gtk::Popover::new();
        popover.set_parent(parent);
        // Match the reference non-grabbing popup; no invented input serial.
        popover.set_autohide(false);
        let anchor = &context["localAnchor"];
        if anchor.is_object() {
            popover.set_pointing_to(Some(&gdk::Rectangle::new(
                anchor["x"].as_f64().unwrap_or(0.) as i32,
                anchor["y"].as_f64().unwrap_or(0.) as i32,
                anchor["width"].as_f64().unwrap_or(1.) as i32,
                anchor["height"].as_f64().unwrap_or(1.) as i32,
            )));
        }
        popover.set_position(gtk::PositionType::Top);
        popover.set_has_arrow(false);
        let body = gtk::Box::new(gtk::Orientation::Vertical, 2);
        body.add_css_class("native-menu");
        body.set_margin_top(8);
        body.set_margin_bottom(8);
        body.set_margin_start(8);
        body.set_margin_end(8);
        let close = gtk::Button::with_label("Close");
        close.add_css_class("flat");
        let popup = popover.downgrade();
        close.connect_clicked(move |_| {
            if let Some(p) = popup.upgrade() {
                p.popdown();
            }
        });
        body.append(&close);
        let keys = gtk::EventControllerKey::new();
        let popup = popover.downgrade();
        keys.connect_key_pressed(move |_, key, _, _| {
            if key == gdk::Key::Escape {
                if let Some(p) = popup.upgrade() {
                    p.popdown();
                }
                return gtk::glib::Propagation::Stop;
            }
            gtk::glib::Propagation::Proceed
        });
        popover.add_controller(keys);
        if id != 0 {
            let parent = parent_id(&menu["layout"], id).unwrap_or(0);
            let back = gtk::Button::with_label("‹ Back");
            let c = context.clone();
            let run = runtime.clone();
            back.connect_clicked(move |_| {
                let _ = run.send(json!({"op":"nativeMenu","context":c,"menuId":parent}));
            });
            body.append(&back);
        }
        if let Some(node) = find(&menu["layout"], id) {
            for child in arr(&node[2]).iter().filter(|n| n[1]["visible"] != false) {
                let p = &child[1];
                if p["type"] == "separator" {
                    body.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
                    continue;
                }
                let checked = p["toggle-state"].as_i64() == Some(1);
                let prefix = match s(&p["toggle-type"]) {
                    "checkmark" => {
                        if checked {
                            "✓  "
                        } else {
                            "   "
                        }
                    }
                    "radio" => {
                        if checked {
                            "●  "
                        } else {
                            "○  "
                        }
                    }
                    _ => "",
                };
                let submenu = !arr(&child[2]).is_empty() || p["children-display"] == "submenu";
                let label = format!(
                    "{prefix}{}{}",
                    s(&p["label"]).replace('_', ""),
                    if submenu { "  ›" } else { "" }
                );
                let button = gtk::Button::with_label(&label);
                button.add_css_class("flat");
                button.set_sensitive(p["enabled"] != false);
                button.set_halign(gtk::Align::Fill);
                if let Some(l) = button.child().and_downcast::<gtk::Label>() {
                    l.set_xalign(0.);
                }
                let item = child[0].as_i64().unwrap_or(0);
                let c = context.clone();
                let run = runtime.clone();
                let popup = popover.downgrade();
                button.connect_clicked(move|_|{let _=run.send(json!({"op":if submenu{"nativeMenu"}else{"nativeMenuEvent"},"context":c,"menuId":item}));if !submenu{if let Some(p)=popup.upgrade(){p.popdown();}}});
                body.append(&button);
            }
        }
        let scroll = gtk::ScrolledWindow::builder()
            .max_content_height(600)
            .propagate_natural_height(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();
        scroll.set_child(Some(&body));
        popover.set_child(Some(&scroll));
        popover.popup();
        Self {
            popover,
            context,
            owner: s(&menu["owner"]).into(),
            path: s(&menu["path"]).into(),
            id,
        }
    }
}
