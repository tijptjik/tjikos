//! Read-only presentation of the existing bar's items for same-session comparison.
//! This is a renderer comparison bridge, not a replacement provider or action host.
use crate::{
    config,
    util::{arr, num, s},
};
use serde_json::Value;
use std::collections::BTreeMap;
use tjik_gtk::gtk::{self, prelude::*};
fn rows(
    module: &Value,
    base: &Value,
    orientation: gtk::Orientation,
    cache: &mut BTreeMap<String, super::motion::Visual>,
    icons: &mut BTreeMap<String, super::icons::Icon>,
) -> gtk::Box {
    let mut previous = std::mem::take(cache);
    let mut previous_icons = std::mem::take(icons);
    let style = config::merge(base, &module["style"]);
    let group = gtk::Box::new(orientation, num(&style["itemGap"]) as i32);
    let activity = &module["comparisonActivity"];
    if module["provider"] == "icontray"
        && (activity["screenRecording"] == true
            || ["recording", "processing"].contains(&s(&activity["hyprvoice"])))
    {
        let icon = previous_icons
            .remove("__recording")
            .unwrap_or_else(super::icons::Icon::new);
        detach_icon(&icon);
        icon.update(
            &Value::Null,
            &style,
            false,
            false,
            true,
            activity["screenRecording"] != true && activity["hyprvoice"] == "processing",
        );
        let label = if activity["screenRecording"] == true {
            "Screen recording"
        } else if activity["hyprvoice"] == "processing" {
            "Hyprvoice processing"
        } else {
            "Hyprvoice recording"
        };
        icon.root.set_tooltip_text(Some(label));
        icon.root
            .update_property(&[gtk::accessible::Property::Label(label)]);
        group.append(&icon.root);
        icons.insert("__recording".into(), icon);
    }
    for row in arr(&module["comparisonItems"])
        .iter()
        .filter(|r| r["visible"] != false)
    {
        let item = gtk::Box::new(orientation, num(&style["iconTextGap"]) as i32);
        item.set_tooltip_text(row["tooltip"].as_str());
        let media =
            module["provider"] == "playerctl" && row["idle"] != true && row["artist"].is_object();
        let kinds = if media {
            vec!["icon", "artist", "separator", "track"]
        } else {
            vec!["icon", "text"]
        };
        for kind in kinds {
            if !media && row["type"] != kind && row["type"] != "icon_text" {
                continue;
            }
            let separator = serde_json::json!({"value":"·"});
            let part = if kind == "separator" {
                &separator
            } else {
                &row[kind]
            };
            let style = part_style(
                module,
                base,
                row,
                if kind == "separator" { "artist" } else { kind },
            );
            let size = if kind == "icon" {
                num(&style["iconSize"])
            } else {
                num(&style["fontSize"])
            }
            .max(1.) as i32;
            let value = s(&part["value"]);
            let widget: gtk::Widget =
                if kind == "icon" && ["image", "theme", "symbol"].contains(&s(&part["kind"])) {
                    let key = s(&row["id"]).to_owned();
                    let icon = previous_icons
                        .remove(&key)
                        .unwrap_or_else(super::icons::Icon::new);
                    detach_icon(&icon);
                    icon.update(
                        part,
                        &style,
                        module["provider"] == "icontray" && row["state"]["urgent"] == true,
                        module["comparisonTailnet"] == true,
                        false,
                        false,
                    );
                    let widget = icon.root.clone().upcast();
                    icons.insert(key, icon);
                    widget
                } else {
                    let key = serde_json::json!([row["id"], kind, value, style]).to_string();
                    let visual = if let Some(visual) = previous.remove(&key) {
                        if let Some(parent) = visual.parent().and_downcast::<gtk::Box>() {
                            parent.remove(&visual);
                        }
                        visual
                    } else {
                        let visual = super::motion::Visual::new();
                        visual.set_text(
                            value,
                            s(&style["font"]),
                            size as f64,
                            gtk::gdk::RGBA::parse(s(&style["color"]))
                                .or_else(|_| gtk::gdk::RGBA::parse(s(&style["text"])))
                                .unwrap_or(gtk::gdk::RGBA::WHITE),
                            style["maxTextWidth"].as_f64().unwrap_or(10000.),
                            if kind == "separator" {
                                0.
                            } else {
                                num(&style["minItemSize"])
                            },
                            20.,
                            style["marquee"] == true,
                        );
                        visual
                    };
                    cache.insert(key, visual.clone());
                    visual.upcast()
                };
            widget.set_valign(gtk::Align::Center);
            if kind == "icon" {
                widget.set_size_request(
                    size.max(num(&style["minItemSize"]) as i32),
                    size.max(num(&style["minItemSize"]) as i32),
                );
            }
            if row["enabled"] == false {
                widget.set_opacity(0.5);
            }
            item.append(&widget);
        }
        group.append(&item);
    }
    group
}

