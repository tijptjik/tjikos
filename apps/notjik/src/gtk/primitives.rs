//! Shared card typography, field rows, status treatments and action layout.
use super::model::*;
use gtk::{glib, prelude::*};
use serde_json::Value;

pub fn column(spacing: i32) -> gtk::Box {
    gtk::Box::new(gtk::Orientation::Vertical, spacing)
}
pub fn row(spacing: i32) -> gtk::Box {
    gtk::Box::new(gtk::Orientation::Horizontal, spacing)
}
/// Shared geometry for text and icon action footers.
pub fn action_footer(controls: &gtk::Box) -> gtk::Box {
    controls.set_height_request(30);
    let section = column(6);
    section.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    section.append(controls);
    section
}
pub fn label(value: &str, class: &str) -> gtk::Label {
    let w = gtk::Label::new(Some(value));
    w.set_xalign(0.);
    w.set_wrap(true);
    w.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    w.set_hexpand(true);
    w.set_max_width_chars(1);
    w.add_css_class(class);
    w
}
pub fn button(title: &str, name: &str) -> gtk::Button {
    let b = gtk::Button::with_label(title);
    b.set_widget_name(name);
    b.set_tooltip_text(Some(title));
    if let Some(label) = b.child().and_then(|w| w.downcast::<gtk::Label>().ok()) {
        label.set_wrap(true);
        label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        label.set_max_width_chars(22);
    }
    b
}
pub fn flow() -> gtk::FlowBox {
    let b = gtk::FlowBox::new();
    b.set_selection_mode(gtk::SelectionMode::None);
    b.set_min_children_per_line(1);
    b.set_max_children_per_line(12);
    b.set_row_spacing(6);
    b.set_column_spacing(6);
    b.set_homogeneous(false);
    b
}
pub fn append(flow: &gtk::FlowBox, w: &impl IsA<gtk::Widget>) {
    flow.insert(w, -1);
}
pub fn field_row(k: &str, v: &str, tone: &str, ingredient: bool) -> gtk::Box {
    let r = row(12);
    if ingredient {
        r.set_margin_start(12);
    }
    let k = label(
        &format!("{}{}", if ingredient { "↳ " } else { "" }, k),
        if ingredient { "muted" } else { "field-name" },
    );
    let v = label(v, if ingredient { "muted" } else { tone });
    v.set_xalign(1.);
    v.set_justify(gtk::Justification::Right);
    k.set_hexpand(true);
    v.set_hexpand(false);
    v.set_max_width_chars(16);
    k.set_valign(gtk::Align::Center);
    v.set_valign(gtk::Align::Center);
    r.append(&k);
    r.append(&v);
    r
}
pub fn rainbow(value: &str) -> String {
    let colors = [
        "#ff5f5f", "#ffaf00", "#5fff87", "#5fd7ff", "#5f87ff", "#d787ff",
    ];
    value
        .chars()
        .enumerate()
        .map(|(i, c)| {
            if c == ' ' {
                " ".into()
            } else {
                format!(
                    "<span foreground=\"{}\">{}</span>",
                    colors[i % 6],
                    glib::markup_escape_text(&c.to_string())
                )
            }
        })
        .collect()
}
pub fn status(d: &Value, default: &str, detail_key: &str) -> gtk::Box {
    let r = row(10);
    r.add_css_class("status-strip");
    let tone = match d["tone"].as_str() {
        Some("critical") => "critical",
        Some("success") => "success",
        _ => "accent",
    };
    let dot = gtk::DrawingArea::new();
    dot.set_content_width(6);
    dot.set_content_height(6);
    dot.set_valign(gtk::Align::Center);
    dot.add_css_class(tone);
    dot.set_draw_func(|widget, cr, w, h| {
        let ink = widget.color();
        cr.set_source_rgba(
            ink.red() as f64,
            ink.green() as f64,
            ink.blue() as f64,
            ink.alpha() as f64,
        );
        cr.arc(w as f64 / 2., h as f64 / 2., 3., 0., std::f64::consts::TAU);
        let _ = cr.fill();
    });
    r.append(&dot);
    let state = label(&fallback(&d["state"], default), tone);
    state.add_css_class("strong");
    state.set_valign(gtk::Align::Center);
    if d["rainbow-text"] == true {
        state.set_markup(&rainbow(&fallback(&d["state"], default)));
    }
    r.append(&state);
    let detail = label(&text(&d[detail_key]), "muted");
    detail.set_xalign(1.);
    detail.set_hexpand(false);
    detail.set_max_width_chars(16);
    detail.set_valign(gtk::Align::Center);
    r.append(&detail);
    r
}

