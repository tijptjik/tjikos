use gtk::gdk;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Theme {
    pub background: String,
    pub surface: String,
    pub tile: String,
    pub text: String,
    pub muted: String,
    pub accent: String,
    pub border: String,
    pub icon_theme: String,
}
impl Default for Theme {
    fn default() -> Self {
        Self {
            background: "#191724".into(),
            surface: "#26233a".into(),
            tile: "#1f1d2e".into(),
            text: "#e0def4".into(),
            muted: "#908caa".into(),
            accent: "#ff53a6".into(),
            border: "#403a52".into(),
            icon_theme: "rose-punk".into(),
        }
    }
}
fn color(value: &str, fallback: &str) -> String {
    gdk::RGBA::parse(value)
        .or_else(|_| gdk::RGBA::parse(fallback))
        .unwrap()
        .to_string()
}
pub fn apply(provider: &gtk::CssProvider, theme: &Theme, font: &str, size: i32) {
    let t = theme;
    let font = font
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace(['\n', '\r'], " ");
    provider.load_from_string(&format!(r#"
window.tjik {{ background: transparent; color: {text}; font-family: "{font}"; font-size: {size}px; }}
.tjik * {{ font-family: "{font}"; }}
.tjik .shell {{ background: {surface}; border: 1px solid {border}; border-radius: 22px; padding: 19px 19px 23px; }}
.tjik .pane {{ background: {background}; border-radius: 14px; padding: 8px; }}
.tjik entry {{ background: {background}; color: {text}; border: 1px solid {border}; box-shadow: none; outline: none; border-radius: 8px; padding: 6px 10px; min-height: 24px; }}
.tjik entry:focus {{ border-color: {accent}; }}
.tjik entry.search {{ border-radius: 23px; min-height: 44px; padding: 0 17px; font-size: {search_size}px; }}
.tjik entry.search:focus {{ border-color: {border}; box-shadow: none; }}
.tjik entry placeholder, .tjik .muted {{ color: {muted}; }}
.tjik .accent {{ color: {accent}; }}
.tjik .command-symbol {{ color: {accent}; font-size: 28px; min-width: 28px; }}
.tjik listview, .tjik gridview {{ background: transparent; }}
.tjik listview row, .tjik gridview child {{ padding: 0; margin: 0; background: transparent; border: 0; outline: none; box-shadow: none; }}
.tjik .result {{ border: 1px solid transparent; border-radius: 12px; margin: 0 6px 6px 0; }}
.tjik row:selected .result, .tjik child:selected .result {{ background: {surface}; border-color: {accent}; }}
.tjik .scrolling row:selected .result, .tjik .scrolling child:selected .result, .tjik .scrolling .result:hover {{ background: transparent; border-color: transparent; }}
.tjik .result-title {{ font-size: {title_size}px; }}
.tjik button {{ background: transparent; color: {text}; border: 1px solid {border}; border-radius: 8px; padding: 5px 9px; box-shadow: none; }}
.tjik button:hover, .tjik button:checked {{ background: {surface}; border-color: {accent}; }}
.tjik button.flat {{ border-color: transparent; }}
.tjik textview, .tjik textview text {{ background: {background}; color: {text}; }}
.tjik .card {{ background: {background}; border: 1px solid {border}; border-radius: 12px; padding: 12px; }}
.tjik .task-selected {{ border-color: {accent}; }}
.tjik .error {{ color: {accent}; padding: 8px; }}
.tjik scrollbar, .tjik scrollbar trough {{ background: transparent; border: 0; outline: none; box-shadow: none; }}
.tjik scrollbar slider {{ border: 0; box-shadow: none; min-width: 3px; min-height: 24px; background: alpha({muted}, .4); border-radius: 3px; }}
.tjik .word-caption {{ color: {muted}; font-size: {title_size}px; }}
.tjik .word-heading {{ font-weight: bold; font-size: {search_size}px; }}
.tjik .word-locale {{ border: 1px solid {border}; border-radius: 5px; padding: 2px 5px; color: {muted}; font-size: 10px; }}
.tjik .word-watermark {{ color: {surface}; font-size: 72px; }}
.tjik .word-example {{ color: {muted}; font-style: italic; }}
.tjik .word-sense {{ padding: 16px 16px 16px 8px; border-radius: 10px; }}
.tjik .word-stripe {{ background: transparent; border-radius: 1px; }}
.tjik .word-selected .word-stripe {{ background: {accent}; }}
.tjik .word-selected {{ background: {surface}; }}
.tjik button.word-provider {{ background: transparent; opacity: .5; padding: 6px 10px; }}
.tjik button.word-provider:checked {{ background: {surface}; border-color: {border}; opacity: 1; }}
.tjik button.word-term, .tjik button.word-source {{ border: 0; background: transparent; color: {accent}; padding: 4px 7px; }}
.tjik button.word-source {{ color: {muted}; }}
.tjik .capture-choice {{ border: 1px solid transparent; border-radius: 12px; padding: 8px; }}
.tjik .capture-choice .word-caption {{ font-size: 11px; }}
.tjik .capture-choice.word-selected {{ border-color: {accent}; background: {surface}; }}
.tjik .reading {{ padding: 14px; }}
.tjik .reading, .tjik .reading * {{ font-family: sans-serif; }}
.tjik .reading .muted {{ font-size: 12px; }}
.tjik .reading .accent {{ font-size: 11px; }}
.tjik .reading-heading {{ font-size: 24px; font-weight: bold; }}
.tjik .reading-title {{ font-size: 16px; font-weight: bold; }}
.tjik .reading-card {{ background: {surface}; border: 1px solid {border}; border-radius: 12px; padding: 8px; }}
.tjik .reading-card.task-selected {{ border-color: {accent}; }}
.tjik .reading-art {{ background: {background}; border-radius: 8px; }}
.tjik .grip {{ color: {muted}; }}
"#, background=color(&t.background,"#191724"), surface=color(&t.surface,"#26233a"), text=color(&t.text,"#e0def4"), muted=color(&t.muted,"#908caa"), accent=color(&t.accent,"#ff53a6"), border=color(&t.border,"#403a52"), size=size.clamp(8,32), search_size=size.clamp(8,32)+1, title_size=size.clamp(8,32)-1));
    if let Some(display) = gdk::Display::default() {
        gtk::Settings::for_display(&display).set_gtk_icon_theme_name(Some(&theme.icon_theme));
    }
}
pub fn provider() -> gtk::CssProvider {
    let provider = gtk::CssProvider::new();
    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
    provider
}
