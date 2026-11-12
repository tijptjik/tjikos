//! Shared attached surface and layout primitives. Provider content owns no placement policy.
use crate::{
    panels::{self, Target},
    util::{arr, s},
    Event,
};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use serde_json::{json, Value};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Instant,
};
use tjik_desktop::Rect;
use tjik_gtk::gtk::{self, gdk, glib, prelude::*};

/// Original PanelHeader/PanelButton artwork, shared by attached cards and fixtures.
fn artwork(provider: &str, size: i32) -> gtk::Widget {
    let (kind, value) = match provider {
        "mullvad" => ("image", "icons/tray/mullvad.svg"),
        "tailscale" => ("image", "icons/tailscale.svg"),
        "codex" => ("image", "icons/codex.svg"),
        "openrouter" => ("image", "icons/openrouter.svg"),
        "audio" => ("image", "icons/proicons/volume.svg"),
        "network" => ("image", "icons/proicons/wi-fi.svg"),
        "nightlight" => ("image", "icons/proicons/moon.svg"),
        "battery" => ("image", "icons/battery.svg"),
        "DND" => ("image", "Notifications/history-bell.svg"),
        "datetime" => ("symbol", "schedule"),
        "crypto" => ("symbol", "show_chart"),
        "playerctl" => ("symbol", "music_note"),
        "workspaces" => ("symbol", "dashboard"),
        "locale" => ("symbol", "keyboard"),
        "caffeine" => ("symbol", "coffee"),
        "monitor-layout" => ("symbol", "desktop_windows"),
        "pin" => ("symbol", "push_pin"),
        "close" => ("symbol", "close"),
        _ => ("symbol", "info"),
    };
    let widget: gtk::Widget = if kind == "image" {
        let image = gtk::Image::from_file(crate::util::root().join("assets").join(value));
        image.set_pixel_size(size);
        image.upcast()
    } else {
        let label = gtk::Label::new(Some(value));
        label.add_css_class("panel-symbol");
        let attrs = gtk::pango::AttrList::new();
        attrs.insert(gtk::pango::AttrSize::new_size_absolute(
            size * gtk::pango::SCALE,
        ));
        label.set_attributes(Some(&attrs));
        label.upcast()
    };
    widget.set_size_request(size, size);
    widget.set_valign(gtk::Align::Center);
    widget
}
fn card_header(title: &str, provider: &str) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row.set_height_request(32);
    row.append(&artwork(provider, 22));
    let title = heading(title, "");
    title.set_hexpand(true);
    title.set_valign(gtk::Align::Center);
    row.append(&title);
    row
}
/// Surface-backed world-clock row: identical hierarchy and metrics to StatusDetails.
pub fn clock_row(label: &str, value: &str, detail: &str) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    row.add_css_class("panel-clock-row");
    let names = gtk::Box::new(gtk::Orientation::Vertical, 4);
    names.set_hexpand(true);
    names.set_valign(gtk::Align::Center);
    for (text, class) in [(label, "panel-clock-label"), (detail, "panel-clock-detail")] {
        let label = gtk::Label::new(Some(text));
        label.set_xalign(0.);
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        label.add_css_class(class);
        names.append(&label);
    }
    let time = gtk::Label::new(Some(value));
    time.add_css_class("panel-clock-value");
    time.set_valign(gtk::Align::Center);
    row.append(&names);
    row.append(&time);
    row
}
/// Shared icon content: text-row alignment must never leak into button glyphs.
/// GTK label alignment is a widget property, not a CSS text-align property.
pub fn control_symbol(text: &str) -> gtk::Label {
    let glyph = gtk::Label::new(Some(text));
    glyph.add_css_class("panel-symbol");
    glyph.add_css_class("panel-control-symbol");
    glyph.set_xalign(0.5);
    glyph.set_yalign(0.5);
    glyph.set_halign(gtk::Align::Center);
    glyph.set_valign(gtk::Align::Center);
    glyph
}
/// The original workspace frame uses concave joins, not a floating rounded button.
fn workspace_shape(style: &Value, width: i32, close: bool, top: i32, right: i32) -> gtk::Overlay {
    let background =
        gdk::RGBA::parse(s(&style["surface"])).unwrap_or(gdk::RGBA::new(0.149, 0.137, 0.227, 1.));
    let canvas = gtk::DrawingArea::new();
    canvas.set_content_width(if close { 40 + right } else { width + 16 });
    canvas.set_content_height(36 + top);
    canvas.set_can_target(false);
    canvas.set_draw_func(move |_, cr, _, _| {
        let q = |cx: f64, cy: f64, x: f64, y: f64| {
            let (px, py) = cr.current_point().unwrap();
            cr.curve_to(
                px + (cx - px) * 2. / 3.,
                py + (cy - py) * 2. / 3.,
                x + (cx - x) * 2. / 3.,
                y + (cy - y) * 2. / 3.,
                x,
                y,
            );
        };
        if close {
            cr.translate(0., top as f64);
            cr.move_to(0., 0.);
            q(8., 0., 8., 8.);
            cr.line_to(8., 20.);
            q(8., 28., 16., 28.);
            cr.line_to(32., 28.);
            q(40., 28., 40., 36.);
            // Join through the whole top/right gutter. No separate paint edge
            // remains where a fractional preview allocation could show through.
            cr.line_to(40. + right as f64, 36.);
            cr.line_to(40. + right as f64, -(top as f64));
            cr.line_to(0., -(top as f64));
        } else {
            let w = width as f64;
            cr.move_to(0., 36.);
            q(8., 36., 8., 28.);
            cr.line_to(8., 10.);
            q(8., 0., 18., 0.);
            cr.line_to(w - 2., 0.);
            q(w + 8., 0., w + 8., 10.);
            cr.line_to(w + 8., 28.);
            q(w + 8., 36., w + 16., 36.);
        }
        cr.close_path();
        cr.set_source_rgba(
            background.red() as f64,
            background.green() as f64,
            background.blue() as f64,
            background.alpha() as f64,
        );
        let _ = cr.fill();
    });
    let overlay = gtk::Overlay::new();
    overlay.set_child(Some(&canvas));
    overlay
}
pub(crate) fn workspace_notch(style: &Value, width: i32) -> gtk::Overlay {
    let notch = workspace_shape(style, width, false, 0, 0);
    notch.set_widget_name("workspace-notch");
    notch
}
fn workspace_close_art(style: &Value, top: i32, right: i32) -> gtk::Overlay {
    let overlay = workspace_shape(style, 32, true, top, right);
    let glyph = control_symbol("close");
    glyph.set_size_request(32, 28);
    glyph.set_margin_top(top);
    glyph.set_margin_end(right);
    glyph.set_halign(gtk::Align::End);
    glyph.set_valign(gtk::Align::Start);
    glyph.set_can_target(false);
    overlay.add_overlay(&glyph);
    overlay
}
fn stylesheet(style: &Value) -> String {
    let color = |key: &str, fallback: &str| {
        gdk::RGBA::parse(s(&style[key]))
            .unwrap_or_else(|_| gdk::RGBA::parse(fallback).unwrap())
            .to_string()
    };
    let font = style["font"]
        .as_str()
        .unwrap_or("sans-serif")
        .replace(['\\', '"', '\n', '\r'], " ");
    let border = if s(&style["panelBorder"]).is_empty() {
        "border"
    } else {
        "panelBorder"
    };
    format!(
        r#"
.panel-card {{background:{background};color:{text};border:1px solid {border};border-radius:12px;padding:15px;font-family:"{font}";font-size:13px;}}
.panel-card.panel-edge-content {{padding:0;}}
popover.workspace-tooltip {{font-family:"{font}";font-size:12px;}}
popover.workspace-tooltip contents {{background:{background};color:{text};padding:0;margin:0;border:1px solid {border};border-radius:8px;box-shadow:none;min-height:0;}}
popover.workspace-tooltip label.workspace-tooltip-title {{font-family:"{font}";font-size:12px;font-weight:normal;color:{text};}}
.panel-card .music-controls {{padding:0 12px;background:{surface};}}
.panel-card .music-controls button.music-button,
.panel-card .music-controls button.music-button:hover,
.panel-card .music-controls button.music-button:active,
.panel-card .music-controls button.music-button:focus-visible {{padding:0;min-width:0;min-height:0;margin:0;border:0;outline:none;box-shadow:none;background-image:none;border-radius:8px;color:{text};background:transparent;}}
.panel-card .music-controls button.music-button:hover,
.panel-card .music-controls button.music-button:focus-visible {{background:rgba(255,255,255,0.1);}}
.panel-card .music-controls button.music-button:active {{background:rgba(255,255,255,0.15);}}
.panel-card .music-controls button.music-button:disabled {{opacity:0.35;}}
.panel-card .music-controls .music-button .panel-control-symbol {{font-size:18px;}}
.panel-card .music-controls .music-transport .panel-control-symbol {{font-size:24px;}}
.panel-card .lyrics-placeholder {{color:{muted};opacity:0.2;}}
.panel-card .lyrics-placeholder .panel-control-symbol {{font-size:96px;color:{muted};}}
.panel-card .lyric-current {{color:{accent};font-weight:bold;}}
.panel-card.panel-reveal-content {{background:transparent;border-color:transparent;}}
.panel-card .panel-symbol {{font-family:"Material Symbols Rounded";}}
.panel-card scale.audio-scale {{padding:0;margin:0;background:transparent;}}
.panel-card scale.audio-scale trough, .panel-card scale.audio-scale highlight {{background:transparent;border:0;box-shadow:none;min-height:16px;min-width:0;margin:0;padding:0;}}
.panel-card scale.audio-scale slider {{min-width:16px;min-height:26px;margin:0;border:0;background:transparent;box-shadow:none;}}
.panel-card button.audio-route {{min-height:46px;padding:0;border:1px solid transparent;border-radius:8px;background:{surface};box-shadow:none;color:{text};}}
.panel-card button.audio-route .panel-control-symbol {{font-size:26px;}}
.panel-card button.audio-route .network-title {{font-size:13px;}}
.panel-card button.audio-route-active {{background:alpha({accent},0.22);border-color:{accent};color:{accent};}}
.panel-card .vpn-country {{font-size:15px;}}
.panel-card .vpn-city {{font-size:12px;color:{muted};}}
.panel-card .vpn-peer {{min-height:58px;padding:0 4px 0 8px;border-radius:8px;}}
.panel-card .vpn-peer:hover {{background:{surface};}}
.panel-card .vpn-online {{font-size:10px;color:{accent};}}
.panel-card button.vpn-city-option {{min-height:16px;padding:8px;border:0;border-radius:6px;background:transparent;box-shadow:none;}}
.panel-card button.vpn-city-option:hover {{background:{background};}}
.panel-card button.vpn-flag {{min-width:0;min-height:46px;padding:2px;border:1px solid {border};border-radius:3px;background:{surface};box-shadow:none;}}
.panel-card button.vpn-flag:hover {{border-color:{accent};}}
.panel-card .network-label {{font-size:13px;}}
.panel-card .network-title {{font-size:14px;font-weight:600;}}
.panel-card .network-muted {{font-size:11px;color:{muted};}}
.panel-card .network-address-name {{font-size:13px;color:{muted};min-height:16px;}}
.panel-card .network-symbol, .panel-card .panel-control-symbol {{font-size:20px;}}
.panel-card .network-accent-symbol {{color:{accent};}}
.panel-card .network-connection {{padding:12px;background:{surface};border-radius:10px;}}
.panel-card .network-connection separator {{background:alpha({border},0.45);}}
.panel-card .network-form {{padding:12px;background:{surface};border-radius:8px;}}
.panel-card .network-selected {{color:{accent};font-size:13px;}}
.panel-card button.network-row {{padding:12px;min-height:0px;border:0;border-radius:8px;background:transparent;box-shadow:none;color:{text};}}
.panel-card button.network-row:hover {{background:{surface};}}
.panel-card button.network-row:focus-visible {{outline:1px solid {accent};outline-offset:-1px;}}
.panel-card button.panel-control.network-accent {{color:{accent};}}
.panel-card passwordentry {{padding:10px;background:{background};color:{text};border:1px solid {border};border-radius:8px;font-size:12px;}}
.panel-card passwordentry.network-password {{padding-right:42px;}}
.panel-card button.panel-control.network-password-submit {{background:transparent;border:0;}}
.panel-card passwordentry:focus-within {{border-color:{accent};}}
.panel-card .router-muted {{font-family:sans-serif;font-size:12px;color:{muted};}}
.panel-card .router-balance {{font-size:32px;}}
.panel-card .router-caption {{font-family:sans-serif;font-size:10px;color:{muted};}}
.panel-card .router-date {{font-size:13px;color:{muted};}}
.panel-card .router-tokens {{font-size:14px;}}
.panel-card separator.router-divider {{background:rgba(255,255,255,0.09);}}
.panel-card .codex-caption {{font-family:sans-serif;font-size:11px;color:{muted};}}
.panel-card .codex-muted {{font-family:sans-serif;font-size:11px;color:{muted};}}
.panel-card .codex-account {{font-size:15px;font-weight:600;}}
.panel-card .codex-plan {{font-family:sans-serif;font-size:10px;color:{muted};}}
.panel-card .codex-percent {{font-size:15px;}}
.panel-card .codex-reset {{font-size:14px;}}
.panel-card .codex-stat {{font-size:12px;}}
.panel-card .panel-heading {{font-weight:600;font-size:16px;}}
.panel-card .panel-muted {{color:{muted};font-size:11px;}}
.panel-card .panel-row {{padding:7px 0;}}
.panel-card separator {{min-height:1px;background:{surface};border:0;padding:0;}}
.panel-card .panel-clock-row {{min-height:56px;padding:0 12px;border-radius:8px;background:{surface};}}
.panel-card .panel-clock-label {{font-size:13px;}}
.panel-card .panel-clock-detail {{font-size:10px;color:{muted};}}
.panel-card .panel-clock-value {{font-size:23px;font-weight:400;}}
.panel-card button.panel-control {{min-width:30px;min-height:30px;padding:0;margin:0;border:1px solid transparent;border-radius:8px;background:{surface};box-shadow:none;color:{text};}}
.panel-card button.panel-control:hover {{border-color:{border};background:mix({surface},white,0.04);}}
.panel-card button.panel-control:active {{border-color:{accent};background:mix({surface},white,0.09);}}
.panel-card button.panel-control:checked {{border-color:{accent};}}
.panel-card button.panel-control:focus-visible {{outline:1px solid {accent};outline-offset:2px;}}
.panel-card button.panel-close:hover {{color:{accent};border-color:transparent;background:{surface};}}
.panel-card button.input-method {{padding:10px;min-height:34px;border:0;border-radius:8px;background:transparent;box-shadow:none;}}
.panel-card button.input-method:hover, .panel-card button.input-method.current {{background:{surface};}}
.panel-card .input-badge {{min-width:34px;min-height:32px;border-radius:7px;background:{background};font-size:15px;}}
.panel-card .input-name {{font-size:12px;}}
.panel-card .input-caption {{font-size:10px;color:{muted};}}
.panel-card .input-check {{color:{accent};}}
.panel-card .history-search {{background:{surface};color:{text};border:1px solid {border};border-radius:6px;padding:0 8px;min-height:32px;box-shadow:none;}}
.panel-card .history-search:focus-within {{border-color:{accent};}}
.panel-card .history-search text {{background:transparent;color:{text};padding:0;min-height:0;}}
.panel-card .notjik .card-frame {{border:0;}}
.panel-card .history-group {{padding:12px;border-radius:8px;background:{surface};border:0;box-shadow:none;}}
.panel-card .history-notice {{padding:10px;border-radius:8px;background:transparent;border:0;box-shadow:none;}}
.panel-card .history-notice:hover, .panel-card .history-detail {{background:{surface};border-radius:8px;}}
.panel-card .history-detail {{padding:10px;}}
.panel-card .history-title {{font-weight:600;}}
.panel-card .left-value {{font-size:16px;}}
.panel-card .left-art-empty {{font-size:64px;}}
.panel-card .left-lyrics {{padding:16px;font-size:14px;}}
.panel-card .left-lyrics.lyric-line {{padding:0;font-size:16px;}}
.panel-card button.workspace-panel-close, .panel-card button.workspace-panel-close:hover, .panel-card button.workspace-panel-close:active, .panel-card button.workspace-panel-close:focus, .panel-card button.workspace-panel-close:focus-visible {{outline:none;outline-offset:0;background-image:none;padding:0;min-width:0;min-height:0;border:0;border-radius:0;background:transparent;box-shadow:none;}}
.panel-card button.workspace-panel-close:hover, .panel-card button.workspace-panel-close:focus-visible {{color:{accent};outline:none;box-shadow:none;}}
.panel-card .workspace-scene {{background:{surface};}}
.panel-card .workspace-desktop {{background:{background};border-radius:4px;}}
.panel-card .workspace-overflow {{padding:0;background:transparent;}}
.panel-card .workspace-overflow button.policy-button {{min-width:20px;min-height:20px;padding:2px;border:0;background:transparent;box-shadow:none;}}
.panel-card .workspace-overflow button.policy-button {{border-radius:5px;}}
.panel-card .workspace-overflow button.policy-button:hover,
.panel-card .workspace-overflow button.policy-button:focus-visible {{background:rgba(255,255,255,0.06);color:{text};outline:none;border:0;}}
.panel-card .workspace-overflow button.policy-button:active {{background:rgba(255,255,255,0.12);color:{text};outline:none;border:0;}}
.panel-card .workspace-overflow button.panel-close {{min-width:12px;min-height:12px;padding:0;}}
.panel-card .workspace-overflow button.panel-close .panel-symbol {{font-size:12px;}}
.panel-card .left-card button.workspace-tile {{padding:0;min-width:0;min-height:0;border:0;border-radius:0;background:{background};box-shadow:none;}}
.panel-card .left-card button.workspace-tile:hover {{border:0;}}
.panel-card .policy-label {{font-size:12px;font-weight:600;}}
.panel-card .policy-title {{font-size:14px;font-weight:600;}}
.panel-card .policy-muted {{font-size:11px;color:{muted};}}
.panel-card .policy-feedback {{color:{accent};}}
.panel-card button.policy-button {{min-height:30px;padding:0 10px;margin:0;border:1px solid transparent;border-radius:7px;background:{surface};box-shadow:none;color:{text};font-size:12px;}}
.panel-card button.policy-button:disabled {{opacity:0.45;}}
.panel-card button.policy-button:hover {{border-color:{border};color:{accent};}}
.panel-card button.policy-accent {{border-color:{accent};color:{accent};}}
.panel-card .policy-choice > button {{min-height:32px;padding:0 10px;border:1px solid transparent;border-radius:7px;background:{surface};box-shadow:none;color:{text};font-size:12px;}}
.panel-card .policy-choice popover contents {{background:{background};color:{text};border:1px solid {border};}}
.panel-card .policy-field {{min-height:32px;padding:0 10px;border:1px solid transparent;border-radius:7px;background:{surface};color:{text};font-size:12px;}}
.panel-card .policy-field:focus-within {{border-color:{accent};}}
.panel-card switch.policy-toggle {{min-width:38px;min-height:22px;padding:0;border:0;border-radius:11px;background:{surface};}}
.panel-card switch.policy-toggle:checked {{background:{accent};}}
.panel-card switch.policy-toggle slider {{min-width:13px;min-height:16px;margin:3px;padding:0;border:0;border-radius:8px;background:{text};box-shadow:none;}}
.panel-card switch.policy-toggle:checked slider {{background:{background};}}
.panel-card scale.policy-scale {{min-height:26px;padding:0;}}
.panel-card scale.policy-scale trough {{min-height:4px;border:0;border-radius:2px;background:{surface};}}
.panel-card scale.policy-scale highlight {{min-width:0;min-height:4px;border:0;border-radius:2px;background:{accent};}}
.panel-card scale.policy-scale slider {{min-width:12px;min-height:12px;margin:-4px;border:0 solid {surface};border-radius:8px;background:{text};box-shadow:none;}}
.panel-card scale.policy-scale slider:active {{border-color:{accent};}}
.panel-card .policy-map {{background:{surface};border-radius:10px;}}
.panel-card .policy-confirm {{padding:12px;background:{surface};border:1px solid {accent};border-radius:8px;}}
.panel-card .policy-card flowboxchild {{padding:0;}}
.panel-card button.panel-control:disabled {{opacity:0.4;}}
"#,
        background = color("background", "#1f1d2e"),
        text = color("text", "#e5e5e5"),
        border = color(border, "#555169"),
        surface = color("surface", "#26233a"),
        muted = color("muted", "#a5a0ba"),
        accent = color("accent", "#ff53a6")
    )
}
fn card_frame(header: &impl IsA<gtk::Widget>) -> gtk::Box {
    let card = gtk::Box::new(gtk::Orientation::Vertical, 0);
    card.add_css_class("panel-card");
    card.set_overflow(gtk::Overflow::Hidden);
    card.append(header);
    let divider = gtk::Separator::new(gtk::Orientation::Horizontal);
    divider.set_margin_top(10);
    divider.set_margin_bottom(13);
    card.append(&divider);
    card
}
pub fn heading(title: &str, subtitle: &str) -> gtk::Box {
    let column = gtk::Box::new(gtk::Orientation::Vertical, 4);
    let label = gtk::Label::new(Some(title));
    label.set_xalign(0.);
    label.add_css_class("panel-heading");
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    column.append(&label);
    if !subtitle.is_empty() {
        let label = gtk::Label::new(Some(subtitle));
        label.set_xalign(0.);
        label.set_wrap(true);
        label.add_css_class("panel-muted");
        column.append(&label);
    }
    column
}
pub fn status_row(label: &str, value: &str, detail: &str) -> gtk::Box {
    let column = gtk::Box::new(gtk::Orientation::Vertical, 3);
    column.add_css_class("panel-row");
    let line = gtk::Box::new(gtk::Orientation::Horizontal, 16);
    let name = gtk::Label::new(Some(label));
    name.set_hexpand(true);
    name.set_xalign(0.);
    name.set_ellipsize(gtk::pango::EllipsizeMode::End);
    let value = gtk::Label::new(Some(value));
    value.set_xalign(1.);
    value.set_selectable(true);
    value.set_ellipsize(gtk::pango::EllipsizeMode::End);
    line.append(&name);
    line.append(&value);
    column.append(&line);
    if !detail.is_empty() {
        let label = gtk::Label::new(Some(detail));
        label.set_xalign(0.);
        label.set_wrap(true);
        label.add_css_class("panel-muted");
        column.append(&label);
    }
    column
}
pub fn section(title: &str) -> gtk::Box {
    let body = gtk::Box::new(gtk::Orientation::Vertical, 8);
    if !title.is_empty() {
        body.append(&heading(title, ""));
    }
    body
}
/// Provider-owned body plus presentation hints; no geometry, hover, or IPC policy.
pub struct Content {
    pub title: String,
    pub body: gtk::Widget,
    pub footer: Option<gtk::Widget>,
    pub preferred_width: f64,
    pub needs_keyboard: bool,
}
pub fn footer(text: &str) -> gtk::Widget {
    let label = gtk::Label::new(Some(text));
    label.set_xalign(0.);
    label.set_wrap(true);
    label.add_css_class("panel-muted");
    label.upcast()
}
pub fn locale_content(target: &Target, send: async_channel::Sender<crate::Event>) -> Content {
    let body = gtk::Box::new(gtk::Orientation::Vertical, 6);
    for row in arr(&target.row["panel"]["rows"]) {
        let button = gtk::Button::new();
        button.add_css_class("input-method");
        let active = row["active"] == true;
        if active {
            button.add_css_class("current");
        }
        let line = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        let badge = gtk::Label::new(Some(s(&row["label"])));
        badge.add_css_class("input-badge");
        badge.set_valign(gtk::Align::Center);
        line.append(&badge);
        let names = gtk::Box::new(gtk::Orientation::Vertical, 3);
        names.set_hexpand(true);
        names.set_valign(gtk::Align::Center);
        let name = gtk::Label::new(Some(s(&row["detail"])));
        name.set_xalign(0.);
        name.set_ellipsize(gtk::pango::EllipsizeMode::End);
        name.add_css_class("input-name");
        names.append(&name);
        if active {
            let caption = gtk::Label::new(Some("Current input method"));
            caption.set_xalign(0.);
            caption.add_css_class("input-caption");
            names.append(&caption);
        }
        line.append(&names);
        if active {
            let check = control_symbol("check");
            check.add_css_class("input-check");
            line.append(&check);
        }
        button.set_child(Some(&line));
        button.set_sensitive(!s(&row["id"]).is_empty());
        button.update_property(&[gtk::accessible::Property::Label(s(&row["detail"]))]);
        let context = json!({"barKey":target.bar,"module":target.module,"monitor":target.output,"action":"select","selection":row["id"]});
        let send = send.clone();
        button.connect_clicked(move |_| {
            let _ = send.try_send(crate::Event::ConnectivityAction(context.clone()));
        });
        body.append(&button);
    }
    Content {
        title: "Input methods".into(),
        body: body.upcast(),
        footer: None,
        preferred_width: 420.,
        needs_keyboard: true,
    }
}
pub fn status_content(target: &Target) -> Content {
    if target.provider == "codex" {
        return crate::codex_card::content(&target.row, &target.style);
    }
    if target.provider == "openrouter" {
        return crate::openrouter_card::content(&target.row);
    }
    let panel = &target.row["panel"];
    let body = section("");
    if target.provider == "datetime" {
        body.set_spacing(6);
    }
    if arr(&panel["rows"]).is_empty() {
        body.append(&status_row(
            "Status",
            "No details available",
            target.row["tooltip"].as_str().unwrap_or(""),
        ));
    } else {
        let mut group = String::new();
        for row in arr(&panel["rows"]).iter().take(128) {
            let next = s(&row["group"]);
            if !next.is_empty() && group != next {
                body.append(&heading(next, ""));
                group = next.into();
            }
            let make_row = if target.provider == "datetime" {
                clock_row
            } else {
                status_row
            };
            body.append(&make_row(
                s(&row["label"]),
                s(&row["value"]),
                s(&row["detail"]),
            ));
        }
    }
    Content {
        title: panel["title"].as_str().unwrap_or(&target.provider).into(),
        body: body.upcast(),
        footer: panel["footer"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(footer),
        preferred_width: 420.,
        needs_keyboard: false,
    }
}
pub struct Surface {
    pub window: gtk::ApplicationWindow,
    floating: Option<gtk::ApplicationWindow>,
    pub native_title: Option<String>,
    close_target: Rc<RefCell<Option<String>>>,
    fixed: gtk::Fixed,
    card: gtk::Box,
    reveal: crate::panel_reveal::Reveal,
    header: gtk::Box,
    top: gtk::Box,
    divider: gtk::Widget,
    buttons: gtk::Box,
    overlay: gtk::Overlay,
    stack: gtk::Stack,
    scroll: gtk::ScrolledWindow,
    detached: Rc<Cell<Option<(f64, f64)>>>,
    close: gtk::Button,
    css: gtk::CssProvider,
    last: Value,
    natural_height: f64,
    preferred_width: f64,
    needs_keyboard: bool,
    footer: gtk::Box,
    pub output: String,
    pub inside: Rc<Cell<bool>>,
    actual: Rc<Cell<Rect>>,
    target: Rc<Cell<Rect>>,
    screen: Rc<Cell<Rect>>,
    from: Rc<Cell<Rect>>,
    since: Rc<RefCell<Instant>>,
    fade_since: Rc<RefCell<Instant>>,
    ticking: Rc<Cell<bool>>,
}
impl Drop for Surface {
    fn drop(&mut self) {
        if let Some(d) = gdk::Display::default() {
            gtk::style_context_remove_provider_for_display(&d, &self.css);
        }
        if let Some(window) = self.floating.take() {
            window.destroy();
        }
        self.window.destroy();
    }
}
impl Surface {
    pub fn new(
        app: &gtk::Application,
        monitor: &gdk::Monitor,
        output: &str,
        send: async_channel::Sender<Event>,
    ) -> Self {
        let window = crate::ui::surface(app, monitor, "bartjik-gtk-panel-shell", Layer::Overlay);
        for edge in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
            window.set_anchor(edge, true);
        }
        let fixed = gtk::Fixed::new();
        window.set_child(Some(&fixed));
        let header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        let close = gtk::Button::new();
        close.add_css_class("panel-control");
        close.add_css_class("panel-close");
        close.update_property(&[gtk::accessible::Property::Label("Close panel")]);
        let close_target = Rc::new(RefCell::new(None::<String>));
        let close_id = close_target.clone();
        let tx = send.clone();
        close.connect_clicked(move |_| {
            let event = close_id
                .borrow()
                .as_ref()
                .map(|id| Event::CloseDetached(id.clone()))
                .unwrap_or(Event::PanelCommand("close".into()));
            let _ = tx.try_send(event);
        });
        buttons.append(&close);
        let top = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        header.set_hexpand(true);
        top.append(&header);
        top.append(&buttons);
        let card = card_frame(&top);
        let divider = top.next_sibling().unwrap();
        let stack = gtk::Stack::new();
        stack.set_transition_type(gtk::StackTransitionType::None);
        stack.set_transition_duration(140);
        stack.set_hhomogeneous(false);
        stack.set_vhomogeneous(false);
        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_height(true)
            .build();
        scroll.set_child(Some(&stack));
        let overlay = gtk::Overlay::new();
        overlay.set_child(Some(&scroll));
        card.append(&overlay);
        let footer = gtk::Box::new(gtk::Orientation::Vertical, 0);
        footer.set_visible(false);
        footer.set_margin_top(12);
        card.append(&footer);
        let reveal = crate::panel_reveal::Reveal::new(&card);
        fixed.put(&reveal, 0., 0.);
        let inside = Rc::new(Cell::new(false));
        let motion = gtk::EventControllerMotion::new();
        let i = inside.clone();
        motion.connect_enter(move |_, _, _| i.set(true));
        let i = inside.clone();
        motion.connect_leave(move |_| i.set(false));
        reveal.add_controller(motion);
        let drag_send = send.clone();
        let keys = gtk::EventControllerKey::new();
        keys.connect_key_pressed(move |_, key, _, _| {
            if key == gdk::Key::Escape {
                let _ = send.try_send(Event::PanelCommand("close".into()));
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
        window.add_controller(keys);
        let css = gtk::CssProvider::new();
        gtk::style_context_add_provider_for_display(
            &gdk::Display::default().unwrap(),
            &css,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 2,
        );
        let zero = Rect {
            x: 0.,
            y: 0.,
            width: 0.,
            height: 0.,
        };
        let surface = Self {
            floating: None,
            native_title: None,
            close_target,
            window,
            fixed,
            card,
            reveal,
            header,
            top,
            divider,
            buttons,
            overlay,
            stack,
            scroll,
            detached: Rc::new(Cell::new(None)),
            close,
            css,
            last: Value::Null,
            natural_height: 120.,
            preferred_width: 420.,
            needs_keyboard: false,
            footer,
            output: output.into(),
            inside,
            actual: Rc::new(Cell::new(zero)),
            target: Rc::new(Cell::new(zero)),
            screen: Rc::new(Cell::new(zero)),
            from: Rc::new(Cell::new(zero)),
            since: Rc::new(RefCell::new(Instant::now())),
            fade_since: Rc::new(RefCell::new(Instant::now())),
            ticking: Rc::new(Cell::new(false)),
        };
        surface.install_drag(drag_send);
        surface
    }
    fn install_drag(&self, send: async_channel::Sender<Event>) {
        let drag = gtk::GestureDrag::new();
        drag.set_button(1);
        let origin = Rc::new(Cell::new(None::<Rect>));
        let start = origin.clone();
        let actual = self.actual.clone();
        drag.connect_drag_begin(move |gesture, x, y| {
            let Some(widget) = gesture.widget() else {
                return;
            };
            // Let controls, text selection and scrolling retain their own gestures.
            let mut picked = widget.pick(x, y, gtk::PickFlags::DEFAULT);
            while let Some(child) = picked {
                if child.has_css_class("panel-interactive")
                    || child.is::<gtk::Switch>()
                    || child.is::<gtk::DropDown>()
                    || child.is::<gtk::Button>()
                    || child.is::<gtk::Range>()
                    || child.is::<gtk::Editable>()
                    || child
                        .downcast_ref::<gtk::ScrolledWindow>()
                        .is_some_and(|s| {
                            let a = s.vadjustment();
                            a.upper() > a.page_size()
                        })
                    || child
                        .downcast_ref::<gtk::Label>()
                        .is_some_and(|l| l.is_selectable())
                {
                    gesture.set_state(gtk::EventSequenceState::Denied);
                    start.set(None);
                    return;
                }
                if child == widget {
                    break;
                }
                picked = child.parent();
            }
            start.set(Some(actual.get()));
        });
        let detached = self.detached.clone();
        let actual = self.actual.clone();
        let target = self.target.clone();
        let from = self.from.clone();
        let screen = self.screen.clone();
        let window = self.window.downgrade();
        let fixed = self.fixed.downgrade();
        let reveal = self.reveal.downgrade();
        let close = self.close.downgrade();
        let drag_send = send.clone();
        drag.connect_drag_update(move |gesture, dx, dy| {
            let (Some(window), Some(fixed), Some(reveal), Some(close)) = (
                window.upgrade(),
                fixed.upgrade(),
                reveal.upgrade(),
                close.upgrade(),
            ) else {
                return;
            };
            let Some(mut r) = origin.get() else {
                return;
            };
            if dx.hypot(dy) < 8. && detached.get().is_none() {
                return;
            }
            gesture.set_state(gtk::EventSequenceState::Claimed);
            let bounds = screen.get();
            r = dragged_rect(r, bounds, dx, dy);
            let first = detached.get().is_none();
            detached.set(Some((r.x, r.y)));
            actual.set(r);
            target.set(r);
            from.set(r);
            close.set_visible(true);
            apply(&window, &fixed, &reveal, r, bounds, 1.);
            if first {
                let _ = drag_send.try_send(Event::PanelCommand("detach".into()));
            }
        });
        let tx = send.clone();
        let detached = self.detached.clone();
        drag.connect_drag_end(move |_, _, _| {
            if detached.get().is_some() {
                let _ = tx.try_send(Event::PanelCommand("float".into()));
            }
        });
        // The fullscreen shell stays stationary while the card moves beneath it.
        self.window.add_controller(drag);
    }
    pub fn is_native(&self) -> bool {
        self.floating.is_some()
    }
    pub fn make_native(&mut self, title: &str, send: async_channel::Sender<Event>) {
        if self.floating.is_some() {
            return;
        }
        let native = gtk::ApplicationWindow::builder()
            .application(&self.window.application().unwrap())
            .title(title)
            .decorated(false)
            .resizable(false)
            .build();
        native.add_css_class("tjik");
        let rect = self.actual.get();
        *self.close_target.borrow_mut() = Some(title.to_owned());
        // Retain the last rendered face through Wayland configure/map/first paint.
        // Reparenting the live widget immediately otherwise leaves a blank frame.
        let snapshot = gtk::Snapshot::new();
        let scale = self.window.scale_factor().max(1) as f32;
        snapshot.scale(scale, scale);
        gtk::WidgetPaintable::new(Some(&self.reveal)).snapshot(&snapshot, rect.width, rect.height);
        let handoff = snapshot.to_node().and_then(|node| {
            self.window.renderer().map(|renderer| {
                let texture = renderer.render_texture(
                    &node,
                    Some(&gtk::graphene::Rect::new(
                        0.,
                        0.,
                        rect.width as f32 * scale,
                        rect.height as f32 * scale,
                    )),
                );
                let picture = gtk::Picture::for_paintable(&texture);
                picture.set_can_shrink(true);
                picture.set_content_fit(gtk::ContentFit::Fill);
                picture.set_size_request(rect.width.ceil() as i32, rect.height.ceil() as i32);
                self.fixed.put(
                    &picture,
                    (rect.x - self.screen.get().x).round(),
                    (rect.y - self.screen.get().y).round(),
                );
                picture
            })
        });
        self.fixed.remove(&self.reveal);
        self.reveal.set_visible(true);
        self.reveal
            .layout(rect.width.ceil() as i32, rect.height.ceil() as i32);
        self.reveal
            .frame(rect.width.ceil() as i32, rect.height.ceil() as i32, 1.);
        native.set_child(Some(&self.reveal));
        native.set_default_size(rect.width.ceil() as i32, rect.height.ceil() as i32);
        let tx = send.clone();
        let close_title = title.to_owned();
        native.connect_close_request(move |_| {
            let _ = tx.try_send(Event::CloseDetached(close_title.clone()));
            glib::Propagation::Stop
        });
        let keys = gtk::EventControllerKey::new();
        let close_title = title.to_owned();
        keys.connect_key_pressed(move |_, key, _, _| {
            if key == gdk::Key::Escape {
                let _ = send.try_send(Event::CloseDetached(close_title.clone()));
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
        native.add_controller(keys);
        // Use native Wayland move requests after detachment; the compositor owns
        // subsequent position and workspace changes, exactly like any window.
        let drag = gtk::GestureDrag::new();
        drag.set_button(1);
        let weak = native.downgrade();
        drag.connect_drag_begin(move |gesture, x, y| {
            let Some(window) = weak.upgrade() else {
                return;
            };
            let mut picked = window.pick(x, y, gtk::PickFlags::DEFAULT);
            while let Some(w) = picked {
                if w.has_css_class("panel-interactive")
                    || w.is::<gtk::Button>()
                    || w.is::<gtk::Range>()
                    || w.is::<gtk::Editable>()
                    || w.is::<gtk::DropDown>()
                    || w.is::<gtk::Switch>()
                    || w.downcast_ref::<gtk::Label>()
                        .is_some_and(|l| l.is_selectable())
                    || w.downcast_ref::<gtk::ScrolledWindow>()
                        .is_some_and(|s| s.vadjustment().upper() > s.vadjustment().page_size())
                {
                    gesture.set_state(gtk::EventSequenceState::Denied);
                    return;
                }
                picked = w.parent();
            }
            if let (Some(surface), Some(event)) = (
                window.surface().and_downcast::<gdk::Toplevel>(),
                gesture.current_event(),
            ) {
                if let Some(device) = event.device() {
                    surface.begin_move(&device, 1, x, y, event.time());
                }
            }
        });
        native.add_controller(drag);
        let old = self.window.downgrade();
        let fixed = self.fixed.downgrade();
        let face = handoff.map(|p| p.downgrade());
        let finish: Rc<dyn Fn()> = Rc::new(move || {
            if let Some(old) = old.upgrade() {
                old.set_visible(false);
            }
            if let (Some(fixed), Some(face)) =
                (fixed.upgrade(), face.as_ref().and_then(|p| p.upgrade()))
            {
                if face.parent().is_some() {
                    fixed.remove(&face);
                }
            }
        });
        let painted = finish.clone();
        native.connect_map(move |window| {
            if let Some(clock) = window.frame_clock() {
                let signal = Rc::new(RefCell::new(None));
                let slot = signal.clone();
                let done = painted.clone();
                *signal.borrow_mut() = Some(clock.connect_after_paint(move |clock| {
                    done();
                    if let Some(id) = slot.borrow_mut().take() {
                        clock.disconnect(id);
                    }
                }));
            }
        });
        // Hidden workspaces may throttle frame callbacks. Never retain an overlay
        // indefinitely if the user switches workspace during the handoff.
        glib::timeout_add_local_once(std::time::Duration::from_millis(250), move || finish());
        native.set_visible(true);
        self.floating = Some(native);
    }
    // Keep the layer surface mapped so returning to a workspace does not replay
    // the compositor's layer-open animation. Hidden content owns no desktop input.
    pub fn set_workspace_visible(&self, visible: bool) {
        if self.is_native() {
            return;
        }
        // Match bar concealment: hide the whole native surface as well as its
        // children, so the compositor cannot retain a previously drawn card.
        self.window.set_opacity(if visible { 1. } else { 0. });
        self.window.queue_draw();
        if self.fixed.is_visible() == visible {
            return;
        }
        self.fixed.set_visible(visible);
        self.inside.set(false);
        self.window.set_keyboard_mode(KeyboardMode::None);
        apply(
            &self.window,
            &self.fixed,
            &self.reveal,
            self.actual.get(),
            self.screen.get(),
            1.,
        );
    }
    pub fn rect(&self) -> Rect {
        self.actual.get()
    }
    pub fn status(&self) -> Value {
        let window = self.floating.as_ref().unwrap_or(&self.window);
        let allocated=self.reveal.compute_bounds(window).map(|r|json!({"x":r.x() as f64+self.screen.get().x,"y":r.y() as f64+self.screen.get().y,"width":r.width(),"height":r.height()}));
        json!({"native":self.is_native(),"nativeTitle":self.native_title,"layoutSize":self.reveal.layout_size(),"allocation":allocated,"surfaceWidth":window.width(),"surfaceHeight":window.height(),"output":self.output,"rect":self.rect(),"target":self.target.get(),"visible":window.is_visible() && (self.is_native() || self.fixed.is_visible()),"mapped":window.is_mapped(),"opacity":window.opacity(),"inside":self.inside.get(),"transitioning":self.ticking.get(),"keyboard":format!("{:?}",self.window.keyboard_mode())})
    }
    pub fn show(&mut self, target: &Target, pinned: bool, content: impl FnOnce() -> Content) {
        let data =
            json!({"key":target.key,"row":target.row,"style":target.style,"screen":target.screen});
        let changed = self.last != data;
        let switching = self.last["key"] != data["key"];
        if switching {
            *self.fade_since.borrow_mut() = Instant::now();
            self.reveal.freeze();
        }
        if switching {
            self.detached.set(None);
        }
        self.close.set_visible(self.detached.get().is_some());
        self.screen.set(target.screen);
        if changed {
            self.last = data;
            self.css.load_from_string(&stylesheet(&target.style));
            let color = |key: &str, fallback: &str| {
                gdk::RGBA::parse(s(&target.style[key]))
                    .unwrap_or_else(|_| gdk::RGBA::parse(fallback).unwrap())
            };
            let border = if s(&target.style["panelBorder"]).is_empty() {
                "border"
            } else {
                "panelBorder"
            };
            self.reveal
                .colors(color("background", "#1f1d2e"), color(border, "#555169"));
            if target.provider == "workspaces" {
                self.close.remove_css_class("panel-control");
                self.close.remove_css_class("panel-close");
                self.close.set_has_frame(false);
                self.close.add_css_class("workspace-panel-close");
                self.close
                    .set_child(Some(&workspace_close_art(&target.style, 0, 0)));
            } else {
                self.close.add_css_class("panel-control");
                self.close.add_css_class("panel-close");
                self.close.set_has_frame(true);
                self.close.remove_css_class("workspace-panel-close");
                self.close.set_child(Some(&artwork("close", 20)));
            }
            while let Some(c) = self.header.first_child() {
                self.header.remove(&c);
            }
            let content = content();
            self.preferred_width = content.preferred_width.clamp(200., 900.);
            self.needs_keyboard = content.needs_keyboard;
            let headerless = ["playerctl", "workspaces"].contains(&target.provider.as_str());
            self.top.set_visible(!headerless);
            self.divider.set_visible(!headerless);
            if headerless {
                self.card.add_css_class("panel-edge-content");
                if self.close.parent().as_ref() == Some(self.buttons.upcast_ref()) {
                    self.buttons.remove(&self.close);
                    self.overlay.add_overlay(&self.close);
                }
                self.close.set_halign(gtk::Align::End);
                self.close.set_valign(gtk::Align::Start);
                self.close.set_margin_top(10);
                self.close.set_margin_end(10);
                if target.provider == "workspaces" {
                    let h = target.style["previewHeight"]
                        .as_f64()
                        .unwrap_or(360.)
                        .min((target.screen.height - 24.).max(120.));
                    let scale = ((self.preferred_width - 20.) / target.screen.width.max(1.))
                        .min((h - 20.) / target.screen.height.max(1.));
                    let right = ((self.preferred_width - target.screen.width * scale) / 2. - 1.)
                        .round() as i32;
                    let top = ((h - target.screen.height * scale) / 2. - 1.).round() as i32;
                    self.close.set_margin_end(0);
                    self.close.set_margin_top(0);
                    self.close
                        .set_child(Some(&workspace_close_art(&target.style, top, right)));
                }
            } else {
                self.card.remove_css_class("panel-edge-content");
                if self.close.parent().as_ref() == Some(self.overlay.upcast_ref()) {
                    self.overlay.remove_overlay(&self.close);
                    self.buttons.append(&self.close);
                }
                self.close.set_margin_top(0);
                self.close.set_margin_end(0);
                let header = card_header(&content.title, &target.provider);
                if target.provider == "crypto" {
                    if let Some(old) = header.first_child() {
                        header.remove(&old);
                    }
                    header.prepend(&crate::icons::artwork(
                        &target.row["icon"],
                        &target.style,
                        22,
                    ));
                }
                self.header.append(&header);
            }
            while let Some(child) = self.footer.first_child() {
                self.footer.remove(&child);
            }
            self.footer.set_visible(content.footer.is_some());
            if let Some(footer) = content.footer {
                self.footer.append(&footer);
            }
            let body = content.body;
            // The outgoing card is an immutable render node, not a second live layout.
            if body.parent().as_ref() != Some(self.stack.upcast_ref()) {
                while let Some(child) = self.stack.first_child() {
                    self.stack.remove(&child);
                }
                self.stack.add_child(&body);
            }
            self.stack.set_visible_child(&body);
        }
        if !self.is_native() {
            self.window
                .set_keyboard_mode(if self.needs_keyboard || pinned {
                    KeyboardMode::OnDemand
                } else {
                    KeyboardMode::None
                });
        }
        let width = self
            .preferred_width
            .min((target.screen.width - 24.).max(1.));
        self.scroll
            .set_max_content_height((target.screen.height * 0.65 - 90.).max(40.) as i32);
        {
            self.card.set_size_request(width as i32, -1);
            self.natural_height = (self
                .card
                .measure(gtk::Orientation::Vertical, width as i32)
                .1 as f64)
                .max(120.);
        }
        if let Some(native) = &self.floating {
            let height = self.natural_height.ceil() as i32;
            self.reveal.layout(width.ceil() as i32, height);
            self.reveal.frame(width.ceil() as i32, height, 1.);
            native.set_default_size(width.ceil() as i32, height);
            return;
        }
        let mut next = panels::place(target, width, self.natural_height);
        if let Some((x, y)) = self.detached.get() {
            next.x = x;
            next.y = y;
        }
        self.reveal
            .layout(next.width.ceil() as i32, next.height.ceil() as i32);
        if self.target.get() != next || switching {
            self.from.set(if self.window.is_visible() {
                self.actual.get()
            } else {
                next
            });
            self.target.set(next);
            *self.since.borrow_mut() = Instant::now();
            if !self.window.is_visible() {
                self.actual.set(next);
                apply(
                    &self.window,
                    &self.fixed,
                    &self.reveal,
                    next,
                    target.screen,
                    1.,
                );
            }
            self.animate();
        }
        self.window.set_visible(true);
    }
    fn animate(&self) {
        if self.ticking.replace(true) {
            return;
        }
        let (actual, target, from, screen, since, ticking) = (
            self.actual.clone(),
            self.target.clone(),
            self.from.clone(),
            self.screen.clone(),
            self.since.clone(),
            self.ticking.clone(),
        );
        let fade_since = self.fade_since.clone();
        let fixed = self.fixed.clone();
        let reveal = self.reveal.clone();
        self.window.add_tick_callback(move |window, _| {
            if reveal.parent().as_ref() != Some(fixed.upcast_ref()) {
                ticking.set(false);
                return glib::ControlFlow::Break;
            }
            let t = (since.borrow().elapsed().as_secs_f64() / 0.14).min(1.);
            // Pointer-following can retarget geometry every few milliseconds. It
            // must never restart the outgoing/incoming content handoff.
            let fade = (fade_since.borrow().elapsed().as_secs_f64() / 0.14).min(1.);
            let (f, opacity) = transition_progress(t, fade);
            let a = from.get();
            let b = target.get();
            let r = Rect {
                x: a.x + (b.x - a.x) * f,
                y: a.y + (b.y - a.y) * f,
                width: a.width + (b.width - a.width) * f,
                height: a.height + (b.height - a.height) * f,
            };
            actual.set(r);
            apply(window, &fixed, &reveal, r, screen.get(), opacity);
            if t >= 1. && fade >= 1. {
                ticking.set(false);
                glib::ControlFlow::Break
            } else {
                glib::ControlFlow::Continue
            }
        });
    }
}
fn dragged_rect(mut r: Rect, bounds: Rect, dx: f64, dy: f64) -> Rect {
    r.x = (r.x + dx).clamp(bounds.x, (bounds.x + bounds.width - r.width).max(bounds.x));
    r.y = (r.y + dy).clamp(
        bounds.y,
        (bounds.y + bounds.height - r.height).max(bounds.y),
    );
    r
}
fn transition_progress(geometry: f64, content: f64) -> (f64, f64) {
    let ease = |t: f64| 1. - (1. - t.clamp(0., 1.)).powi(3);
    (ease(geometry), ease(content))
}
#[cfg(test)]
mod transition_tests {
    use super::*;
    #[test]
    fn dragging_clamps_to_selected_output_including_negative_origins() {
        let bounds = Rect {
            x: -1000.,
            y: 50.,
            width: 1000.,
            height: 800.,
        };
        let r = Rect {
            x: -800.,
            y: 100.,
            width: 420.,
            height: 220.,
        };
        let moved = dragged_rect(r, bounds, 100., 60.);
        assert_eq!((moved.x, moved.y), (-700., 160.));
        let edge = dragged_rect(r, bounds, 2000., 2000.);
        assert_eq!((edge.x, edge.y), (-420., 630.));
        let edge = dragged_rect(r, bounds, -2000., -2000.);
        assert_eq!((edge.x, edge.y), (-1000., 50.));
    }
    #[test]
    fn pointer_retargets_never_rewind_content_fade() {
        let mut previous = 0.;
        // Every pointer sample restarts shell movement; the card switch happened once.
        for elapsed_ms in (0..=200).step_by(10) {
            let (position, opacity) = transition_progress(0., elapsed_ms as f64 / 140.);
            assert_eq!(position, 0.);
            assert!(opacity >= previous);
            if elapsed_ms >= 140 {
                assert_eq!(opacity, 1.);
            }
            previous = opacity;
        }
    }
}
fn apply(
    window: &gtk::ApplicationWindow,
    fixed: &gtk::Fixed,
    reveal: &crate::panel_reveal::Reveal,
    r: Rect,
    screen: Rect,
    progress: f64,
) {
    let x = (r.x - screen.x).round();
    let y = (r.y - screen.y).round();
    fixed.move_(reveal, x, y);
    reveal.frame(r.width.ceil() as i32, r.height.ceil() as i32, progress);
    if let Some(surface) = window.surface() {
        surface.set_input_region(&if !fixed.is_visible() {
            gtk::cairo::Region::create()
        } else {
            gtk::cairo::Region::create_rectangle(&gtk::cairo::RectangleInt::new(
                x.floor() as i32,
                y.floor() as i32,
                r.width.ceil() as i32,
                r.height.ceil() as i32,
            ))
        });
        surface.queue_render();
    }
}

#[cfg(test)]
mod render_tests {
    use super::*;
    /// The Python runner installs a workspace-7 rule BEFORE starting this test.
    #[test]
    #[ignore = "maps a synthetic toplevel silently on workspace 7 through the adapter"]
    fn detached_toplevel_render() {
        gtk::init().unwrap();
        let app = gtk::Application::builder()
            .application_id("org.tjikos.bartjik.detachedfixture")
            .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(None::<&gtk::gio::Cancellable>).unwrap();
        let desktop = tjik_desktop::selected();
        let before = desktop.snapshot().unwrap();
        let workspace = before
            .workspaces
            .iter()
            .find(|w| w.workspace.name.as_deref() == Some("7"))
            .expect("workspace 7 exists");
        let output = before
            .outputs
            .iter()
            .find(|o| Some(&o.id) == workspace.output.as_ref())
            .unwrap();
        let monitor = gdk::Display::default()
            .unwrap()
            .monitors()
            .iter::<gdk::Monitor>()
            .flatten()
            .find(|m| m.connector().as_deref() == Some(&output.name))
            .unwrap();
        let title = format!("bartjik-detached:fixture:{}", std::process::id());
        let rect = Rect {
            x: output.logical.x + 120.,
            y: output.logical.y + 140.,
            width: 420.,
            height: 280.,
        };
        let target = tjik_desktop::WorkspaceTarget {
            generation: before.generation.clone(),
            output: output.id.clone(),
            id: Some(workspace.workspace.id.clone()),
            name: "7".into(),
        };
        desktop.prepare_toplevel(&title, &target, rect).unwrap();
        struct Rule(std::sync::Arc<dyn tjik_desktop::Desktop>, String);
        impl Drop for Rule {
            fn drop(&mut self) {
                let _ = self.0.release_toplevel(&self.1);
            }
        }
        let _rule = Rule(desktop.clone(), title.clone());
        let (tx, _rx) = async_channel::bounded(8);
        let mut panel = Surface::new(&app, &monitor, &output.name, tx.clone());
        panel.actual.set(rect);
        panel.screen.set(output.logical);
        panel.native_title = Some(title.clone());
        panel
            .stack
            .add_child(&gtk::Label::new(Some("Synthetic detached panel")));
        // Use an ordinary synthetic source window on workspace 7 to exercise
        // the real rendered-face handoff without mapping a desktop-wide layer.
        let source_title = format!("bartjik-detached:source:{}", std::process::id());
        desktop
            .prepare_toplevel(&source_title, &target, rect)
            .unwrap();
        let _source_rule = Rule(desktop.clone(), source_title.clone());
        panel.window.set_child(None::<&gtk::Widget>);
        panel.window.destroy();
        panel.window = gtk::ApplicationWindow::builder()
            .application(&app)
            .title(&source_title)
            .decorated(false)
            .build();
        panel.window.set_child(Some(&panel.fixed));
        panel.window.set_default_size(420, 280);
        panel.screen.set(rect);
        panel.reveal.layout(420, 280);
        panel.reveal.frame(420, 280, 1.);
        panel.window.set_visible(true);
        for _ in 0..20 {
            while glib::MainContext::default().pending() {
                glib::MainContext::default().iteration(false);
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panel.fixed.allocate(420, 280, -1, None);
        panel.make_native(&title, tx);
        assert!(
            panel.window.is_visible(),
            "source disappeared before native paint"
        );
        assert!(
            panel
                .fixed
                .first_child()
                .is_some_and(|w| w.is::<gtk::Picture>()),
            "handoff did not retain a rendered face"
        );
        let until = std::time::Instant::now() + std::time::Duration::from_secs(3);
        let found = loop {
            while glib::MainContext::default().pending() {
                glib::MainContext::default().iteration(false);
            }
            let snap = desktop.snapshot().unwrap();
            if let Some(w) = snap
                .windows
                .iter()
                .find(|w| w.title.as_deref() == Some(&title))
            {
                break w.clone();
            }
            assert!(
                std::time::Instant::now() < until,
                "detached window did not map"
            );
            std::thread::sleep(std::time::Duration::from_millis(30));
        };
        assert_eq!(found.workspace.as_ref().unwrap().id, workspace.workspace.id);
        assert_eq!(found.floating, Some(true));
        assert_ne!(found.focus_order, Some(0), "fixture stole focus");
        assert!(panel.floating.as_ref().unwrap().is_mapped());
        for _ in 0..30 {
            while glib::MainContext::default().pending() {
                glib::MainContext::default().iteration(false);
            }
            if !panel.window.is_visible() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(!panel.window.is_visible(), "old overlay stayed visible");
        assert!(
            panel.fixed.first_child().is_none(),
            "handoff snapshot was not released"
        );
        let geometry = found.geometry.unwrap();
        assert!(
            (geometry.x - rect.x).abs() <= 1. && (geometry.y - rect.y).abs() <= 1.,
            "drop placement: {geometry:?}"
        );
        let identity = found.id;
        panel.set_workspace_visible(false);
        assert!(
            panel.floating.as_ref().unwrap().is_visible(),
            "application must not remap compositor-owned windows"
        );
        assert!(desktop.snapshot().unwrap().windows.iter().any(
            |w| w.id == identity && w.workspace.as_ref().unwrap().id == workspace.workspace.id
        ));
        panel.css.load_from_string(&stylesheet(&json!({"background":"#1f1d2e","surface":"#26233a","text":"#e5e5e5","border":"#393546","font":"tijpset"})));
        let button = gtk::Button::with_label("Synthetic overflow icon");
        panel.stack.add_child(&button);
        panel.stack.set_visible_child(&button);
        let tip = crate::left_card::overflow_tooltip(&button, "HYPE.HK — Zen Browser");
        tip.set_parent(&button);
        tip.popup();
        for _ in 0..20 {
            while glib::MainContext::default().pending() {
                glib::MainContext::default().iteration(false);
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let text = tip.child().unwrap();
        let width = text.measure(gtk::Orientation::Horizontal, -1).1;
        let height = text.measure(gtk::Orientation::Vertical, width).1;
        assert!(
            width <= 200 && height <= 32,
            "oversized tooltip: {width}x{height}"
        );
        let shot = gtk::Snapshot::new();
        gtk::WidgetPaintable::new(Some(&tip)).snapshot(
            &shot,
            tip.width() as f64,
            tip.height() as f64,
        );
        panel
            .floating
            .as_ref()
            .unwrap()
            .renderer()
            .unwrap()
            .render_texture(&shot.to_node().expect("tooltip rendered"), None)
            .save_to_png("/tmp/bartjik-workspace-tooltip.png")
            .unwrap();
        tip.popdown();
        tip.unparent();
        eprintln!("PASS: detached native toplevel assigned workspace 7 before mapping, no focus, exact placement, layer unmapped, identity retained");
        panel.close.emit_clicked();
        let close = _rx.try_recv().expect("scoped close action");
        crate::ui::exercise_detached_ownership(&app, panel, title.clone(), close);
    }
    #[test]
    #[ignore = "requires the workspace-7 fixture runner"]
    fn panel_reveal_render() {
        gtk::init().unwrap();
        let style = json!({"background":"#1f1d2e","surface":"#26233a","text":"#e5e5e5","muted":"#a5a0ba","border":"#555169","font":"tijpset"});
        let css = gtk::CssProvider::new();
        css.load_from_string(&stylesheet(&style));
        gtk::style_context_add_provider_for_display(
            &gdk::Display::default().unwrap(),
            &css,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 2,
        );
        let header = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        header.append(&card_header("World clock", "datetime"));
        let card = card_frame(&header);
        let body = gtk::Box::new(gtk::Orientation::Vertical, 6);
        body.append(&clock_row("Hong Kong", "18:42", "Monday, 9 November"));
        body.append(&clock_row("London", "10:42", "Monday, 9 November"));
        body.append(&clock_row("New York", "05:42", "Monday, 9 November"));
        card.append(&body);
        let reveal = crate::panel_reveal::Reveal::new(&card);
        reveal.colors(
            gdk::RGBA::parse("#1f1d2e").unwrap(),
            gdk::RGBA::parse("#555169").unwrap(),
        );
        reveal.layout(420, 268);
        reveal.frame(420, 268, 1.);
        let fixed = gtk::Fixed::new();
        fixed.put(&reveal, 0., 0.);
        let window = gtk::Window::builder()
            .title("Bartjik Panel Fixture")
            .decorated(false)
            .default_width(500)
            .default_height(600)
            .child(&fixed)
            .build();
        window.present();
        let context = glib::MainContext::default();
        let until = Instant::now() + std::time::Duration::from_secs(2);
        while Instant::now() < until {
            while context.pending() {
                context.iteration(false);
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        fn positions(
            widget: &gtk::Widget,
            root: &gtk::Widget,
            values: &mut Vec<(String, f32, f32, f32, f32)>,
        ) {
            if let Some(label) = widget.downcast_ref::<gtk::Label>() {
                let r = label.compute_bounds(root).unwrap();
                values.push((label.text().into(), r.x(), r.y(), r.width(), r.height()));
            }
            let mut child = widget.first_child();
            while let Some(c) = child {
                child = c.next_sibling();
                positions(&c, root, values);
            }
        }
        reveal.freeze();
        assert!(reveal.has_outgoing());
        while let Some(c) = header.first_child() {
            header.remove(&c);
        }
        header.append(&card_header("Codex", "codex"));
        while let Some(c) = body.first_child() {
            body.remove(&c);
        }
        let row: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/codex-card.json")).unwrap();
        body.append(&crate::codex_card::content_at(&row, &style, 1000000., false).body);
        reveal.layout(440, 529);
        let mut baseline = vec![];
        for (index, progress) in [0., 0.25, 0.5, 0.75, 1.].into_iter().enumerate() {
            let w = 420 + (20. * progress) as i32;
            let h = 268 + (261. * progress) as i32;
            reveal.frame(w, h, progress);
            // Allocate explicitly: inactive workspaces need not receive compositor frames.
            reveal.allocate(w, h, -1, None);
            let r = card.compute_bounds(&reveal).unwrap();
            assert_eq!((r.width(), r.height()), (440., 529.));
            let mut current = vec![];
            positions(card.upcast_ref(), card.upcast_ref(), &mut current);
            if index == 0 {
                baseline = current;
            } else {
                assert_eq!(
                    baseline, current,
                    "Destination labels reflowed during reveal"
                );
            }
            assert!(
                reveal
                    .pick(20., h as f64 + 8., gtk::PickFlags::DEFAULT)
                    .is_none(),
                "Clipped content accepted input"
            );
            if index == 2 {
                // A geometry-only restart used to make the outgoing card reappear.
                reveal.frame(w, h, 0.05);
                assert_eq!(reveal.fade_progress(), progress);
                let snap = gtk::Snapshot::new();
                fixed.snapshot_child(&reveal, &snap);
                window
                    .renderer()
                    .unwrap()
                    .render_texture(
                        &snap.to_node().unwrap(),
                        Some(&gtk::graphene::Rect::new(0., 0., w as f32, h as f32)),
                    )
                    .save_to_png("/tmp/bartjik-panel-transition-mid.png")
                    .unwrap();
            }
        }
        assert!(
            !reveal.has_outgoing(),
            "Completed transition retained old content"
        );
        // Shrinking and an interrupted handoff preserve wrapping at the new width.
        reveal.freeze();
        while let Some(c) = body.first_child() {
            body.remove(&c);
        }
        let wrapped=gtk::Label::new(Some("A long destination paragraph must keep exactly the same line breaks while its enclosing card shrinks and while the pointer moves to the next card."));
        wrapped.set_wrap(true);
        body.append(&wrapped);
        reveal.layout(280, 240);
        let mut lines = None;
        for (w, h, p) in [(440, 529, 0.), (360, 380, 0.5), (280, 240, 1.)] {
            reveal.frame(w, h, p);
            reveal.allocate(w, h, -1, None);
            let count = wrapped.layout().line_count();
            assert!(count > 1);
            if let Some(previous) = lines {
                assert_eq!(previous, count);
            } else {
                lines = Some(count);
            }
            if p == 0.5 {
                reveal.freeze();
                assert!(reveal.has_outgoing());
            }
        }
        assert!(!reveal.has_outgoing());
        // Equal-sized card switches must crossfade even without a geometry change.
        reveal.freeze();
        assert!(reveal.has_outgoing());
        reveal.frame(280, 240, 1.);
        assert!(!reveal.has_outgoing());
        window.close();
    }
    #[test]
    #[ignore = "requires a display and the workspace-7 fixture runner"]
    fn panel_primitives_render() {
        gtk::init().unwrap();
        let style = json!({"background":"#1f1d2e","surface":"#26233a","text":"#e5e5e5","muted":"#a5a0ba","border":"#555169","font":"tijpset"});
        let css = gtk::CssProvider::new();
        css.load_from_string(&stylesheet(&style));
        gtk::style_context_add_provider_for_display(
            &gdk::Display::default().unwrap(),
            &css,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 2,
        );
        let fixture = std::env::var("BARTJIK_CARD_FIXTURE").unwrap_or_default();
        let system = ["caffeine", "nightlight", "monitor-layout"].contains(&fixture.as_str());
        let mut system_card = None;
        let left = ["crypto", "playerctl", "workspaces"].contains(&fixture.as_str());
        let mut left_card = None;
        let history = std::env::var("BARTJIK_CARD_FIXTURE").as_deref() == Ok("history");
        let locale = std::env::var("BARTJIK_CARD_FIXTURE").as_deref() == Ok("locale");
        let mut history_card = None;
        let audio = std::env::var("BARTJIK_CARD_FIXTURE").as_deref() == Ok("audio");
        let mut audio_card = None;
        let connectivity = std::env::var("BARTJIK_CARD_FIXTURE").as_deref() == Ok("connectivity");
        let mut vpn_card = None;
        let network = std::env::var("BARTJIK_CARD_FIXTURE").as_deref() == Ok("network");
        let fake_network = std::sync::Arc::new(crate::network::Fake::demo());
        let mut network_card = None;
        let codex = std::env::var("BARTJIK_CARD_FIXTURE").as_deref() == Ok("codex");
        let router = std::env::var("BARTJIK_CARD_FIXTURE").as_deref() == Ok("openrouter");
        if codex {
            let (send, _receive) = async_channel::bounded(8);
            let module = json!({"provider":"codex","panel":{"hover":true},"comparisonItems":[{"id":"quota","type":"text","text":{"value":"23%"},"tooltip":"raw account table"}]});
            let mut visual_style = style.clone();
            visual_style["fontSize"] = json!(14);
            let view = crate::comparison::Module::new(
                &module,
                &visual_style,
                gtk::Orientation::Horizontal,
                send,
            );
            assert!(
                !view.root.first_child().unwrap().has_tooltip(),
                "Hover card retained a GTK tooltip"
            );
        }
        let header = card_header(
            if left {
                match fixture.as_str() {
                    "crypto" => "BTC/USD  ·  67,432",
                    "playerctl" => "Music",
                    _ => "Workspace 7",
                }
            } else if system {
                match fixture.as_str() {
                    "caffeine" => "Idle & Sleep",
                    "nightlight" => "Night light",
                    _ => "Monitor Setup",
                }
            } else if history {
                "Notifications"
            } else if locale {
                "Input methods"
            } else if audio {
                "Audio"
            } else if connectivity {
                "Mullvad"
            } else if network {
                "Networking"
            } else if router {
                "OpenRouter"
            } else if codex {
                "Codex"
            } else {
                "World clock"
            },
            if system || left {
                &fixture
            } else if history {
                "DND"
            } else if locale {
                "locale"
            } else if audio {
                "audio"
            } else if connectivity {
                "mullvad"
            } else if network {
                "network"
            } else if router {
                "openrouter"
            } else if codex {
                "codex"
            } else {
                "datetime"
            },
        );
        if history || locale {
            let close = gtk::Button::new();
            close.add_css_class("panel-control");
            close.add_css_class("panel-close");
            close.set_child(Some(&control_symbol("close")));
            close.set_state_flags(gtk::StateFlags::PRELIGHT, false);
            header.append(&close);
        }
        let card = card_frame(&header);
        let first;
        if left {
            let (send, receive) = async_channel::bounded(128);
            let (actions, commands) = async_channel::bounded(128);
            let provider = match fixture.as_str() {
                "crypto" => "crypto",
                "playerctl" => "playerctl",
                _ => "workspaces",
            };
            let view = crate::left_card::fixture(provider, send, actions);
            let content = view.content();
            if ["playerctl", "workspaces"].contains(&provider) {
                header.set_visible(false);
                header.next_sibling().unwrap().set_visible(false);
                card.add_css_class("panel-edge-content");
            }
            if provider == "crypto" {
                if let Some(old) = header.first_child() {
                    header.remove(&old);
                }
                let path = crate::util::root().join("assets/icons/bitcoin.svg");
                header.prepend(&crate::icons::artwork(
                    &json!({"kind":"image","value":path}),
                    &style,
                    22,
                ));
            }
            first = content.body.clone();
            if provider == "workspaces" {
                let frame = gtk::Overlay::new();
                frame.set_child(Some(&content.body));
                let close = gtk::Button::new();
                close.add_css_class("panel-close");
                close.add_css_class("workspace-panel-close");
                close.set_state_flags(gtk::StateFlags::FOCUS_VISIBLE, false);
                close.set_child(Some(&workspace_close_art(&style, 9, 9)));
                close.set_halign(gtk::Align::End);
                close.set_valign(gtk::Align::Start);
                close.set_margin_top(0);
                close.set_margin_end(0);
                frame.add_overlay(&close);
                card.append(&frame);
            } else {
                card.append(&content.body);
            }
            left_card = Some((view, receive, commands));
        } else if system {
            let (send, receive) = async_channel::bounded(128);
            let provider = match fixture.as_str() {
                "caffeine" => "caffeine",
                "nightlight" => "nightlight",
                _ => "monitor-layout",
            };
            let view = crate::system_card::Card::new(provider, send);
            let data = crate::system_card::fixture(provider);
            view.update(
                data,
                json!({"barKey":"fixture","module":provider,"monitor":"synthetic"}),
                style.clone(),
            );
            let content = view.content();
            first = content.body.clone();
            card.append(&content.body);
            system_card = Some((view, receive));
        } else if history {
            let (send, _receive) = async_channel::bounded(32);
            let view = crate::history_card::Card::new(true, send);
            let content = view.content();
            first = content.body.clone();
            card.append(&content.body);
            history_card = Some(view);
        } else if locale {
            let (send, receive) = async_channel::bounded(8);
            let target = Target {
                key: "locale".into(),
                bar: "fixture".into(),
                module: "locale".into(),
                item: "".into(),
                output: "fixture".into(),
                side: "b".into(),
                bounds: Rect {
                    x: 0.,
                    y: 0.,
                    width: 420.,
                    height: 800.,
                },
                anchor: None,
                source: Rect {
                    x: 0.,
                    y: 0.,
                    width: 420.,
                    height: 800.,
                },
                bar_bounds: Rect {
                    x: 0.,
                    y: 0.,
                    width: 420.,
                    height: 800.,
                },
                screen: Rect {
                    x: 0.,
                    y: 0.,
                    width: 420.,
                    height: 800.,
                },
                options: json!({}),
                row: json!({"panel":{"rows":[{"id":"keyboard-us","label":"EN","detail":"English – US","active":true},{"id":"pinyin","label":"拼","detail":"Pinyin – Traditional Chinese"},{"id":"quick","label":"速","detail":"Quick (速成) – Traditional Chinese"}]}}),
                provider: "locale".into(),
                style: style.clone(),
                enabled: true,
            };
            let content = locale_content(&target, send);
            first = content.body.clone();
            card.append(&content.body);
            let row = content
                .body
                .first_child()
                .unwrap()
                .downcast::<gtk::Button>()
                .unwrap();
            assert!(row.has_css_class("current"));
            row.emit_clicked();
            let crate::Event::ConnectivityAction(action) = receive.try_recv().unwrap() else {
                panic!("missing locale action")
            };
            assert_eq!(action["selection"], "keyboard-us");
        } else if audio {
            let (send, _receive) = async_channel::bounded(32);
            let view = crate::audio_card::Card::new(true, send);
            let content = view.content(&style);
            first = content.body.clone();
            card.append(&content.body);
            audio_card = Some(view);
        } else if connectivity {
            let (send, receive) = async_channel::bounded(32);
            let view = crate::connectivity_card::Card::new(send);
            view.update(
                serde_json::from_str(include_str!("../../tests/fixtures/connectivity-card.json"))
                    .unwrap(),
            );
            let content = view.content();
            first = content.body.clone();
            card.append(&content.body);
            vpn_card = Some((view, receive));
        } else if network {
            let (send, _receive) = async_channel::bounded(8);
            let view = crate::network_card::Card::new(fake_network.clone(), send);
            let content = view.content();
            first = content.body.clone();
            card.append(&content.body);
            network_card = Some(view);
        } else if router {
            let mut row: Value =
                serde_json::from_str(include_str!("../../tests/fixtures/openrouter-card.json"))
                    .unwrap();
            if std::env::var_os("BARTJIK_EMPTY_USAGE").is_some() {
                row["panel"]["rows"] = json!([]);
                row["state"]["stale"] = json!(false);
                row["panel"]["activityNote"] = json!("Daily usage isn’t available for this key.");
            }
            let content = crate::openrouter_card::content(&row);
            first = content.body.clone();
            card.append(&content.body);
        } else if codex {
            let row: Value =
                serde_json::from_str(include_str!("../../tests/fixtures/codex-card.json")).unwrap();
            let content = crate::codex_card::content_at(&row, &style, 1000000., false);
            first = content.body.clone();
            card.append(&content.body);
        } else {
            let rows = gtk::Box::new(gtk::Orientation::Vertical, 6);
            let clock = clock_row("Hong Kong", "18:42", "Monday, 9 November");
            first = clock.clone().upcast::<gtk::Widget>();
            rows.append(&clock);
            rows.append(&clock_row("London", "10:42", "Monday, 9 November"));
            rows.append(&clock_row("New York", "05:42", "Monday, 9 November"));
            card.append(&rows);
        }
        let window = gtk::Window::builder()
            .title("Bartjik Panel Fixture")
            .decorated(false)
            .default_width(if fixture == "workspaces" {
                624
            } else if fixture == "monitor-layout" {
                520
            } else if codex || (left && fixture != "playerctl") {
                440
            } else {
                420
            })
            .resizable(false)
            .child(&card)
            .build();
        window.present();
        let context = glib::MainContext::default();
        let until = Instant::now() + std::time::Duration::from_secs(3);
        while Instant::now() < until {
            while context.pending() {
                context.iteration(false);
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        if let Some(view) = &network_card {
            crate::network_card::wide_values_fixture(view);
            card.allocate(420, window.height(), -1, None);
            crate::network_card::assert_controls_fit(view);
        }
        let bounds = first.compute_bounds(&window).unwrap();
        let card_bounds = card.compute_bounds(&window).unwrap();
        assert_eq!(
            card_bounds.width(),
            if fixture == "workspaces" {
                624.
            } else if fixture == "monitor-layout" {
                520.
            } else if codex || (left && fixture != "playerctl") {
                440.
            } else {
                420.
            }
        );
        if !codex
            && !router
            && !network
            && !connectivity
            && !audio
            && !history
            && !locale
            && !system
            && !left
        {
            assert_eq!(first.height(), 56);
        }
        if ["playerctl", "workspaces"].contains(&fixture.as_str()) {
            assert_eq!(bounds.x(), 1.);
            assert_eq!(bounds.y(), 1.);
        } else {
            assert_eq!(header.height(), 32);
            assert_eq!(bounds.x(), 16.);
            assert_eq!(bounds.y(), 72.);
        }
        if !router && !network && !connectivity && !audio && !history && !locale && !system && !left
        {
            assert_eq!(
                card_bounds.height(),
                if codex {
                    529.
                } else {
                    (88 + 3 * 56 + 2 * 6) as f32
                }
            );
        }
        let snap = gtk::Snapshot::new();
        // Snapshot the manually allocated card directly: inactive workspaces may
        // not repaint the window's cached WidgetPaintable after a traffic update.
        window.snapshot_child(&card, &snap);
        let node = snap.to_node().expect("rendered card");
        let texture = window.renderer().unwrap().render_texture(&node, None);
        texture
            .save_to_png(std::env::var("BARTJIK_PANEL_ARTIFACT").unwrap())
            .unwrap();
        if fixture == "workspaces" {
            crate::thumbnail::comparison(&window.renderer().unwrap());
            // Capture only this synthetic fixture, never arbitrary user windows.
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let desktop = tjik_desktop::selected();
                let snapshot = desktop.snapshot().unwrap();
                let target = snapshot
                    .windows
                    .iter()
                    .find(|w| {
                        w.pid == Some(std::process::id() as u64)
                            && w.title.as_deref() == Some("Bartjik Panel Fixture")
                    })
                    .expect("fixture window");
                assert!(matches!(
                    desktop.capture(&tjik_desktop::WindowId("stale".into())),
                    Err(tjik_desktop::DesktopError::StaleTarget)
                ));
                tx.send(desktop.capture(&target.id)).unwrap();
            });
            let deadline = Instant::now() + std::time::Duration::from_secs(4);
            let frame = loop {
                while gtk::glib::MainContext::default().pending() {
                    gtk::glib::MainContext::default().iteration(false);
                }
                if let Ok(result) = rx.try_recv() {
                    break result.expect("real fixture thumbnail");
                }
                assert!(Instant::now() < deadline, "capture deadline");
                std::thread::sleep(std::time::Duration::from_millis(10));
            };
            assert!(frame.width > 0 && frame.width <= 3840 && frame.height <= 2160);
            assert_eq!(frame.rgba.len(), frame.stride * frame.height as usize);
            assert!(
                frame.rgba.chunks_exact(4).any(|p| p != &frame.rgba[..4]),
                "nonblank thumbnail"
            );
        }
        if let Some((view, receive, commands)) = left_card {
            crate::left_card::exercise(&view, &receive, &commands);
        }
        if let Some((view, receive)) = system_card {
            crate::system_card::exercise_fixture(&view, &receive);
        }
        if let Some(view) = history_card {
            crate::history_card::exercise_fixture(&view);
        }
        if let Some(view) = audio_card {
            crate::audio_card::exercise_fixture(&view);
        }
        if let Some((view, receive)) = vpn_card {
            crate::connectivity_card::exercise_fixture(&view, &receive);
        }
        if let Some(card) = network_card {
            crate::network_card::exercise_fixture(&card, &fake_network);
        }
        window.close();
    }
}