pub fn apply_theme(style: &Value) -> gtk::CssProvider {
    fn color(style: &Value, key: &str, default: &str) -> String {
        gtk::gdk::RGBA::parse(style[key].as_str().unwrap_or(default))
            .unwrap_or_else(|_| gtk::gdk::RGBA::parse(default).unwrap())
            .to_string()
    }
    let font = fallback(&style["font"], "monospace")
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace(['\n', '\r'], " ");
    let provider = tjik_gtk::theme::provider();
    provider.load_from_string(&format!(r#"
.notjik {{ background: {bg}; color: {text}; font-family: "{font}"; font-size: {size}px; }}
.notjik .card-frame {{ background: {bg}; border: 1px solid {border}; border-radius: {radius}px; }}
.notjik .card-frame.selected, .notjik .card-frame:focus-within {{ border-color: {accent}; }}
.notjik .card-frame.urgent {{ border-color: {critical}; }}
.notjik .card-content {{ padding: {padding}px; }}
.notjik .card-content.compact-footer {{ padding-bottom: 8px; }}
.notjik .card-title {{ font-weight: bold; font-size: {title}px; }}
.notjik .strong, .notjik .field-name {{ font-weight: bold; }}
@define-color notjik_accent {accent};
.notjik .muted, .notjik .caption {{ color: {muted}; }}
.notjik .caption {{ font-size: {small}px; }}
.notjik .telegram-meta {{ color: {muted}; font-size: {meta}px; }}
.notjik .accent {{ color: {accent}; }}
.notjik .critical {{ color: {critical}; }}
.notjik .success {{ color: {success}; }}
.notjik .artwork {{ background: {surface}; }}
.notjik .initials {{ color: {accent}; font-size: 18px; font-weight: bold; }}
.notjik .status-strip {{ background: shade({bg}, .82); border-radius: 7px; padding: 10px; }}
.notjik .auth-panel {{ background: alpha({accent}, .09); border: 1px solid alpha({accent}, .35); border-radius: 6px; padding: 12px; }}
.notjik .auth-caption {{ font-size: 12px; }}
.notjik .auth-code {{ font-family: monospace; font-size: 30px; font-weight: bold; letter-spacing: 4px; }}
.notjik .countdown {{ color: {accent}; font-size: 22px; font-weight: bold; }}
.notjik button {{ background: transparent; color: {text}; border: 1px solid {border}; border-radius: 7px; padding: 5px 9px; box-shadow: none; }}
.notjik button:hover, .notjik button:checked {{ background: alpha({accent},.09); border-color: {accent}; }}
.notjik button:disabled {{ opacity: .4; }}
.notjik button:focus-visible {{ outline: 1px solid {accent}; outline-offset: 1px; }}
.notjik entry, .notjik textview, .notjik textview text {{ background: {surface}; color: {text}; border-radius: 7px; }}
.notjik entry {{ padding: 7px; border: 1px solid {border}; box-shadow: none; }}
.notjik textview {{ padding: 8px; }}
.notjik textview.message-parts, .notjik textview.message-parts text {{ background: transparent; padding: 0; }}
.notjik flowboxchild {{ padding: 0; background: transparent; }}
.notjik progressbar trough {{ min-height: 6px; background: {surface}; border-radius: 3px; }}
.notjik progressbar progress {{ min-height: 6px; background: {accent}; border-radius: 3px; }}
.notjik separator {{ background: {border}; min-height: 1px; }}
.notjik scale trough {{ background: {border}; }}
.notjik scale highlight {{ background: {accent}; }}
 .notjik button.quiet {{ color: {muted}; background: transparent; border: 0; border-radius: 4px; padding: 2px; min-height: 22px; min-width: 0; font-size: {small}px; }}
.notjik button.quiet:hover, .notjik button.quiet:focus-visible {{ color: {text}; background: transparent; }}
.notjik button.quiet:active {{ background: {surface}; }}
.notjik button.quiet:disabled {{ color: {border}; opacity: 1; }}
.notjik button.icon-action {{ padding: 5px; min-height: 20px; min-width: 20px; }}
.notjik button.voice-play {{ min-width: 42px; min-height: 42px; padding: 0; border: 0; border-radius: 999px; background: {accent}; }}
.notjik button.voice-play:active {{ background: shade({accent},.83); }}
.notjik button.voice-play:disabled {{ background: {surface}; opacity: 1; }}
.notjik button.voice-play:focus-visible {{ outline: 2px solid {text}; outline-offset: -2px; }}
.notjik .wave-seek {{ padding: 0; min-height: 30px; }}
.notjik .wave-seek trough, .notjik .wave-seek highlight {{ background: transparent; min-height: 2px; min-width: 0; border: 0; padding: 0; margin: 0; }}
.notjik .wave-seek slider {{ opacity: 0; background: {accent}; border: 0; box-shadow: none; min-width: 10px; min-height: 10px; margin: 0; }}
.notjik .wave-seek:hover slider, .notjik .wave-seek:focus-visible slider {{ opacity: 1; }}
.notjik .wave-seek:disabled slider {{ opacity: 0; }}
.notjik .interaction-hint {{ background: {border}; border-radius: 2px; }}
.notjik button.page-action {{ font-size: 16px; }}
.notjik button.auth-copy {{ min-height: 26px; padding: 5px 12px; border: 0; background: transparent; color: {muted}; font-size: {small}px; }}
.notjik button.auth-copy:hover {{ color: {text}; background: transparent; }}
.notjik button.auth-copy:active {{ background: {surface}; }}
.notjik button.auth-copy:focus-visible {{ outline: none; color: {text}; background: alpha({accent}, .09); }}
.notjik .status-strip {{ font-size: {small}px; }}
.notjik .gallery {{ padding: 20px; }}
"#,bg=color(style,"background","#191919"),surface=color(style,"surface","#242424"),text=color(style,"text","#eeeeee"),muted=color(style,"muted","#aaaaaa"),border=color(style,"border","#353535"),accent=color(style,"accent","#ffffff"),critical=color(style,"critical","#eb6f92"),success=color(style,"success","#9ccfd8"),size=number(&style["fontSize"]).clamp(10.,24.),title=number(&style["fontSize"]).clamp(10.,24.)+1.,small=number(&style["fontSize"]).clamp(10.,24.)-2.,meta=number(&style["fontSize"]).clamp(10.,24.)-3.,radius=number(&style["radius"]).clamp(0.,32.),padding=number(&style["padding"]).clamp(8.,32.)));
    provider
}

/// Translate the bridge's limited StyledText vocabulary into Pango. Unsupported
/// or malformed markup falls back to the original plain message, never raw HTML.
pub fn styled_markup(styled: &Value) -> Option<String> {
    let markup = styled
        .as_str()?
        .replace("<br>", "\n")
        .replace("<font face=\"monospace\">", "<tt>")
        .replace("</font>", "</tt>");
    gtk::pango::parse_markup(&markup, '\0').ok().map(|_| markup)
}
pub fn styled_label(plain: &str, styled: &Value) -> gtk::Label {
    let label = label(plain, "body");
    if let Some(markup) = styled_markup(styled) {
        label.set_markup(&markup);
    }
    label
}
