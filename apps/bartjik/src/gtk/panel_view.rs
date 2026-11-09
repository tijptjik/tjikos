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
        "locale" => ("symbol", "keyboard"),
        "caffeine" => ("symbol", "coffee"),
        "monitor-layout" => ("symbol", "desktop_windows"),
        "pin" => ("symbol", "push_pin"),
        "close" => ("symbol", "close"),
        _ => ("symbol", "info"),
    };
    let widget: gtk::Widget = if kind == "image" {
        let image = gtk::Image::from_file(crate::util::root().join("qml").join(value));
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
.panel-card.panel-reveal-content {{background:transparent;border-color:transparent;}}
.panel-card .panel-symbol {{font-family:"Material Symbols Rounded";}}
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
pub fn status_content(target: &Target) -> Content {
    if target.provider == "codex" {
        return crate::codex_card::content(&target.row, &target.style);
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
    fixed: gtk::Fixed,
    card: gtk::Box,
    reveal: crate::panel_reveal::Reveal,
    header: gtk::Box,
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
        let tx = send.clone();
        close.connect_clicked(move |_| {
            let _ = tx.try_send(Event::PanelCommand("close".into()));
        });
        buttons.append(&close);
        let top = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        header.set_hexpand(true);
        top.append(&header);
        top.append(&buttons);
        let card = card_frame(&top);
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
        card.append(&scroll);
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
            window,
            fixed,
            card,
            reveal,
            header,
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
                if child.is::<gtk::Button>()
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
                let _ = send.try_send(Event::PanelCommand("detach".into()));
            }
        });
        // The fullscreen shell stays stationary while the card moves beneath it.
        self.window.add_controller(drag);
    }
    pub fn rect(&self) -> Rect {
        self.actual.get()
    }
    pub fn status(&self) -> Value {
        let allocated=self.reveal.compute_bounds(&self.window).map(|r|json!({"x":r.x() as f64+self.screen.get().x,"y":r.y() as f64+self.screen.get().y,"width":r.width(),"height":r.height()}));
        json!({"layoutSize":self.reveal.layout_size(),"allocation":allocated,"surfaceWidth":self.window.width(),"surfaceHeight":self.window.height(),"output":self.output,"rect":self.rect(),"target":self.target.get(),"visible":self.window.is_visible(),"inside":self.inside.get(),"transitioning":self.ticking.get(),"keyboard":format!("{:?}",self.window.keyboard_mode())})
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
            self.close.set_child(Some(&artwork("close", 20)));
            while let Some(c) = self.header.first_child() {
                self.header.remove(&c);
            }
            let content = content();
            self.preferred_width = content.preferred_width.clamp(200., 900.);
            self.needs_keyboard = content.needs_keyboard;
            self.header
                .append(&card_header(&content.title, &target.provider));
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
        self.window
            .set_keyboard_mode(if self.needs_keyboard || pinned {
                KeyboardMode::OnDemand
            } else {
                KeyboardMode::None
            });
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
    let x = r.x - screen.x;
    let y = r.y - screen.y;
    fixed.move_(reveal, x, y);
    reveal.frame(r.width.ceil() as i32, r.height.ceil() as i32, progress);
    if let Some(surface) = window.surface() {
        surface.set_input_region(&gtk::cairo::Region::create_rectangle(
            &gtk::cairo::RectangleInt::new(
                x.floor() as i32,
                y.floor() as i32,
                r.width.ceil() as i32,
                r.height.ceil() as i32,
            ),
        ));
        surface.queue_render();
    }
}

#[cfg(test)]
mod render_tests {
    use super::*;
    /// The Python runner installs a workspace-7 rule BEFORE starting this test.
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
        let network = std::env::var("BARTJIK_CARD_FIXTURE").as_deref() == Ok("network");
        let fake_network = std::sync::Arc::new(crate::network::Fake::demo());
        let mut network_card = None;
        let codex = std::env::var("BARTJIK_CARD_FIXTURE").as_deref() == Ok("codex");
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
            if network {
                "Networking"
            } else if codex {
                "Codex"
            } else {
                "World clock"
            },
            if network {
                "network"
            } else if codex {
                "codex"
            } else {
                "datetime"
            },
        );

        let card = card_frame(&header);
        let first;
        if network {
            let (send, _receive) = async_channel::bounded(8);
            let view = crate::network_card::Card::new(fake_network.clone(), send);
            let content = view.content();
            first = content.body.clone();
            card.append(&content.body);
            network_card = Some(view);
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
            .default_width(if codex { 440 } else { 420 })
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
        assert_eq!(card_bounds.width(), if codex { 440. } else { 420. });
        if !codex && !network {
            assert_eq!(first.height(), 56);
        }
        assert_eq!(header.height(), 32);
        assert_eq!(bounds.x(), 16.);
        assert_eq!(bounds.y(), 72.);
        if !network {
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

        if let Some(card) = network_card {
            crate::network_card::exercise_fixture(&card, &fake_network);
        }
        window.close();
    }
}
