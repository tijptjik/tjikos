//! Provider presentation. Discovery, actions and persistence stay in runtjik-core.
use gtk::{glib, prelude::*};
use serde_json::Value;
use std::rc::Rc;
fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().unwrap_or("")
}
fn label(value: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(value)
        .wrap(true)
        .xalign(0.)
        .selectable(true)
        .build()
}
pub fn select_word(root: &impl IsA<gtk::Widget>, selected: u32) {
    let mut child = root.first_child();
    while let Some(widget) = child {
        child = widget.next_sibling();
        if widget.has_css_class("word-sense") || widget.has_css_class("capture-choice") {
            if widget.widget_name() == format!("word-{selected}") {
                widget.add_css_class("word-selected");
            } else {
                widget.remove_css_class("word-selected");
            }
        }
        select_word(&widget, selected);
    }
}

pub fn words(
    rows: &[Value],
    query: &str,
    settings: &Value,
    counts: &Value,
    header: &gtk::Box,
    selection: &gtk::SingleSelection,
    hidden: Rc<std::cell::RefCell<std::collections::HashSet<String>>>,
    lookup: impl Fn(String) + 'static,
) -> gtk::Widget {
    while let Some(child) = header.first_child() {
        header.remove(&child);
    }
    let synonyms = query.starts_with("/synonym");
    let heading = label(if synonyms { "SYNONYMS" } else { "DEFINITIONS" });
    heading.add_css_class("word-caption");
    header.append(&heading);
    let root = gtk::Box::new(gtk::Orientation::Vertical, 8);
    root.set_vexpand(true);
    let content = gtk::Box::new(gtk::Orientation::Vertical, 8);
    content.set_margin_start(4);
    content.set_margin_end(12);
    root.append(&content);
    let lookup = Rc::new(lookup);
    let sources = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let source_scroll = gtk::ScrolledWindow::builder()
        .child(&sources)
        .hscrollbar_policy(gtk::PolicyType::External)
        .vscrollbar_policy(gtk::PolicyType::Never)
        .hexpand(true)
        .build();
    header.append(&source_scroll);
    let name = |key: &str| match key {
        "wiktionary" => "Wiktionary".to_owned(),
        "urban_dictionary" => "Urban Dictionary".to_owned(),
        "free_dictionary" => "Free Dictionary".to_owned(),
        "datamuse" => "Datamuse".to_owned(),
        _ => key.to_owned(),
    };
    let mut providers: Vec<String> = settings["plugin_settings"]["Words"][if synonyms {
        "synonym_sources"
    } else {
        "define_sources"
    }]
    .as_array()
    .into_iter()
    .flatten()
    .filter_map(Value::as_str)
    .map(name)
    .collect();
    for row in rows {
        let source = s(row, "word_source");
        if !source.is_empty() && !providers.iter().any(|p| p == source) {
            providers.push(source.into());
        }
    }
    let indicator = gtk::Box::new(gtk::Orientation::Vertical, 14);
    indicator.set_valign(gtk::Align::Center);
    indicator.set_halign(gtk::Align::Center);
    indicator.set_vexpand(true);
    let mark = label(if synonyms { "≈" } else { "Aa" });
    mark.add_css_class("word-watermark");
    mark.set_halign(gtk::Align::Center);
    indicator.append(&mark);
    let term = query
        .split_once(char::is_whitespace)
        .map(|(_, t)| t.trim())
        .unwrap_or("");
    let loading_title = format!("Looking up “{term}”…");
    let title = label(if term.is_empty() {
        if synonyms {
            "Find the right word"
        } else {
            "Look up a word"
        }
    } else {
        &loading_title
    });
    title.set_halign(gtk::Align::Center);
    title.add_css_class("muted");
    indicator.append(&title);
    let hint = label(if !term.is_empty() {
        "Results appear as each source responds."
    } else if synonyms {
        "Type a word above to explore its synonyms."
    } else {
        "Type a word above for definitions and examples."
    });
    hint.add_css_class("word-caption");
    hint.set_halign(gtk::Align::Center);
    indicator.append(&hint);
    root.append(&indicator);
    let empty =
        term.is_empty() || rows.is_empty() || rows.iter().all(|r| s(r, "id") == "Words:loading");
    content.set_visible(!empty);
    indicator.set_visible(empty);
    let mut groups: Vec<(String, gtk::Widget)> = vec![];
    let mut previous = Value::Null;
    if !empty {
        for (index, row) in rows.iter().enumerate() {
            if s(row, "id") == "Words:loading" {
                continue;
            }
            let source = s(row, "word_source");
            let section = s(row, "section");
            let group = gtk::Box::new(gtk::Orientation::Vertical, 10);
            let first_group = ["word_source", "language", "section", "group"]
                .iter()
                .any(|k| row[*k] != previous[*k]);
            if first_group && row["disabled"] != true {
                let top = gtk::Box::new(gtk::Orientation::Horizontal, 9);
                top.set_margin_start(16);
                top.set_margin_top(if index == 0 { 0 } else { 8 });
                let section_name = section
                    .split('_')
                    .map(|word| {
                        word.chars()
                            .next()
                            .map(|c| c.to_uppercase().to_string() + &word[c.len_utf8()..])
                            .unwrap_or_default()
                    })
                    .collect::<Vec<_>>()
                    .join(" ");
                let heading = if section == "definitions" || section.is_empty() {
                    if !s(row, "part_of_speech").is_empty() {
                        s(row, "part_of_speech")
                    } else if source == "Urban Dictionary" {
                        "Slang"
                    } else {
                        "Definitions"
                    }
                } else {
                    &section_name
                };
                let heading = heading
                    .chars()
                    .next()
                    .map(|c| c.to_uppercase().to_string() + &heading[c.len_utf8()..])
                    .unwrap_or_default();
                let title = label(&heading);
                title.add_css_class("word-heading");
                title.set_wrap(false);
                top.append(&title);
                if !s(row, "language").is_empty() {
                    let locale = match s(row, "language") {
                        "English" => "EN",
                        "French" => "FR",
                        "German" => "DE",
                        "Chinese" => "ZH",
                        "Japanese" => "JA",
                        other => other,
                    };
                    let badge = label(locale);
                    badge.add_css_class("word-locale");
                    badge.set_valign(gtk::Align::Center);
                    top.append(&badge);
                }
                let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                spacer.set_hexpand(true);
                top.append(&spacer);
                if !s(row, "url").is_empty() {
                    let link = gtk::LinkButton::with_label(s(row, "url"), "↗");
                    link.add_css_class("word-source");
                    top.append(&link);
                }
                group.append(&top);
                let meta = if section == "definitions" {
                    s(row, "headword")
                } else {
                    s(row, "part_of_speech")
                };
                if !meta.is_empty() {
                    let l = label(meta);
                    l.add_css_class("muted");
                    l.set_margin_start(16);
                    group.append(&l);
                }
            }
            let card = gtk::Box::new(gtk::Orientation::Horizontal, 12);
            card.add_css_class("word-sense");
            card.set_widget_name(&format!("word-{index}"));
            card.set_margin_start(row["sense_depth"].as_i64().unwrap_or(0).clamp(0, 4) as i32 * 18);
            let stripe = gtk::Box::new(gtk::Orientation::Vertical, 0);
            stripe.add_css_class("word-stripe");
            stripe.set_width_request(2);
            card.append(&stripe);
            if index as u32 == selection.selected() {
                card.add_css_class("word-selected");
            }
            if !s(row, "sense_number").is_empty() {
                let num = s(row, "sense_number");
                let n = label(&format!(
                    "{num}{}",
                    if num.contains('.') { "" } else { "." }
                ));
                n.add_css_class("muted");
                n.set_width_chars(3);
                n.set_xalign(1.);
                n.set_valign(gtk::Align::Start);
                card.append(&n);
            }
            let body = gtk::Box::new(gtk::Orientation::Vertical, 9);
            body.set_hexpand(true);
            if !s(row, "name").is_empty() {
                body.append(&label(s(row, "name")));
            }
            if !s(row, "context").is_empty() {
                let l = label(s(row, "context"));
                l.add_css_class("muted");
                body.append(&l);
            }
            for key in ["terms", "examples", "sense_synonyms"] {
                let values = row[key].as_array().cloned().unwrap_or_default();
                if values.is_empty() {
                    continue;
                }
                if key == "examples" {
                    for example in values {
                        if let Some(text) = example.as_str() {
                            let l = label(text);
                            l.add_css_class("word-example");
                            body.append(&l);
                        }
                    }
                    continue;
                }
                if key == "sense_synonyms" {
                    let l = label("Synonyms");
                    l.add_css_class("word-caption");
                    body.append(&l);
                }
                let flow = gtk::FlowBox::new();
                flow.set_selection_mode(gtk::SelectionMode::None);
                flow.set_homogeneous(false);
                flow.set_max_children_per_line(20);
                for term in values {
                    let text = term.as_str().unwrap_or_else(|| {
                        if !s(&term, "text").is_empty() {
                            s(&term, "text")
                        } else {
                            s(&term, "term")
                        }
                    });
                    if text.is_empty() {
                        continue;
                    }
                    let column = gtk::Box::new(gtk::Orientation::Vertical, 2);
                    let b = gtk::Button::with_label(text);
                    b.add_css_class("word-term");
                    let query = if !s(&term, "term").is_empty() {
                        s(&term, "term")
                    } else {
                        text
                    }
                    .to_owned();
                    let lookup = lookup.clone();
                    b.connect_clicked(move |_| lookup(query.clone()));
                    column.append(&b);
                    if !s(&term, "qualifier").is_empty() {
                        let l = label(s(&term, "qualifier"));
                        l.add_css_class("word-caption");
                        column.append(&l);
                    }
                    flow.insert(&column, -1);
                }
                body.append(&flow);
            }
            if row["disabled"] == true {
                let l = label(s(row, "subtitle"));
                l.add_css_class("muted");
                body.append(&l);
            }
            card.append(&body);
            let click = gtk::GestureClick::new();
            let select = selection.clone();
            click.connect_released(move |_, _, _, _| select.set_selected(index as u32));
            card.add_controller(click);
            group.append(&card);
            group.set_visible(!hidden.borrow().contains(source));
            content.append(&group);
            groups.push((source.into(), group.upcast()));
            previous = row.clone();
        }
    }
    let groups = Rc::new(groups);
    for provider in &providers {
        let toggle = gtk::ToggleButton::new();
        toggle.add_css_class("word-provider");
        toggle.set_active(!hidden.borrow().contains(provider));
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let icon_name = match provider.as_str() {
            "Wiktionary" => "wiktionary",
            "Urban Dictionary" => "urban-dictionary",
            _ => "dictionary",
        };
        let icon = gtk::Image::from_file(format!(
            "{}/qml/icons/providers/{icon_name}.svg",
            std::env::var("RUNTJIK_HOME").unwrap_or_default()
        ));
        icon.set_pixel_size(20);
        row.append(&icon);
        let provider_label = label(provider);
        provider_label.set_selectable(false);
        row.append(&provider_label);
        let count = counts[provider].as_u64().unwrap_or_else(|| {
            rows.iter()
                .filter(|r| s(r, "word_source") == provider && r["disabled"] != true)
                .count() as u64
        });
        let count = label(&count.to_string());
        count.add_css_class("accent");
        count.set_selectable(false);
        row.append(&count);
        toggle.set_child(Some(&row));
        let select = selection.clone();
        let row_sources: Vec<String> = rows
            .iter()
            .map(|r| s(r, "word_source").to_owned())
            .collect();
        let name = provider.clone();
        let hidden = hidden.clone();
        let groups = groups.clone();
        let providers = providers.clone();
        let indicator = indicator.clone();
        let title = title.clone();
        let hint = hint.clone();
        toggle.connect_toggled(move |t| {
            if t.is_active() {
                hidden.borrow_mut().remove(&name);
            } else {
                hidden.borrow_mut().insert(name.clone());
            }
            if row_sources
                .get(select.selected() as usize)
                .is_some_and(|s| hidden.borrow().contains(s))
            {
                if let Some(at) = row_sources
                    .iter()
                    .position(|s| !hidden.borrow().contains(s))
                {
                    select.set_selected(at as u32);
                }
            }
            for (source, group) in groups.iter() {
                group.set_visible(!hidden.borrow().contains(source));
            }
            let all_hidden = providers.iter().all(|p| hidden.borrow().contains(p));
            indicator.set_visible(empty || all_hidden);
            if all_hidden {
                title.set_text("Sources hidden");
                hint.set_text("Choose a source above to show its results.");
            } else {
                title.set_text(if empty {
                    "Look up a word"
                } else {
                    "Looking up your word…"
                });
                hint.set_text("Type a word above for definitions and examples.");
            }
        });
        sources.append(&toggle);
    }
    if !providers.is_empty() && providers.iter().all(|p| hidden.borrow().contains(p)) {
        indicator.set_visible(true);
        title.set_text("Sources hidden");
        hint.set_text("Choose a source above to show its results.");
    }
    root.upcast()
}
pub fn stats(value: &Value, theme: &tjik_gtk::theme::Theme, indexed: u64) -> gtk::Widget {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 16);
    root.set_margin_top(12);
    root.set_margin_start(12);
    root.set_margin_end(12);
    let title = label(&format!("Launcher activity · {indexed} indexed"));
    title.add_css_class("accent");
    root.append(&title);
    let grid = gtk::Grid::new();
    grid.set_column_spacing(4);
    grid.set_row_spacing(4);
    let days = value["calendar"].as_array().cloned().unwrap_or_default();
    let peak = days
        .iter()
        .filter_map(|d| d["count"].as_u64())
        .max()
        .unwrap_or(1)
        .max(1);
    for (i, day) in days
        .iter()
        .rev()
        .take(26 * 7)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .enumerate()
    {
        let cell = gtk::Label::new(Some("  "));
        cell.set_size_request(16, 16);
        let count = day["count"].as_u64().unwrap_or(0);
        let color = if count > 0 {
            &theme.accent
        } else {
            &theme.surface
        };
        cell.set_markup(&format!(
            "<span background=\"{}\">  </span>",
            glib::markup_escape_text(color)
        ));
        cell.set_opacity(if count > 0 {
            0.28 + 0.72 * (count as f64 / peak as f64)
        } else {
            1.
        });
        cell.set_tooltip_text(Some(&format!("{} · {count} opens", s(day, "date"))));
        grid.attach(&cell, (i / 7) as i32, (i % 7) as i32, 1, 1);
    }
    root.append(&grid);
    for metric in value["metrics"].as_array().into_iter().flatten() {
        root.append(&label(&format!(
            "{}     {} total     {:.1} / day",
            s(metric, "label"),
            metric["total"],
            metric["average"].as_f64().unwrap_or(0.)
        )));
    }
    root.append(&label("Daily averages include idle days."));
    root.upcast()
}

