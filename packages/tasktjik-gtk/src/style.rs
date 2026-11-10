//! Styles owned by task and reading views, appended by their host application.
pub const CSS: &str = r#"
.tjik .task-selected { border-color: @tjik_accent; }
.tjik .reading { padding: 14px; }
.tjik .reading, .tjik .reading * { font-family: sans-serif; }
.tjik .reading .muted { font-size: 12px; }
.tjik .reading .accent { font-size: 11px; }
.tjik .reading-heading { font-size: 24px; font-weight: bold; }
.tjik .reading-title { font-size: 16px; font-weight: bold; }
.tjik .reading-card { background: @tjik_surface; border: 1px solid @tjik_border; border-radius: 12px; padding: 8px; }
.tjik .reading-card.task-selected { border-color: @tjik_accent; }
.tjik .reading-art { background: @tjik_background; border-radius: 8px; }
"#;
