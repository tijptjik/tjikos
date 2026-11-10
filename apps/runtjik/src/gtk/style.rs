//! Runtjik-specific view styling composed with shared GTK primitives.
pub use tjik_gtk::theme::{provider, Theme};

pub fn apply(provider: &gtk::CssProvider, theme: &Theme, font: &str, size: i32) {
    let size = size.clamp(8, 32);
    let views = format!(
        r#"
.tjik .word-caption {{ color: @tjik_muted; font-size: {title_size}px; }}
.tjik .word-heading {{ font-weight: bold; font-size: {search_size}px; }}
.tjik .word-locale {{ border: 1px solid @tjik_border; border-radius: 5px; padding: 2px 5px; color: @tjik_muted; font-size: 10px; }}
.tjik .word-watermark {{ color: @tjik_surface; font-size: 72px; }}
.tjik .word-example {{ color: @tjik_muted; font-style: italic; }}
.tjik .word-sense {{ padding: 16px 16px 16px 8px; border-radius: 10px; }}
.tjik .word-stripe {{ background: transparent; border-radius: 1px; }}
.tjik .word-selected .word-stripe {{ background: @tjik_accent; }}
.tjik .word-selected {{ background: @tjik_surface; }}
.tjik .recent-word.word-sense {{ padding: 10px 14px; border: 1px solid transparent; border-radius: 9px; min-height: 24px; }}
.tjik .recent-word:hover {{ background: alpha(@tjik_surface, .5); }}
.tjik .recent-word.word-selected {{ background: @tjik_surface; border-color: @tjik_accent; }}
.tjik .recent-action {{ opacity: 0; color: @tjik_accent; }}
.tjik .recent-word.word-selected .recent-action {{ opacity: 1; }}
.tjik button.word-provider {{ background: transparent; opacity: .5; padding: 6px 10px; }}
.tjik button.word-provider:checked {{ background: @tjik_surface; border-color: @tjik_border; opacity: 1; }}
.tjik button.word-term, .tjik button.word-source {{ border: 0; background: transparent; color: @tjik_accent; padding: 4px 7px; }}
.tjik button.word-source {{ color: @tjik_muted; }}
.tjik .capture-choice {{ border: 1px solid transparent; border-radius: 12px; padding: 8px; }}
.tjik .capture-choice .word-caption {{ font-size: 11px; }}
.tjik .capture-choice.word-selected {{ border-color: @tjik_accent; background: @tjik_surface; }}
"#,
        title_size = size - 1,
        search_size = size + 1
    );
    tjik_gtk::theme::apply_with_css(
        provider,
        theme,
        font,
        size,
        &format!("{}{}", views, tasktjik_gtk::style::CSS),
    );
}
