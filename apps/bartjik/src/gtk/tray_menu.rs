//! DBusMenu presentation; the backend owns revisions, AboutToShow and validation.
use crate::util::{arr, s};
use serde_json::{json, Value};
use std::{
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
};
use tjik_gtk::gtk::{self, gdk, prelude::*};
pub struct Menu {
    pub popover: gtk::Popover,
    pub context: Value,
    pub owner: String,
    pub path: String,
    pub id: i32,
    css: gtk::CssProvider,
}
impl Drop for Menu {
    fn drop(&mut self) {
        self.popover.popdown();
        self.popover.unparent();
        if let Some(display) = gdk::Display::default() {
            gtk::style_context_remove_provider_for_display(&display, &self.css);
        }
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
    pub fn new(
        parent: &gtk::Widget,
        value: &Value,
        style: &Value,
        runtime: crate::runtime::Runtime,
    ) -> Self {
        let context = value["context"].clone();
        Self::with_dispatch(parent, value, style, move |id, submenu| {
            let _ = runtime.send(json!({"op":if submenu{"nativeMenu"}else{"nativeMenuEvent"},"context":context,"menuId":id}));
        })
    }
    fn with_dispatch(
        parent: &gtk::Widget,
        value: &Value,
        style: &Value,
        dispatch: impl Fn(i64, bool) + 'static,
    ) -> Self {
        let dispatch = Rc::new(dispatch);
        let context = value["context"].clone();
        let id = value["menuId"].as_i64().unwrap_or(0) as i32;
        let menu = &value["menu"];
        let popover = gtk::Popover::new();
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let scope = format!("bartjik-tray-menu-{}", NEXT.fetch_add(1, Ordering::Relaxed));
        popover.add_css_class(&scope);
        let css = gtk::CssProvider::new();
        css.load_from_string(&stylesheet(&scope, style));
        gtk::style_context_add_provider_for_display(
            &popover.display(),
            &css,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 3,
        );
        popover.set_parent(parent);
        // Match the reference non-grabbing popup; no invented input serial.
        popover.set_autohide(false);
        let anchor = &context["localAnchor"];
        if anchor.is_object() {
            popover.set_pointing_to(Some(&gdk::Rectangle::new(
                anchor["x"].as_f64().unwrap_or(0.) as i32,
                // Clear the whole bar, including the padding above its icons.
                0,
                anchor["width"].as_f64().unwrap_or(1.) as i32,
                anchor["height"].as_f64().unwrap_or(1.) as i32,
            )));
        }
        popover.set_position(gtk::PositionType::Top);
        popover.set_offset(0, -12);
        popover.set_has_arrow(false);
        let gap = (style["iconTextGap"].as_f64().unwrap_or(5.) / 2.).floor() as i32;
        let body = gtk::Box::new(gtk::Orientation::Vertical, gap);
        body.add_css_class("native-menu");
        body.set_width_request(260);
        let close = gtk::Button::new();
        let icon = gtk::Image::from_icon_name("window-close-symbolic");
        icon.set_pixel_size(16);
        close.set_child(Some(&icon));
        close.add_css_class("tray-menu-item");
        close.add_css_class("tray-menu-close");
        close.update_property(&[gtk::accessible::Property::Label("Close menu")]);
        close.set_tooltip_text(Some("Close menu"));
        close.set_halign(gtk::Align::End);
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
            back.add_css_class("tray-menu-item");
            if let Some(label) = back.child().and_downcast::<gtk::Label>() {
                label.set_xalign(0.);
            }
            let dispatch = dispatch.clone();
            back.connect_clicked(move |_| {
                dispatch(parent as i64, true);
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
                button.add_css_class("tray-menu-item");
                button.set_sensitive(p["enabled"] != false);
                button.set_halign(gtk::Align::Fill);
                if let Some(l) = button.child().and_downcast::<gtk::Label>() {
                    l.set_xalign(0.);
                }
                let item = child[0].as_i64().unwrap_or(0);
                let dispatch = dispatch.clone();
                let popup = popover.downgrade();
                button.connect_clicked(move |_| {
                    dispatch(item, submenu);
                    if !submenu {
                        if let Some(p) = popup.upgrade() {
                            p.popdown();
                        }
                    }
                });
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
            css,
        }
    }
}

fn stylesheet(scope: &str, style: &Value) -> String {
    let color = |key: &str, fallback: &str| {
        gdk::RGBA::parse(s(&style[key]))
            .unwrap_or_else(|_| gdk::RGBA::parse(fallback).unwrap())
            .to_string()
    };
    let padding = style["padding"].as_f64().unwrap_or(10.);
    let px = style["paddingX"].as_f64().unwrap_or(padding);
    let py = style["paddingY"].as_f64().unwrap_or(padding);
    let row_padding = (py * 0.6).round();
    let radius = style["radius"].as_f64().unwrap_or(11.);
    let font = s(&style["font"]).replace(['\\', '"', '\n', '\r'], " ");
    let size = style["fontSize"].as_f64().unwrap_or(14.);
    let background = gdk::RGBA::parse(s(&style["menuBackground"]))
        .or_else(|_| gdk::RGBA::parse(s(&style["background"])))
        .unwrap_or_else(|_| gdk::RGBA::parse("#191724").unwrap())
        .to_string();
    let border = color("panelBorder", "#393546");
    let text = color("text", "#e5e5e5");
    let muted = color("muted", "#a5a0ba");
    let surface = color("surface", "#26233a");
    let accent = color("accent", "#ff53a6");
    format!(
        r#"
popover.{scope} {{font-family:"{font}";font-size:{size}px;color:{text};}}
popover.{scope} contents {{background:{background};color:{text};border:1px solid {border};border-radius:{radius}px;padding:{py}px {px}px;box-shadow:none;}}
popover.{scope} .native-menu {{background:transparent;}}
popover.{scope} button.tray-menu-item {{background:transparent;background-image:none;color:{text};border:1px solid transparent;border-radius:6px;box-shadow:none;outline:none;margin:0;padding:{row_padding}px {px}px;min-height:20px;min-width:0;font-family:"{font}";font-size:{size}px;font-weight:normal;}}
popover.{scope} button.tray-menu-close {{padding:4px;min-width:16px;min-height:16px;}}
popover.{scope} button.tray-menu-item:hover {{background:{surface};}}
popover.{scope} button.tray-menu-item:active {{background:alpha({accent},0.15);}}
popover.{scope} button.tray-menu-item:focus-visible {{border-color:{accent};background:{surface};}}
popover.{scope} button.tray-menu-item:disabled {{color:{muted};}}
popover.{scope} separator {{background:{border};min-height:1px;padding:0;margin:{row_padding}px {px}px;border:0;}}
"#
    )
}

#[cfg(test)]
mod render_tests {
    use super::*;
    use std::{
        cell::RefCell,
        time::{Duration, Instant},
    };
    use tjik_gtk::gtk::{glib, prelude::SnapshotExt};

    #[test]
    #[ignore = "requires the workspace-7 synthetic menu fixture runner"]
    fn tray_menu_render() {
        gtk::init().unwrap();
        let style: Value =
            serde_json::from_str(include_str!("../../themes/rose-punk.json")).unwrap();
        let anchor = gtk::Button::with_label("Synthetic tray");
        anchor.set_valign(gtk::Align::End);
        let window = gtk::Window::builder()
            .title("Bartjik Tray Menu Fixture")
            .default_width(440)
            .default_height(600)
            .child(&anchor)
            .build();
        window.present();
        let context = glib::MainContext::default();
        let until = Instant::now() + Duration::from_millis(500);
        while Instant::now() < until {
            while context.pending() {
                context.iteration(false);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let value = json!({"menu":{"layout":[0,{},[
            [1,{"label":"Recent","enabled":false},[]],
            [2,{"label":"A synthetic conversation"},[]],
            [3,{"label":"Continue previous task"},[]],
            [4,{"label":"More","children-display":"submenu"},[]],
            [5,{"type":"separator"},[]],
            [6,{"label":"Notifications","toggle-type":"checkmark","toggle-state":1},[]],
            [7,{"label":"Available","toggle-type":"radio","toggle-state":1},[]],
            [8,{"label":"Open application"},[]],
            [9,{"type":"separator"},[]],
            [10,{"label":"Quit"},[]]
        ]]}});
        let dispatched = Rc::new(RefCell::new(vec![]));
        let events = dispatched.clone();
        let menu = Menu::with_dispatch(anchor.upcast_ref(), &value, &style, move |id, submenu| {
            events.borrow_mut().push((id, submenu));
        });
        let until = Instant::now() + Duration::from_secs(3);
        while Instant::now() < until {
            while context.pending() {
                context.iteration(false);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let body = menu
            .popover
            .child()
            .and_downcast::<gtk::ScrolledWindow>()
            .unwrap()
            .child()
            .unwrap();
        let body = body
            .clone()
            .downcast::<gtk::Viewport>()
            .map(|viewport| viewport.child().unwrap())
            .unwrap_or(body);
        let mut child = body.first_child();
        let mut buttons = vec![];
        while let Some(widget) = child {
            child = widget.next_sibling();
            if let Ok(button) = widget.downcast::<gtk::Button>() {
                let height = button.allocated_height();
                assert!(
                    if buttons.is_empty() {
                        height == 26
                    } else {
                        (34..=36).contains(&height)
                    },
                    "menu row height: {height}px"
                );
                buttons.push(button);
            }
        }
        assert_eq!(buttons.len(), 9);
        assert_eq!(
            buttons[0]
                .child()
                .and_downcast::<gtk::Image>()
                .unwrap()
                .icon_name()
                .as_deref(),
            Some("window-close-symbolic")
        );
        assert!(!buttons[1].is_sensitive());
        let popup = menu
            .popover
            .surface()
            .unwrap()
            .downcast::<gdk::Popup>()
            .unwrap();
        let anchor_top = anchor.compute_bounds(&window).unwrap().y();
        assert!(
            popup.position_y() as f32 + popup.height() as f32 <= anchor_top - 12.,
            "menu overlaps the tray anchor"
        );
        let shot = gtk::Snapshot::new();
        gtk::WidgetPaintable::new(Some(&menu.popover)).snapshot(
            &shot,
            menu.popover.width() as f64,
            menu.popover.height() as f64,
        );
        window
            .renderer()
            .unwrap()
            .render_texture(&shot.to_node().expect("rendered menu"), None)
            .save_to_png("/tmp/bartjik-tray-menu.png")
            .unwrap();
        buttons[4].emit_clicked();
        assert_eq!(*dispatched.borrow(), vec![(4, true)]);
        assert!(menu.popover.get_visible());
        buttons.last().unwrap().emit_clicked();
        assert_eq!(*dispatched.borrow(), vec![(4, true), (10, false)]);
        drop(menu);
        window.close();
    }
}