fn part_style(module: &Value, base: &Value, row: &Value, kind: &str) -> Value {
    let item = &module["items"][s(&row["id"])];
    let mut style = config::merge(&config::merge(base, &module["style"]), &item["style"]);
    if let Some(states) = row["state"].as_object() {
        for (key, value) in states {
            if value == true {
                style = config::merge(&style, &style["states"][key]);
            }
        }
    }
    for layer in [
        &row[kind]["style"],
        &module[kind]["style"],
        &item[kind]["style"],
    ] {
        style = config::merge(&style, layer);
    }
    style
}
pub struct Module {
    pub root: gtk::Box,
    base: Value,
    orientation: gtk::Orientation,
    last: Value,
    ring: Option<super::motion::Visual>,
    text_cache: BTreeMap<String, super::motion::Visual>,
    icons: BTreeMap<String, super::icons::Icon>,
    tray: Option<super::tray_motion::Tray>,
}
impl Module {
    pub fn new(module: &Value, base: &Value, orientation: gtk::Orientation) -> Self {
        let root = gtk::Box::new(orientation, 0);
        let ring = (module["provider"] == "workspaces" && module["appearance"] == "ring")
            .then(super::motion::Visual::new);
        if let Some(v) = &ring {
            root.append(v);
        }
        let tray = (module["provider"] == "icontray")
            .then(|| super::tray_motion::Tray::new(orientation == gtk::Orientation::Vertical));
        if let Some(t) = &tray {
            root.append(t);
        }
        let mut result = Self {
            root,
            base: base.clone(),
            orientation,
            last: Value::Null,
            ring,
            text_cache: BTreeMap::new(),
            icons: BTreeMap::new(),
            tray,
        };
        result.update(module);
        result
    }
    pub fn update(&mut self, module: &Value) {
        let mut presentation = module.clone();
        if let Some(rows) = presentation["comparisonItems"].as_array_mut() {
            for row in rows {
                if let Some(o) = row.as_object_mut() {
                    o.remove("tooltip");
                }
            }
        }
        if self.last == presentation {
            return;
        }
        self.last = presentation;
        self.root.set_visible(module["comparisonHidden"] != true);
        if let Some(tray) = &self.tray {
            let style = config::merge(&self.base, &module["style"]);
            let activity = &module["comparisonActivity"];
            let mut desired = vec![];
            if activity["screenRecording"] == true
                || ["recording", "processing"].contains(&s(&activity["hyprvoice"]))
            {
                desired.push((
                    "__recording".into(),
                    Value::Null,
                    style.clone(),
                    false,
                    true,
                    activity["screenRecording"] != true && activity["hyprvoice"] == "processing",
                ));
            }
            for row in arr(&module["comparisonItems"])
                .iter()
                .filter(|r| r["visible"] != false)
            {
                desired.push((
                    s(&row["id"]).into(),
                    row["icon"].clone(),
                    part_style(module, &self.base, row, "icon"),
                    row["state"]["urgent"] == true,
                    false,
                    false,
                ));
            }
            tray.update(
                desired,
                num(&style["itemGap"]),
                style["animateIcons"] != false,
            );
        } else if let Some(v) = &self.ring {
            let entries = arr(&module["comparisonItems"])
                .iter()
                .filter(|r| r["visible"] != false)
                .map(|row| {
                    let style = part_style(module, &self.base, row, "text");
                    let text = s(&row["text"]["value"]);
                    let layout = v.create_pango_layout(Some(text));
                    let mut font = gtk::pango::FontDescription::from_string(s(&style["font"]));
                    font.set_absolute_size(num(&style["fontSize"]) * gtk::pango::SCALE as f64);
                    layout.set_font_description(Some(&font));
                    super::motion::Entry {
                        id: s(&row["id"]).into(),
                        text: text.into(),
                        width: (layout.pixel_size().0 as f64 + 14.).max(32.),
                        layout,
                        color: gtk::gdk::RGBA::parse(s(&style["color"]))
                            .or_else(|_| gtk::gdk::RGBA::parse(s(&style["text"])))
                            .unwrap_or(gtk::gdk::RGBA::WHITE),
                        active: row["state"]["active"] == true || row["state"]["focused"] == true,
                    }
                })
                .collect();
            let style = config::merge(&self.base, &module["style"]);
            v.workspaces(
                entries,
                num(&style["itemGap"]),
                self.orientation == gtk::Orientation::Vertical,
                gtk::gdk::RGBA::parse(s(&style["accent"])).unwrap_or(gtk::gdk::RGBA::WHITE),
            );
        } else {
            while let Some(child) = self.root.first_child() {
                self.root.remove(&child);
            }
            self.root.append(&rows(
                module,
                &self.base,
                self.orientation,
                &mut self.text_cache,
                &mut self.icons,
            ));
        }
    }
    pub fn resize_frame(&self) {
        if let Some(t) = &self.tray {
            t.queue_resize();
        }
    }
    pub fn resizing(&self) -> bool {
        self.tray.as_ref().is_some_and(|t| t.moving())
    }
    pub fn tray_status(&self) -> Value {
        self.tray
            .as_ref()
            .map(|t| t.status())
            .unwrap_or(Value::Null)
    }
    pub fn icon_status(&self) -> Value {
        if let Some(t) = &self.tray {
            return t.icons();
        }
        Value::Array(
            self.icons
                .iter()
                .map(|(id, icon)| serde_json::json!({"id":id,"state":icon.status()}))
                .collect(),
        )
    }
    pub fn wake_icons(&self) {
        if let Some(t) = &self.tray {
            t.wake();
        }
        for icon in self.icons.values() {
            icon.wake();
        }
    }
    pub fn visuals(&self) -> Vec<super::motion::Visual> {
        fn visit(w: &gtk::Widget, result: &mut Vec<super::motion::Visual>) {
            if let Some(v) = w.downcast_ref::<super::motion::Visual>() {
                result.push(v.clone());
            }
            let mut child = w.first_child();
            while let Some(c) = child {
                child = c.next_sibling();
                visit(&c, result);
            }
        }
        let mut result = vec![];
        visit(self.root.upcast_ref(), &mut result);
        result
    }
}

fn detach_icon(icon: &super::icons::Icon) {
    if let Some(parent) = icon.root.parent().and_downcast::<gtk::Box>() {
        parent.remove(&icon.root);
    }
}
