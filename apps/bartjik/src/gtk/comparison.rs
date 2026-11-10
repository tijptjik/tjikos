//! Shared bar presentation for native providers and the read-only comparison bridge.
use crate::{
    config,
    util::{arr, num, s},
};
use serde_json::Value;
use std::collections::BTreeMap;
use tjik_gtk::gtk::{self, prelude::*};
/// hover cards/previews replace the raw provider tooltip.
fn tooltip<'a>(module: &Value, row: &'a Value) -> Option<&'a str> {
    let hover_action = ["item", "icon", "text", "artist", "track"]
        .iter()
        .any(|part| {
            [
                "audio.panel",
                "network.panel",
                "nightlight.panel",
                "monitor.panel",
                "idle.panel",
            ]
            .contains(&s(&crate::action_choice::choose(
                module,
                row,
                part,
                "hoverEnter",
            )))
        });
    if module["provider"] == "crypto"
        || module["panel"]["hover"] == true
        || (module["provider"] == "workspaces" && module["preview"]["enabled"] == true)
        || hover_action
    {
        None
    } else {
        row["tooltip"].as_str()
    }
}
fn rows(
    module: &Value,
    base: &Value,
    orientation: gtk::Orientation,
    cache: &mut BTreeMap<String, super::motion::Visual>,
    icons: &mut BTreeMap<String, super::icons::Icon>,
    bindings: &mut BTreeMap<String, crate::input::Binding>,
    sender: &async_channel::Sender<crate::Event>,
    group: &gtk::Box,
) {
    let mut previous = std::mem::take(cache);
    let mut previous_icons = std::mem::take(icons);
    let style = config::merge(base, &module["style"]);
    group.set_spacing(num(&style["itemGap"]) as i32);
    let mut live_rows = vec![];
    let mut live_keys = vec![];
    let mut prior_row: Option<gtk::Widget> = None;
    for row in arr(&module["comparisonItems"])
        .iter()
        .filter(|r| r["visible"] != false)
    {
        let id = s(&row["id"]);
        let mut existing = group.first_child();
        let mut found = None;
        while let Some(child) = existing {
            existing = child.next_sibling();
            if child.widget_name() == id {
                found = child.downcast::<gtk::Box>().ok();
                break;
            }
        }
        let item = found.unwrap_or_else(|| {
            let b = gtk::Box::new(orientation, 0);
            b.set_widget_name(id);
            group.append(&b);
            b
        });
        group.reorder_child_after(&item, prior_row.as_ref());
        prior_row = Some(item.clone().upcast());
        live_rows.push(item.clone().upcast::<gtk::Widget>());
        item.set_spacing(num(&style["iconTextGap"]) as i32);
        item.set_tooltip_text(tooltip(module, row));
        let media =
            module["provider"] == "playerctl" && row["idle"] != true && row["artist"].is_object();
        let kinds = if media {
            vec!["icon", "artist", "separator", "track"]
        } else {
            vec!["icon", "text"]
        };
        let mut live_parts = vec![];
        let mut prior_part: Option<gtk::Widget> = None;
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
            let size = num(&style[if kind == "icon" {
                "iconSize"
            } else {
                "fontSize"
            }])
            .max(1.);
            let value = s(&part["value"]);
            let key = format!("{id}/{kind}");
            let widget: gtk::Widget = if kind == "icon"
                && ["image", "theme", "symbol", "pixmap"].contains(&s(&part["kind"]))
            {
                let icon = previous_icons
                    .remove(id)
                    .unwrap_or_else(super::icons::Icon::new);
                icon.update(
                    part,
                    &style,
                    false,
                    module["comparisonTailnet"] == true,
                    false,
                    false,
                );
                let w = icon.root.clone().upcast();
                icons.insert(id.into(), icon);
                w
            } else {
                let visual = previous
                    .remove(&key)
                    .unwrap_or_else(super::motion::Visual::new);
                visual.set_text(
                    value,
                    s(&style["font"]),
                    size,
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
                cache.insert(key.clone(), visual.clone());
                visual.upcast()
            };
            widget.set_valign(gtk::Align::Center);
            widget.set_opacity(if row["enabled"] == false { 0.5 } else { 1. });
            if widget.parent().as_ref() != Some(item.upcast_ref()) {
                if let Some(parent) = widget.parent().and_downcast::<gtk::Box>() {
                    parent.remove(&widget);
                }
                item.append(&widget);
            }
            item.reorder_child_after(&widget, prior_part.as_ref());
            prior_part = Some(widget.clone());
            live_parts.push(widget.clone());
            if kind != "separator" {
                bind(
                    bindings,
                    &key,
                    &widget,
                    crate::input::payload(module, row, kind),
                    sender,
                );
                if let Some(binding) = bindings.get(&key) {
                    let mut hovered = row.clone();
                    hovered["state"]["hover"] = serde_json::json!(true);
                    let hover_style = part_style(module, base, &hovered, kind);
                    let normal = style.clone();
                    if let Some(icon) = icons.get(id).filter(|_| kind == "icon").cloned() {
                        let part = part.clone();
                        let tailnet = module["comparisonTailnet"] == true;
                        binding.appearance(std::rc::Rc::new(move |hover| {
                            icon.update(
                                &part,
                                if hover { &hover_style } else { &normal },
                                false,
                                tailnet,
                                false,
                                false,
                            )
                        }));
                    } else if let Some(visual) = cache.get(&key).cloned() {
                        let text = value.to_owned();
                        binding.appearance(std::rc::Rc::new(move |hover| {
                            let style = if hover { &hover_style } else { &normal };
                            visual.set_text(
                                &text,
                                s(&style["font"]),
                                num(&style["fontSize"]).max(1.),
                                gtk::gdk::RGBA::parse(s(&style["color"]))
                                    .or_else(|_| gtk::gdk::RGBA::parse(s(&style["text"])))
                                    .unwrap_or(gtk::gdk::RGBA::WHITE),
                                style["maxTextWidth"].as_f64().unwrap_or(10000.),
                                num(&style["minItemSize"]),
                                20.,
                                style["marquee"] == true,
                            );
                        }));
                    }
                }
                widget.update_property(&[gtk::accessible::Property::Label(if kind == "icon" {
                    row["tooltip"]
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .unwrap_or(value)
                } else {
                    value
                })]);
                live_keys.push(key);
            }
        }
        let mut child = item.first_child();
        while let Some(w) = child {
            child = w.next_sibling();
            if !live_parts.contains(&w) {
                item.remove(&w);
            }
        }
    }
    let mut child = group.first_child();
    while let Some(w) = child {
        child = w.next_sibling();
        if !live_rows.contains(&w) {
            group.remove(&w);
        }
    }
    bindings.retain(|key, _| live_keys.contains(key));
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
    bindings: BTreeMap<String, crate::input::Binding>,
    sender: async_channel::Sender<crate::Event>,
}
impl Module {
    pub fn new(
        module: &Value,
        base: &Value,
        orientation: gtk::Orientation,
        sender: async_channel::Sender<crate::Event>,
    ) -> Self {
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
            bindings: Default::default(),
            sender,
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
            for (id, widget) in tray.widgets() {
                if let Some(row) = arr(&module["comparisonItems"])
                    .iter()
                    .find(|r| r["id"] == id)
                {
                    widget.set_tooltip_text(tooltip(module, row));
                    bind(
                        &mut self.bindings,
                        &format!("{id}/icon"),
                        &widget,
                        crate::input::payload(module, row, "icon"),
                        &self.sender,
                    );
                }
            }
            self.bindings.retain(|key, _| {
                arr(&module["comparisonItems"])
                    .iter()
                    .any(|r| key == &format!("{}/icon", s(&r["id"])))
            });
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
            let module = module.clone();
            let visual = v.clone();
            let resolve: crate::input::Resolve = std::rc::Rc::new(move |x, y| {
                let id = visual.hit(x, y);
                let row = arr(&module["comparisonItems"])
                    .iter()
                    .find(|r| r["id"] == id)
                    .cloned()
                    .unwrap_or(Value::Null);
                crate::input::payload(&module, &row, "text")
            });
            if let Some(binding) = self.bindings.get("__workspaces") {
                binding.update(resolve);
            } else {
                self.bindings.insert(
                    "__workspaces".into(),
                    crate::input::Binding::new(v.upcast_ref(), resolve, self.sender.clone()),
                );
            }
        } else {
            let group = self
                .root
                .first_child()
                .and_downcast::<gtk::Box>()
                .unwrap_or_else(|| {
                    let g = gtk::Box::new(self.orientation, 0);
                    self.root.append(&g);
                    g
                });
            rows(
                module,
                &self.base,
                self.orientation,
                &mut self.text_cache,
                &mut self.icons,
                &mut self.bindings,
                &self.sender,
                &group,
            );
        }
    }
    pub fn hit_regions(&self, window: &gtk::Widget) -> Vec<(String, tjik_desktop::Rect)> {
        let bounds = |w: &gtk::Widget| {
            w.compute_bounds(window).map(|r| tjik_desktop::Rect {
                x: r.x() as f64,
                y: r.y() as f64,
                width: r.width() as f64,
                height: r.height() as f64,
            })
        };
        if !self.root.is_visible() {
            return vec![];
        }
        if let Some(ring) = &self.ring {
            let Some(origin) = bounds(ring.upcast_ref()) else {
                return vec![];
            };
            return ring
                .regions()
                .into_iter()
                .map(|(id, mut r)| {
                    r.x += origin.x;
                    r.y += origin.y;
                    (id, r)
                })
                .collect();
        }
        if let Some(tray) = &self.tray {
            return tray
                .widgets()
                .into_iter()
                .filter_map(|(id, w)| Some((id, bounds(&w)?)))
                .collect();
        }
        let mut result = vec![];
        if let Some(group) = self.root.first_child() {
            let mut child = group.first_child();
            while let Some(w) = child {
                child = w.next_sibling();
                if let Some(r) = bounds(&w) {
                    result.push((w.widget_name().to_string(), r));
                }
            }
        }
        result
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

fn bind(
    bindings: &mut BTreeMap<String, crate::input::Binding>,
    key: &str,
    widget: &gtk::Widget,
    payload: Value,
    sender: &async_channel::Sender<crate::Event>,
) {
    let weak = widget.downgrade();
    let resolve: crate::input::Resolve = std::rc::Rc::new(move |_, y| {
        if payload["context"]["part"] == "icon"
            && payload["module"]["comparisonTailnetInput"].is_object()
            && weak.upgrade().is_some_and(|w| y >= (w.height() - 7) as f64)
        {
            payload["module"]["comparisonTailnetInput"].clone()
        } else {
            payload.clone()
        }
    });
    if let Some(binding) = bindings.get(key).filter(|b| b.same(widget)) {
        binding.update(resolve);
    } else {
        bindings.insert(
            key.into(),
            crate::input::Binding::new(widget, resolve, sender.clone()),
        );
    }
}

#[cfg(test)]
mod tooltip_tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn panels_replace_tooltips_but_plain_tray_items_keep_them() {
        let row = json!({"tooltip":"account table"});
        assert_eq!(
            tooltip(&json!({"provider":"codex","panel":{"hover":true}}), &row),
            None
        );
        assert_eq!(
            tooltip(
                &json!({"provider":"workspaces","preview":{"enabled":true}}),
                &row
            ),
            None
        );
        assert_eq!(
            tooltip(
                &json!({"provider":"audio","actions":{"hoverEnter":"audio.panel"}}),
                &row
            ),
            None
        );
        assert_eq!(
            tooltip(&json!({"provider":"tray"}), &row),
            Some("account table")
        );
    }
}
