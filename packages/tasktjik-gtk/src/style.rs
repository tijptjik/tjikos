//! Tasktjik owns its palette, typography and spacing in every host. Launcher themes
//! must not silently turn the embedded application into a different presentation.
pub const CSS: &str = r#"
.tjik .shell.task-host { padding: 0; background: #26233a; }
.tjik .tasktjik { background: #26233a; color: #f5f5f7; padding: 32px; border-radius: 12px; font-size: 14px; }
.tjik .tasktjik * { font-family: sans-serif; }
.tjik .tasktjik .muted { color: #aaa5ba; font-size: 12px; }
.tjik .tasktjik .accent { color: #a8c7a0; }
.tjik .tasktjik .task-heading, .tjik .tasktjik .reading-heading { font-size: 24px; font-weight: 600; }
.tjik .tasktjik .domain-heading { color: #f5f5f7; font-size: 13px; font-weight: bold; letter-spacing: .4px; }
.tjik .tasktjik .task-date { font-family: monospace; font-size: 11px; color: #aaa5ba; min-width: 28px; }
.tjik .tasktjik .task-selected .task-title { color: #a8c7a0; }
.tjik .tasktjik .task-title { font-size: 15px; font-weight: 600; }
.tjik .tasktjik .card { background: #39354b; border: 1px solid #555169; border-radius: 10px; padding: 10px; }
.tjik .tasktjik .task-selected { background: #39354b; border-color: #a8c7a0; }
.tjik .tasktjik button { color: #f5f5f7; background: transparent; border: 1px solid #555169; border-radius: 9px; padding: 6px 10px; }
.tjik .tasktjik button:hover, .tjik .tasktjik button:checked { background: #39354b; border-color: #a8c7a0; }
.tjik .tasktjik button.flat { border-color: transparent; }
.tjik .tasktjik entry, .tjik .tasktjik textview, .tjik .tasktjik textview text { color: #f5f5f7; background: #26233a; border-color: #555169; }
.tjik .tasktjik entry:focus { border-color: #a8c7a0; }
.tjik .tasktjik checkbutton check:checked { background: #a8c7a0; color: #26233a; }
.tjik .tasktjik .pane { background: #26233a; padding: 0; }
.tjik .tasktjik .task-editor-title { font-size: 18px; }
.tjik .tasktjik .reading { padding: 0; }
.tjik .tasktjik .reading-title { font-size: 16px; font-weight: bold; }
.tjik .tasktjik .reading-card { background: #39354b; border: 1px solid #555169; border-radius: 12px; padding: 8px; }
.tjik .tasktjik .reading-card.task-selected { border-color: #a8c7a0; }
.tjik .tasktjik .reading-art { background: #26233a; border-radius: 8px; }
/* Original Tasktjik controls: quiet navigation and compact, unboxed rows. */
.tjik .tasktjik button { min-height: 24px; padding: 6px 13px; font-size: 13px; font-weight: normal; border-radius: 10px; box-shadow: none; background-image: none; }
.tjik .tasktjik button:hover { background: #39354b; border-color: #555169; }
.tjik .tasktjik button:active { background: #555169; }
.tjik .tasktjik button:focus-visible { outline: none; border-color: #a8c7a0; }
.tjik .tasktjik button.primary { background: #a8c7a0; color: #26233a; border-color: #a8c7a0; }
.tjik .tasktjik button.primary:hover { background: #b9d5b1; }
.tjik .tasktjik button.primary:disabled { opacity: .45; }
.tjik .tasktjik button.flat { border-color: transparent; }
.tjik .tasktjik button.flat:checked { border-color: #555169; background: #39354b; }
.tjik .tasktjik dropdown > button { background: #39354b; min-width: 92px; }
.tjik .tasktjik .icon-button { min-width: 24px; padding: 6px; border-color: transparent; border-radius: 8px; }
.tjik .tasktjik .icon-button:hover { background: #555169; border-color: transparent; }
.tjik .tasktjik .icon-button:checked { color: #a8c7a0; background: #39354b; }
/* Keep geometry identical across pointer and keyboard selection states. */
.tjik .tasktjik .task-card, .tjik .tasktjik .task-card:hover, .tjik .tasktjik .task-card.task-selected { background: transparent; border: 0; border-radius: 4px; padding: 6px 4px; }
.tjik .tasktjik .task-card:focus { outline: none; }
.tjik .tasktjik .row-action, .tjik .tasktjik .row-action > button { border: 0; min-height: 20px; min-width: 20px; padding: 0; border-radius: 4px; }
.tjik .tasktjik .domain-bar button { color: #aaa5ba; padding: 0; min-height: 24px; min-width: 24px; border: 0; border-radius: 4px; }
.tjik .tasktjik .domain-bar button:hover { color: #a8c7a0; }
.tjik .tasktjik .domain-heading { font-size: 14px; letter-spacing: 0; }
.tjik .tasktjik button.task-tag { color: #a8c7a0; font-size: 11px; padding: 0; min-height: 14px; border: 0; border-radius: 2px; }
.tjik .tasktjik .task-editor-title { min-height: 46px; padding: 0 10px; }
.tjik .tasktjik flowboxchild { padding: 0; }
.tjik .tasktjik .empty-drop-target { padding: 12px 20px; border: 1px dashed #555169; border-radius: 8px; color: #aaa5ba; }
.tjik .tasktjik .drop-target { box-shadow: inset 0 2px #a8c7a0; }
.tjik .tasktjik .task-footer button { border: 0; outline: none; box-shadow: none; background: transparent; min-width: 26px; min-height: 26px; padding: 6px; border-radius: 8px; }
.tjik .tasktjik .task-footer button:hover { background: #555169; }
.tjik .tasktjik .task-footer button:focus-visible { background: #39354b; color: #a8c7a0; }
.tjik .tasktjik button.completion-marker, .tjik .tasktjik button.completion-marker:hover, .tjik .tasktjik button.completion-marker:active { min-width: 28px; min-height: 0; padding: 0; margin: 0; border: 0; background: transparent; box-shadow: none; }
.tjik .tasktjik button.completion-marker:disabled { opacity: 1; }
.tjik .tasktjik .subtask-row { padding: 4px 0; border-radius: 4px; }
.tjik .tasktjik .subtask-row:hover { background: #39354b; }
.tjik .tasktjik .subtask-row:focus { outline: none; }
.tjik .tasktjik .subtask-title { font-size: 13px; }
.tjik .tasktjik .subtask-row:focus .subtask-title { color: #a8c7a0; }
.tjik .tasktjik .subtask-symbol { font-size: 16px; }
.tjik .tasktjik .subtask-exit { color: #eb6f92; }
.tjik .tasktjik .subtask-reminder { color: #f6c177; }
.tjik .tasktjik button.add-subtask { min-width: 26px; min-height: 26px; padding: 0; border: 0; color: #aaa5ba; background: transparent; border-radius: 7px; }
.tjik .tasktjik button.add-subtask:hover { color: #a8c7a0; background: alpha(#a8c7a0,.09); }
.tjik .tasktjik button.subtask-pips { min-height: 6px; padding: 4px 0; border: 0; background: transparent; }
.tjik .tasktjik .task-title-row button.add-subtask { min-height: 21px; }
.tjik .tasktjik .icon-button.filter-included { color: #a8c7a0; background: transparent; }
.tjik .tasktjik .icon-button.filter-included:hover { background: #39354b; }
.tjik .tasktjik .icon-button.filter-only { color: #ff53a6; background: transparent; }
.tjik .tasktjik .icon-button.filter-only:hover { background: alpha(#ff53a6,.24); }
.tjik .tasktjik .task-completed .task-title, .tjik .tasktjik .task-cancelled .task-title { color: #aaa5ba; }
.tjik .tasktjik .task-completed.task-selected .task-title { color: #a8c7a0; }
"#;