/// Synthetic UI diagnostics use actual allocated widgets, never private content.
pub fn inspect(
    header: &gtk::Box,
    rich: &impl IsA<gtk::Widget>,
    window: &gtk::ApplicationWindow,
) -> Value {
    fn walk(widget: &gtk::Widget, window: &gtk::ApplicationWindow, output: &mut Vec<Value>) {
        if widget.has_css_class("word-provider")
            || widget.has_css_class("word-sense")
            || widget.has_css_class("capture-choice")
        {
            let bounds = widget.compute_bounds(window);
            output.push(serde_json::json!({"class": if widget.has_css_class("word-provider") {"provider"} else {"sense"}, "name": widget.widget_name().as_str(), "visible": widget.is_mapped(), "active": widget.downcast_ref::<gtk::ToggleButton>().map(|t|t.is_active()), "x": bounds.as_ref().map(|b|b.x()), "y": bounds.as_ref().map(|b|b.y()), "width": bounds.as_ref().map(|b|b.width()), "height": bounds.as_ref().map(|b|b.height())}));
        }
        let mut child = widget.first_child();
        while let Some(w) = child {
            child = w.next_sibling();
            walk(&w, window, output);
        }
    }
    let mut widgets = vec![];
    walk(header.upcast_ref(), window, &mut widgets);
    walk(rich.as_ref(), window, &mut widgets);
    serde_json::json!(widgets)
}

