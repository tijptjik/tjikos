//! Reusable GTK card renderers. Notification admission, D-Bus and action execution
//! remain host responsibilities. The synthetic gallery is one such host.
mod actions;
pub mod clipboard;
mod desktop_bus;
mod details;
mod executor;
pub mod fixtures;
pub mod host;
mod icons;
mod keyboard;
mod media;
pub mod model;
pub mod primitives;
mod stack;
mod telegram;
use gtk::{gdk, glib, prelude::*};
pub use media::MediaStatus;
use model::*;
use primitives::*;
use std::{
    cell::{Cell, RefCell},
    path::{Path, PathBuf},
    rc::Rc,
};

type ReplyTarget = (String, String, String);
struct Context {
    notice: RefCell<Notice>,
    state: RefCell<ViewState>,
    now: Cell<f64>,
    callback: Rc<dyn Fn(CardEvent)>,
    draft: RefCell<String>,
    cursor: Cell<i32>,
    action_page: Cell<usize>,
    editor: RefCell<glib::WeakRef<gtk::TextView>>,
    composing: Cell<bool>,
    quoted: Cell<bool>,
    snoozing: Cell<bool>,
    reply_target: RefCell<Option<ReplyTarget>>,
    timer: RefCell<Option<(gtk::Label, gtk::Label)>>,
    voice: RefCell<Option<telegram::Voice>>,
    media: RefCell<Vec<media::Media>>,
    hovered: Cell<bool>,
    dragging: Cell<bool>,
    focused: Cell<bool>,
    revealers: RefCell<Vec<glib::WeakRef<gtk::Revealer>>>,
    quiet: RefCell<Vec<(glib::WeakRef<gtk::Widget>, bool)>>,
    hint: RefCell<glib::WeakRef<gtk::Widget>>,
}
impl Context {
    fn revealed(&self) -> bool {
        let state = self.state.borrow();
        self.hovered.get()
            || self.dragging.get()
            || self.focused.get()
            || state.selected
            || self.composing.get()
            || state.busy
            || !["", "idle", "error"].contains(&state.recording.phase.as_str())
    }
    fn sync_reveal(&self) {
        let state = self.state.borrow();
        let reveal = self.revealed()
            && !state.locked
            && !state.paused
            && !state.history
            && !state.blocked
            && !self.notice.borrow().archived();
        for r in self.revealers.borrow().iter().filter_map(|r| r.upgrade()) {
            if reveal {
                r.set_visible(true);
            }
            r.set_reveal_child(reveal);
            r.set_sensitive(reveal);
        }
        for (w, enabled) in self.quiet.borrow().iter() {
            if let Some(w) = w.upgrade() {
                w.set_opacity(if reveal { 1. } else { 0. });
                w.set_sensitive(reveal && *enabled);
                w.set_can_target(reveal);
            }
        }
        if let Some(hint) = self.hint.borrow().upgrade() {
            hint.set_visible(!reveal && (!self.revealers.borrow().is_empty() || state.floating));
        }
    }
    fn reveal(&self, child: &impl IsA<gtk::Widget>, name: &str) -> gtk::Revealer {
        let r = gtk::Revealer::new();
        r.set_widget_name(name);
        r.set_transition_type(gtk::RevealerTransitionType::SlideDown);
        r.set_transition_duration(160);
        r.set_visible(false);
        r.connect_child_revealed_notify(|r| {
            if !r.reveals_child() && !r.is_child_revealed() {
                r.set_visible(false);
            }
        });
        if matches!(name, "card-actions-reveal" | "telegram-controls-reveal") {
            r.connect_visible_notify(|r| {
                if let Some(content) = r.parent() {
                    // Keep body padding when collapsed; the revealed footer needs
                    // only a small bottom inset around its existing hit targets.
                    if r.is_visible() {
                        content.add_css_class("compact-footer");
                    } else {
                        content.remove_css_class("compact-footer");
                    }
                }
            });
        }
        r.set_child(Some(child));
        self.revealers.borrow_mut().push(r.downgrade());
        r
    }
    fn quiet(&self, widget: &impl IsA<gtk::Widget>) {
        let w = widget.as_ref();
        self.quiet
            .borrow_mut()
            .push((w.downgrade(), w.is_sensitive()));
    }
    fn allowed(&self) -> bool {
        self.state.borrow().interactive(&self.notice.borrow())
    }
    fn emit(&self, intent: Intent) {
        let passive = matches!(intent, Intent::Hover(_) | Intent::Hold(false));
        if !passive && !self.allowed() {
            return;
        }
        {
            let state = self.state.borrow();
            match &intent {
                Intent::RecordSend
                    if state.recording.uncertain
                        || !(state.recording.phase == "ready"
                            || (state.recording.phase == "recording"
                                && state.recording.duration_ms > 0)) =>
                {
                    return
                }
                Intent::RecordDiscard
                    if ["sending", "discarding"].contains(&state.recording.phase.as_str()) =>
                {
                    return
                }
                Intent::Seek(_) if !state.playback.seekable => return,
                _ => {}
            }
        }
        let event = CardEvent {
            id: self.notice.borrow().id(),
            intent,
        };
        (self.callback)(event);
    }
    fn telegram(&self, action: &str, body: &str, quote: bool, target: Option<&ReplyTarget>) {
        let (chat_id, message_id) = {
            let n = self.notice.borrow();
            target
                .map(|t| (t.0.clone(), t.1.clone()))
                .unwrap_or_else(|| (text(&n.data()["chatId"]), text(&n.data()["messageId"])))
        };
        self.emit(Intent::Telegram {
            action: action.into(),
            text: body.into(),
            quote,
            chat_id,
            message_id,
        });
    }
    fn tick_timer(&self) {
        if let Some((digits, caption)) = self.timer.borrow().as_ref() {
            let n = self.notice.borrow();
            let seconds = n.remaining(self.now.get());
            if digits.text() != stamp(seconds) {
                digits.set_text(&stamp(seconds));
            }
            let text = fallback(
                &n.data()[if seconds == 0 { "doneLabel" } else { "label" }],
                if seconds == 0 { "Ready" } else { "Remaining" },
            );
            if caption.text() != text {
                caption.set_text(&text);
            }
        }
    }
}
/// A stable outer widget. Replacement preserves drafts for the same notification
/// identity; a new identity clears them. The host supplies wall-clock ticks.
pub struct Card {
    pub widget: gtk::Box,
    context: Rc<Context>,
    root: PathBuf,
}
impl Card {
    pub fn new(
        notice: Notice,
        state: ViewState,
        root: &Path,
        now: f64,
        callback: impl Fn(CardEvent) + 'static,
    ) -> Self {
        let widget = column(0);
        widget.add_css_class("card-frame");
        widget.set_focusable(true);
        widget.set_vexpand(false);
        widget.set_valign(gtk::Align::Start);
        widget.set_overflow(gtk::Overflow::Hidden);
        let context = Rc::new(Context {
            notice: RefCell::new(notice),
            state: RefCell::new(state),
            now: Cell::new(now),
            callback: Rc::new(callback),
            draft: RefCell::new(String::new()),
            cursor: Cell::new(0),
            action_page: Cell::new(0),
            editor: RefCell::new(glib::WeakRef::new()),
            composing: Cell::new(false),
            quoted: Cell::new(false),
            snoozing: Cell::new(false),
            reply_target: RefCell::new(None),
            timer: RefCell::new(None),
            voice: RefCell::new(None),
            media: RefCell::new(vec![]),
            hovered: Cell::new(false),
            dragging: Cell::new(false),
            focused: Cell::new(false),
            revealers: RefCell::new(vec![]),
            quiet: RefCell::new(vec![]),
            hint: RefCell::new(glib::WeakRef::new()),
        });
        let card = Self {
            widget,
            context,
            root: root.into(),
        };
        card.gestures();
        card.render();
        card
    }
    pub fn update(&self, notice: Notice) {
        if self.context.notice.borrow().id() != notice.id() {
            self.clear_reply();
            self.context.snoozing.set(false);
        }
        *self.context.notice.borrow_mut() = notice;
        self.render();
    }
    pub fn set_state(&self, state: ViewState) {
        let mut previous = self.state();
        let playing_changed = previous.playback.playing != state.playback.playing;
        previous.playback = state.playback.clone();
        previous.selected = state.selected;
        let incremental = previous == state;
        *self.context.state.borrow_mut() = state;
        if incremental {
            if self.context.state.borrow().selected {
                self.widget.add_css_class("selected");
            } else {
                self.widget.remove_css_class("selected");
            }
            if let Some(voice) = self.context.voice.borrow().as_ref() {
                voice.update(&self.context, playing_changed);
            }
            self.context.sync_reveal();
        } else {
            self.render();
        }
    }
    /// Pause decorative animation for reduced motion, capture, or hidden hosts.
    pub fn media_status(&self) -> Vec<MediaStatus> {
        self.context
            .media
            .borrow()
            .iter()
            .map(|m| m.status())
            .collect()
    }
    pub fn set_animations_active(&self, active: bool) {
        let state = self.context.state.borrow();
        for media in self.context.media.borrow().iter() {
            media.set_active(active && !state.locked && !state.paused && !state.history);
        }
    }
    pub fn set_hovered(&self, hovered: bool) {
        self.context.hovered.set(hovered);
        self.context.sync_reveal();
    }
    pub fn tick(&self, now: f64) {
        self.context.now.set(now);
        self.context.tick_timer();
    }
    pub fn clear_reply(&self) {
        self.context.draft.borrow_mut().clear();
        self.context.cursor.set(0);
        self.context.composing.set(false);
        self.context.quoted.set(false);
        self.context.reply_target.borrow_mut().take();
    }
    pub(crate) fn copy_interaction_from(&self, other: &Card) {
        *self.context.draft.borrow_mut() = other.context.draft.borrow().clone();
        self.context.cursor.set(other.context.cursor.get());
        self.context.composing.set(other.context.composing.get());
        self.context.quoted.set(other.context.quoted.get());
        *self.context.reply_target.borrow_mut() = other.context.reply_target.borrow().clone();
        self.context
            .action_page
            .set(other.context.action_page.get());
        self.render();
    }
    pub fn draft(&self) -> String {
        self.context.draft.borrow().clone()
    }
    pub fn notice(&self) -> Notice {
        self.context.notice.borrow().clone()
    }
    pub fn state(&self) -> ViewState {
        self.context.state.borrow().clone()
    }
    fn gestures(&self) {
        let motion = gtk::EventControllerMotion::new();
        let c = self.context.clone();
        motion.connect_enter(move |_, _, _| {
            c.hovered.set(true);
            c.sync_reveal();
            c.emit(Intent::Hover(true));
        });
        let c = self.context.clone();
        motion.connect_leave(move |_| {
            c.hovered.set(false);
            c.sync_reveal();
            c.emit(Intent::Hover(false));
        });
        self.widget.add_controller(motion);
        let focus = gtk::EventControllerFocus::new();
        let c = self.context.clone();
        focus.connect_enter(move |_| {
            c.focused.set(true);
            c.sync_reveal();
        });
        let c = self.context.clone();
        focus.connect_leave(move |_| {
            c.focused.set(false);
            c.sync_reveal();
        });
        self.widget.add_controller(focus);
        // One recognizer owns plain-surface clicks and drags, so crossing the
        // drag threshold cannot also deliver a background dismissal.
        let drag = gtk::GestureDrag::new();
        drag.set_button(1);
        drag.set_propagation_phase(gtk::PropagationPhase::Capture);
        let candidate = Rc::new(Cell::new(false));
        let active = candidate.clone();
        let c = self.context.clone();
        let weak = self.widget.downgrade();
        drag.connect_drag_begin(move |gesture, x, y| {
            active.set(false);
            c.dragging.set(false);
            let Some(root) = weak.upgrade() else {
                return;
            };
            if gesture
                .current_event_state()
                .contains(gdk::ModifierType::SHIFT_MASK)
            {
                if !c.state.borrow().locked {
                    root.clipboard()
                        .set_text(&clipboard::markdown(&c.notice.borrow(), c.now.get()));
                    root.set_tooltip_text(Some("Copied as Markdown"));
                }
                gesture.set_state(gtk::EventSequenceState::Claimed);
                return;
            }
            let mut target = root.pick(x, y, gtk::PickFlags::DEFAULT);
            while let Some(w) = target {
                if w.is::<gtk::Button>()
                    || w.is::<gtk::TextView>()
                    || w.is::<gtk::Range>()
                    || w.is::<gtk::Entry>()
                {
                    gesture.set_state(gtk::EventSequenceState::Denied);
                    return;
                }
                if w == root {
                    break;
                }
                target = w.parent();
            }
            if c.allowed() {
                active.set(true);
                if let Some(stack) = root.parent().and_downcast::<stack::Stack>() {
                    stack.grab(true);
                }
                gesture.set_state(gtk::EventSequenceState::Claimed);
            } else {
                gesture.set_state(gtk::EventSequenceState::Denied);
            }
        });
        let active = candidate.clone();
        let c = self.context.clone();
        drag.connect_drag_update(move |_, dx, dy| {
            if !active.get() || !c.allowed() || (!c.dragging.get() && dx.hypot(dy) < 8.) {
                return;
            }
            c.dragging.set(true);
            c.emit(Intent::Drag {
                dx,
                dy,
                finished: false,
            });
        });
        let c = self.context.clone();
        let weak = self.widget.downgrade();
        drag.connect_drag_end(move |_, dx, dy| {
            if !candidate.replace(false) {
                return;
            }
            if let Some(stack) = weak
                .upgrade()
                .and_then(|w| w.parent())
                .and_downcast::<stack::Stack>()
            {
                stack.grab(false);
            }
            if c.dragging.replace(false) {
                c.emit(Intent::Drag {
                    dx,
                    dy,
                    finished: true,
                });
                c.sync_reveal();
            } else if dx.hypot(dy) < 8. && !c.state.borrow().floating {
                c.emit(Intent::Dismiss);
            }
        });
        self.widget.add_controller(drag);
        let keys = gtk::EventControllerKey::new();
        let c = self.context.clone();
        keys.connect_key_pressed(move |_, key, _, _| {
            // Entry/textview keys are handled by their editor; only act when the
            // outer card itself owns focus.
            if key == gdk::Key::Delete {
                c.emit(Intent::Dismiss);
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        keys.set_propagation_phase(gtk::PropagationPhase::Target);
        self.widget.add_controller(keys);
    }
    fn render(&self) {
        let restore_editor_focus = self
            .context
            .editor
            .borrow()
            .upgrade()
            .is_some_and(|editor| editor.has_focus());
        while let Some(child) = self.widget.first_child() {
            self.widget.remove(&child);
        }
        self.context.timer.borrow_mut().take();
        self.context.voice.borrow_mut().take();
        self.context.media.borrow_mut().clear();
        self.context.revealers.borrow_mut().clear();
        self.context.quiet.borrow_mut().clear();
        let n = self.context.notice.borrow().clone();
        let state = self.context.state.borrow().clone();
        self.widget.set_widget_name(&format!("card-{}", n.id()));
        self.widget
            .set_visible((!state.locked || state.show_when_locked) && !state.paused);
        for (class, on) in [
            ("selected", state.selected),
            ("urgent", n.0["urgency"] == "critical"),
        ] {
            if on {
                self.widget.add_css_class(class)
            } else {
                self.widget.remove_css_class(class)
            }
        }
        let body = row(0);
        let rail = self.artwork(&n);
        body.append(&rail);
        let content = column(10);
        content.add_css_class("card-content");
        content.set_hexpand(true);
        let heading = row(8);
        let title = label(&n.title(), "card-title");
        title.set_lines(3);
        title.set_ellipsize(gtk::pango::EllipsizeMode::End);
        heading.append(&title);
        if state.group_count > 1 {
            let group = button(&state.group_count.to_string(), "card-group");
            let c = self.context.clone();
            group.connect_clicked(move |_| c.emit(Intent::OpenGroup));
            group.set_sensitive(self.context.allowed());
            heading.append(&group);
        }
        let close = button("×", "card-dismiss");
        let c = self.context.clone();
        close.connect_clicked(move |_| c.emit(Intent::Dismiss));
        close.set_sensitive(self.context.allowed());
        close.add_css_class("quiet");
        if state.floating {
            self.context.quiet(&close);
            heading.append(&close);
        }
        let text_header = column(if n.kind() == "telegram" { 3 } else { 7 });
        text_header.append(&heading);
        if n.kind() == "telegram" {
            let age = if self.context.now.get()
                - n.0["received"].as_f64().unwrap_or(self.context.now.get())
                < 60000.
            {
                "now".to_string()
            } else {
                chrono::DateTime::from_timestamp_millis(n.0["received"].as_i64().unwrap_or(0))
                    .map(|d| d.with_timezone(&chrono::Local).format("%H:%M").to_string())
                    .unwrap_or_default()
            };
            text_header.append(&label(&format!("Telegram · {age}"), "telegram-meta"));
        }
        if !n.body().is_empty() && !["key-value", "telegram"].contains(&n.kind()) {
            let body = label(&n.body(), "body");
            body.set_lines(6);
            body.set_ellipsize(gtk::pango::EllipsizeMode::End);
            text_header.append(&body);
        }
        content.append(&text_header);
        details::content(&self.context, &content);
        if let Some(actions) = actions::footer(&self.context) {
            content.append(&self.context.reveal(&actions, "card-actions-reveal"));
        }
        if !state.feedback.is_empty() {
            content.append(&label(&state.feedback, "muted"));
        }
        if state.busy {
            let spin = gtk::Spinner::new();
            spin.start();
            spin.set_halign(gtk::Align::Start);
            content.append(&spin);
        }
        body.append(&content);
        let overlay = gtk::Overlay::new();
        overlay.set_child(Some(&body));
        let hint = column(0);
        hint.set_widget_name("interaction-hint");
        hint.add_css_class("interaction-hint");
        hint.set_size_request(120, 3);
        hint.set_halign(gtk::Align::Center);
        hint.set_valign(gtk::Align::End);
        hint.set_can_target(false);
        overlay.add_overlay(&hint);
        *self.context.hint.borrow_mut() = hint.upcast::<gtk::Widget>().downgrade();
        self.widget.append(&overlay);
        self.context.sync_reveal();
        for media in self.context.media.borrow().iter() {
            media.set_active(!state.locked && !state.paused && !state.history);
        }
        if restore_editor_focus && self.context.allowed() && self.context.composing.get() {
            if let Some(editor) = self.context.editor.borrow().upgrade() {
                editor.grab_focus();
            }
        }
    }
    fn artwork(&self, n: &Notice) -> gtk::Box {
        let rail = column(0);
        rail.add_css_class("artwork");
        rail.set_width_request(80);
        rail.set_hexpand(false);
        rail.set_overflow(gtk::Overflow::Hidden);
        let terminal = ["kitty", "kitty.desktop", "fish", "fish.desktop"]
            .contains(&text(&n.0["app"]).to_lowercase().as_str())
            || ["kitty", "kitty.desktop", "fish", "fish.desktop"]
                .contains(&text(&n.0["desktopEntry"]).to_lowercase().as_str());
        let source = if terminal {
            String::new()
        } else {
            let image = text(&n.0["image"]);
            if image.is_empty() {
                text(&n.0["icon"])
            } else {
                image
            }
        };
        if n.kind() == "telegram" && source.is_empty() {
            let initials = n
                .title()
                .split_whitespace()
                .take(2)
                .filter_map(|s| s.chars().next())
                .collect::<String>()
                .to_uppercase();
            let l = label(&initials, "initials");
            l.set_xalign(0.5);
            l.set_valign(gtk::Align::Center);
            l.set_vexpand(true);
            rail.append(&l);
        } else if let Some(path) = local_path(&source).or_else(|| {
            if source.is_empty() {
                Some(artwork_path(&self.root, n))
            } else {
                None
            }
        }) {
            let picture = gtk::Picture::for_file(&gtk::gio::File::for_path(path));
            picture.set_can_shrink(true);
            picture.set_content_fit(gtk::ContentFit::Cover);
            picture.set_width_request(80);
            picture.set_vexpand(true);
            let overlay = gtk::Overlay::new();
            let spacer = column(0);
            spacer.set_width_request(80);
            spacer.set_height_request(80);
            overlay.set_child(Some(&spacer));
            overlay.add_overlay(&picture);
            overlay.set_measure_overlay(&picture, false);
            overlay.set_clip_overlay(&picture, true);
            overlay.set_vexpand(true);
            rail.append(&overlay);
        } else {
            let image = gtk::Image::new();
            tjik_gtk::images::set(&image, &source, "dialog-information-symbolic");
            image.set_pixel_size(48);
            image.set_valign(gtk::Align::Center);
            image.set_vexpand(true);
            rail.append(&image);
        }
        rail
    }
}
fn artwork_path(root: &Path, n: &Notice) -> PathBuf {
    // Read the shared manifest so existing notification history keeps its artwork.
    let logic = include_str!("../../qml/Logic.js");
    let array = |key: &str| -> Vec<&str> {
        logic
            .split(key)
            .nth(1)
            .unwrap_or("")
            .split(']')
            .next()
            .unwrap_or("")
            .split('"')
            .enumerate()
            .filter_map(|(i, s)| if i % 2 == 1 { Some(s) } else { None })
            .collect()
    };
    let fish = ["app", "desktopEntry"]
        .iter()
        .any(|key| ["fish", "fish.desktop"].contains(&text(&n.0[key]).to_lowercase().as_str()));
    let files = array(if fish {
        "var fishFiles = ["
    } else {
        "var robotFiles = ["
    });
    let seed = format!("{}:{}", n.id(), n.0["received"].as_f64().unwrap_or(0.));
    let hash = seed
        .encode_utf16()
        .fold(2166136261u32, |h, c| (h ^ c as u32).wrapping_mul(16777619)) as usize;
    let index = if fish {
        let names = array("var fishNames = [");
        let legacy = array("var legacyFishNames = [");
        let name = n.0["fishName"].as_str().or_else(|| {
            n.0["fishIndex"]
                .as_u64()
                .and_then(|i| legacy.get(i as usize).copied())
        });
        name.and_then(|name| names.iter().position(|s| *s == name))
            .unwrap_or(hash % files.len().max(1))
    } else {
        let (start, count) = match n.0["urgency"].as_str() {
            Some("critical") => (64, 12),
            Some("low") => (76, 12),
            _ => (0, 64),
        };
        n.0["robotIndex"]
            .as_u64()
            .map(|i| i as usize)
            .filter(|i| *i >= start && *i < start + count)
            .unwrap_or(start + hash % count)
    };
    root.join("qml/icons")
        .join(if fish { "fish" } else { "robots" })
        .join(files.get(index).copied().unwrap_or(""))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn historical_artwork_keeps_names_and_urgency_pools() {
        let root = Path::new("/synthetic");
        let fish = Notice(json!({"desktopEntry":"fish.desktop","fishIndex":13}));
        assert!(artwork_path(root, &fish).ends_with("fish-05-seahorse.png"));
        let fish = Notice(json!({"app":"fish","fishIndex":13,"fishName":"jade-koi"}));
        assert!(artwork_path(root, &fish).ends_with("fish-04-jade-koi.png"));
        let critical =
            Notice(json!({"id":"stable","received":1,"urgency":"critical","robotIndex":0}));
        let a = artwork_path(root, &critical);
        assert_eq!(a, artwork_path(root, &critical));
        assert!(a.file_name().unwrap().to_string_lossy().contains("alert"));
    }
}