pub fn reveal_word(scroll: &gtk::ScrolledWindow, selected: u32) {
    fn find(widget: &gtk::Widget, name: &str) -> Option<gtk::Widget> {
        if widget.widget_name() == name {
            return Some(widget.clone());
        }
        let mut child = widget.first_child();
        while let Some(w) = child {
            child = w.next_sibling();
            if let Some(found) = find(&w, name) {
                return Some(found);
            }
        }
        None
    }
    if let Some(widget) = find(scroll.upcast_ref(), &format!("word-{selected}")) {
        if let Some(bounds) = widget.compute_bounds(scroll) {
            let a = scroll.vadjustment();
            let delta = if bounds.y() < 0. {
                bounds.y()
            } else if bounds.y() + bounds.height() > scroll.height() as f32 {
                bounds
                    .y()
                    .min(bounds.y() + bounds.height() - scroll.height() as f32)
            } else {
                0.
            };
            a.set_value(
                (a.value() + delta as f64)
                    .clamp(a.lower(), (a.upper() - a.page_size()).max(a.lower())),
            );
        }
    }
}

pub fn capture(rows: &[Value], selected: u32, activate: impl Fn(u32) + 'static) -> gtk::Grid {
    let grid = gtk::Grid::new();
    grid.set_row_homogeneous(true);
    grid.set_column_homogeneous(true);
    grid.set_row_spacing(6);
    grid.set_column_spacing(6);
    grid.set_vexpand(true);
    let activate = Rc::new(activate);
    let choices = rows
        .iter()
        .position(|r| s(r, "id").ends_with(":history"))
        .unwrap_or(rows.len());
    let recording = choices >= 4;
    let cols = if recording { 2 } else { 3 };
    for (i, row) in rows.iter().enumerate() {
        let card = gtk::Box::new(gtk::Orientation::Vertical, 6);
        card.add_css_class("capture-choice");
        card.set_widget_name(&format!("word-{i}"));
        card.set_hexpand(true);
        card.set_vexpand(true);
        if i as u32 == selected {
            card.add_css_class("word-selected");
        }
        let content = gtk::Box::new(gtk::Orientation::Vertical, 6);
        content.set_valign(gtk::Align::Center);
        content.set_vexpand(true);
        let icon = gtk::Image::new();
        icon.set_pixel_size(40);
        tjik_gtk::images::set(&icon, s(row, "icon"), "camera-photo");
        content.append(&icon);
        let title = label(s(row, "name"));
        title.set_selectable(false);
        title.set_halign(gtk::Align::Center);
        content.append(&title);
        let subtitle = label(s(row, "subtitle"));
        subtitle.set_selectable(false);
        subtitle.set_justify(gtk::Justification::Center);
        subtitle.set_xalign(0.5);
        subtitle.add_css_class("word-caption");
        content.append(&subtitle);
        card.append(&content);
        let click = gtk::GestureClick::new();
        let activate = activate.clone();
        click.connect_released(move |_, _, _, _| activate(i as u32));
        card.add_controller(click);
        if i >= choices {
            let wide = rows.len() == choices + 1;
            grid.attach(
                &card,
                if wide { 0 } else { (i - choices) as i32 },
                if recording { 2 } else { 1 },
                if wide { cols } else { 1 },
                if recording { 2 } else { 1 },
            );
        } else {
            grid.attach(&card, i as i32 % cols, i as i32 / cols, 1, 1);
        }
    }
    grid
}
