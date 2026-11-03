use crate::{
    codex::{self, Action, Proposal},
    model::{self, Contact, Draft, Label, Message, Page},
    store::Store,
    worker::{self, Command, Event},
};
use gpui_kit::base::Disableable;
use gpui_kit::component::{
    Icon, Sizable, Theme, ThemeMode,
    button::{Button, ButtonVariants},
    input::{Input, InputEvent, InputState, Textarea, TextareaState},
    resizable::{ResizableState, h_resizable, resizable_panel},
    text::TextView,
};
use gpui_kit::{prelude::*, *};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

const BG: u32 = 0x1f1d2e;
const SURFACE: u32 = 0x26233a;
const RAISED: u32 = 0x39354b;
const TEXT: u32 = 0xe5e5e5;
const MUTED: u32 = 0xa5a0ba;
const ACCENT: u32 = 0xff53a6;
const BORDER: u32 = 0x393546;

fn proicon(name: &str) -> Icon {
    let bytes: &[u8] = match name {
        "archive" => include_bytes!("../assets/proicons/archive.svg"),
        "arrow-left" => include_bytes!("../assets/proicons/arrow-left.svg"),
        "arrow-reply" => include_bytes!("../assets/proicons/arrow-reply.svg"),
        "arrow-right" => include_bytes!("../assets/proicons/arrow-right.svg"),
        "arrow-sync" => include_bytes!("../assets/proicons/arrow-sync.svg"),
        "chevron-down" => include_bytes!("../assets/proicons/chevron-down.svg"),
        "chevron-right" => include_bytes!("../assets/proicons/chevron-right.svg"),
        "compose" => include_bytes!("../assets/proicons/compose.svg"),
        "delete" => include_bytes!("../assets/proicons/delete.svg"),
        "folder" => include_bytes!("../assets/proicons/folder.svg"),
        "mail" => include_bytes!("../assets/proicons/mail.svg"),
        "mail-open" => include_bytes!("../assets/proicons/mail-open.svg"),
        "open" => include_bytes!("../assets/proicons/open.svg"),
        "send" => include_bytes!("../assets/proicons/send.svg"),
        "settings" => include_bytes!("../assets/proicons/settings.svg"),
        "sparkle" => include_bytes!("../assets/proicons/sparkle.svg"),
        "star" => include_bytes!("../assets/proicons/star.svg"),
        _ => unreachable!("Unknown Proicon"),
    };
    Icon::default().data(bytes)
}

pub fn theme(cx: &mut App) {
    Theme::change(ThemeMode::Dark, None, cx);
    Theme::update(cx, |t| {
        t.colors.background = rgb(BG).into();
        t.colors.foreground = rgb(TEXT).into();
        t.colors.border = rgb(BORDER).into();
        t.colors.input = rgb(BORDER).into();
        t.colors.primary = rgb(ACCENT).into();
        t.colors.primary_foreground = rgb(BG).into();
        t.colors.button = rgb(RAISED).into();
        t.colors.button_foreground = rgb(TEXT).into();
        t.colors.button_primary = rgb(ACCENT).into();
        t.colors.button_primary_foreground = rgb(BG).into();
        t.colors.button_primary_hover = rgb(0xe64894).into();
        t.colors.button_primary_active = rgb(0xcf3d83).into();
        t.colors.button_hover = rgb(0x454056).into();
        t.colors.muted_foreground = rgb(MUTED).into();
        t.radius = px(10.);
        t.font_size = px(14.);
        t.font_family = "tijpset".into();
    });
}

pub struct MailApp {
    settings: bool,
    settings_reading: bool,
    settings_sync: bool,
    push_fields: Vec<Entity<InputState>>,
    push_enabled: bool,
    push_note: String,
    rules: Vec<crate::rules::Rule>,
    rule_index: usize,
    rule_fields: Vec<Entity<InputState>>,
    rules_note: String,
    panes: Entity<ResizableState>,
    inbox_width: f32,
    html_width: u32,
    html_width_candidate: (u32, Instant),
    html_tiles: std::collections::HashMap<u64, Vec<(u32, Arc<Image>)>>,
    html_renderer: crate::html::Renderer,
    html_resources: std::collections::HashMap<String, crate::html::Resources>,
    text_messages: std::collections::HashSet<String>,
    focus: FocusHandle,
    shortcuts: bool,
    zone: usize,
    mail_scroll: UniformListScrollHandle,
    reader_scroll: ScrollHandle,
    collapsed_labels: std::collections::HashSet<String>,
    label_cursor: String,
    label_visibility: crate::labels::Preferences,
    show_hidden_labels: bool,
    label_menu: Option<String>,
    label_menu_index: usize,
    demo: bool,
    dir: Option<PathBuf>,
    store: Store,
    sender: Option<mpsc::Sender<Command>>,
    events: Option<mpsc::Receiver<Event>>,
    labels: Vec<Label>,
    page: Page,
    demo_messages: Vec<Message>,
    selected: Option<Message>,
    label: String,
    query: String,
    unread_only: bool,
    marking_all_read: bool,
    page_token: String,
    previous: Vec<String>,
    ticket: u64,
    loading: bool,
    last_sync: Instant,
    email: String,
    status: String,
    error: String,
    oauth_path: Entity<InputState>,
    connecting: bool,
    search: Entity<InputState>,
    to: Entity<InputState>,
    cc: Entity<InputState>,
    bcc: Entity<InputState>,
    subject: Entity<InputState>,
    body: Entity<TextareaState>,
    instruction: Entity<InputState>,
    draft: Draft,
    composing: bool,
    compose_busy: bool,
    review_send: bool,
    contacts: Vec<Contact>,
    known_contacts: Vec<Contact>,
    contact_defaults: crate::contacts::Defaults,
    expanded_contact: Option<String>,
    contact_cursor: Option<usize>,
    contact_photos: std::collections::HashMap<String, Arc<Image>>,
    contact_field: usize,
    contacts_due: Option<Instant>,
    contact_query: String,
    images: Vec<(String, Arc<Image>)>,
    auto_images: bool,
    image_generation: u64,
    conversation: Vec<Message>,
    thread_images: std::collections::HashMap<String, Vec<(String, Arc<Image>)>>,
    expanded_quotes: std::collections::HashSet<String>,
    expanded_messages: std::collections::HashSet<String>,
    manual_images: std::collections::HashSet<String>,
    light_images: bool,
    proposal: Option<Proposal>,
    proposal_target: Option<String>,
    proposal_draft: Option<Draft>,
    assistant_busy: bool,
    assistant_events: Option<mpsc::Receiver<codex::Event>>,
    cancel: Arc<AtomicBool>,
    _subscriptions: Vec<Subscription>,
}

impl MailApp {
    pub fn new(
        demo: bool,
        dir: Option<PathBuf>,
        open: Option<crate::links::MailLink>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let store =
            Store::open(dir.as_deref()).expect("Private cache must open before the UI starts");
        if demo && std::env::var("GMAILTJIK_DEMO_SCENE").as_deref() == Ok("saved-draft") {
            store
                .put(
                    "composer",
                    &Draft {
                        to: "friend@example.test".into(),
                        subject: "Saved synthetic draft".into(),
                        body: "Keep this draft until I choose Compose.".into(),
                        ..Default::default()
                    },
                )
                .expect("Synthetic draft must be saved");
        }
        let draft = store.draft().unwrap_or_default();
        let (labels, page) = if demo {
            if std::env::var("GMAILTJIK_DEMO_SCENE").as_deref() == Ok("threads") {
                model::demo_conversations()
            } else {
                model::demo()
            }
        } else {
            (
                store.get("labels").ok().flatten().unwrap_or_default(),
                store
                    .page(&worker::key("INBOX", "", ""))
                    .ok()
                    .flatten()
                    .unwrap_or_default(),
            )
        };
        let input = |placeholder: &'static str,
                     value: String,
                     window: &mut Window,
                     cx: &mut Context<Self>| {
            cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(placeholder)
                    .default_value(value)
            })
        };
        let oauth_path = input(
            "Private path to Google Desktop OAuth client JSON",
            String::new(),
            window,
            cx,
        );
        let search = input(
            "Search all mail · from:  before:  has:attachment",
            String::new(),
            window,
            cx,
        );
        let rules = crate::rules::load(&store).unwrap_or_else(|_| crate::rules::defaults());
        let first_rule = rules.first().cloned().unwrap_or_default();
        let rule_fields = vec![
            input("Rule name", first_rule.name, window, cx),
            input("Any label", first_rule.label, window, cx),
            input("Any sender", first_rule.sender, window, cx),
            input("Any subject", first_rule.subject, window, cx),
        ];
        let push_config = dir
            .as_ref()
            .and_then(|dir| crate::push::load(dir).ok().flatten());
        let push_fields = vec![
            input(
                "projects/my-project/topics/gmail",
                push_config
                    .as_ref()
                    .map(|c| c.topic.clone())
                    .unwrap_or_default(),
                window,
                cx,
            ),
            input(
                "projects/my-project/subscriptions/gmail",
                push_config
                    .as_ref()
                    .map(|c| c.subscription.clone())
                    .unwrap_or_default(),
                window,
                cx,
            ),
            input(
                "/private/path/pubsub-credentials.json",
                push_config
                    .as_ref()
                    .map(|c| c.credentials_file.display().to_string())
                    .unwrap_or_default(),
                window,
                cx,
            ),
        ];
        let to = input("To", draft.to.clone(), window, cx);
        let cc = input("Cc", draft.cc.clone(), window, cx);
        let bcc = input("Bcc", draft.bcc.clone(), window, cx);
        let subject = input("Subject", draft.subject.clone(), window, cx);
        let body = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder("Write something thoughtful…")
                .default_value(draft.body.clone())
        });
        let instruction = input(
            "Tell Codex what to do with this email…",
            String::new(),
            window,
            cx,
        );
        let mut subscriptions = vec![cx.subscribe_in(&search, window, |this, _, event, _, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                this.query = this.search.read(cx).value().trim().into();
                this.label = if this.query.is_empty() {
                    "INBOX".into()
                } else {
                    String::new()
                };
                this.previous.clear();
                this.page_token.clear();
                this.load(cx);
            }
        })];
        for field in &rule_fields {
            subscriptions.push(cx.subscribe_in(field, window, |this, _, event, _, cx| {
                if matches!(event, InputEvent::Change) {
                    this.rules_note = "Unsaved changes".into();
                    cx.notify();
                }
            }));
        }
        for field in [&to, &cc, &bcc, &subject] {
            subscriptions.push(cx.subscribe_in(field, window, |this, _, event, _, cx| {
                if matches!(event, InputEvent::Change) {
                    this.changed(cx);
                }
            }));
        }
        for (index, field) in [&to, &cc, &bcc].into_iter().enumerate() {
            subscriptions.push(
                cx.subscribe_in(field, window, move |this, _, event, _, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.expanded_contact = None;
                        this.contact_cursor = None;
                        this.contact_field = index;
                        let field = [&this.to, &this.cc, &this.bcc][index];
                        this.contact_query = field
                            .read(cx)
                            .value()
                            .rsplit([',', ';'])
                            .next()
                            .unwrap_or_default()
                            .trim()
                            .into();
                        this.contacts.clear();
                        this.contacts_due = Some(Instant::now() + Duration::from_millis(350));
                    }
                }),
            );
        }
        subscriptions.push(cx.subscribe_in(&body, window, |this, _, event, _, cx| {
            if matches!(event, InputEvent::Change) {
                this.changed(cx);
            }
        }));
        subscriptions.push(
            cx.subscribe_in(&instruction, window, |this, _, event, _, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.assist(cx);
                }
            }),
        );
        let (sender, events) = if let Some(dir) = &dir {
            let (s, e) = worker::start(dir.clone(), false);
            (Some(s), Some(e))
        } else {
            (None, None)
        };
        let email = if demo {
            "you@example.test".into()
        } else {
            store.get("email").ok().flatten().unwrap_or_default()
        };
        let selected = page.representatives().first().cloned();
        let contacts_demo =
            demo && std::env::var("GMAILTJIK_DEMO_SCENE").as_deref() == Ok("contacts");
        // Restore draft contents, but leave opening the composer to an explicit action.
        let composing = contacts_demo;
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        let mut this = Self {
            panes: cx.new(|_| ResizableState::default()),
            inbox_width: store
                .get::<f32>("inbox_width")
                .ok()
                .flatten()
                .unwrap_or(330.)
                .clamp(220., 700.),
            html_width: 600,
            html_width_candidate: (600, Instant::now()),
            html_tiles: Default::default(),
            html_renderer: crate::html::Renderer::new(),
            html_resources: Default::default(),
            text_messages: Default::default(),
            settings: demo && std::env::var("GMAILTJIK_DEMO_SCENE").as_deref() == Ok("rules"),
            settings_reading: false,
            settings_sync: false,
            push_enabled: push_config.is_some_and(|c| c.enabled),
            push_fields,
            push_note: String::new(),
            rules,
            rule_index: 0,
            rule_fields,
            rules_note: String::new(),
            contact_defaults: store
                .get("contact_defaults")
                .ok()
                .flatten()
                .unwrap_or_default(),
            known_contacts: store.get("contacts").ok().flatten().unwrap_or_default(),
            auto_images: store
                .get("auto_remote_images")
                .ok()
                .flatten()
                .unwrap_or(true),
            image_generation: 0,
            conversation: vec![],
            thread_images: Default::default(),
            expanded_quotes: Default::default(),
            expanded_messages: Default::default(),
            manual_images: Default::default(),
            collapsed_labels: store
                .get("collapsed_labels")
                .ok()
                .flatten()
                .unwrap_or_default(),
            label_cursor: "system:INBOX".into(),
            label_visibility: store
                .get("label_visibility")
                .ok()
                .flatten()
                .unwrap_or_default(),
            show_hidden_labels: false,
            label_menu: None,
            label_menu_index: 0,
            focus,
            shortcuts: false,
            zone: 1,
            mail_scroll: UniformListScrollHandle::new(),
            reader_scroll: ScrollHandle::new(),
            demo,
            dir,
            store,
            sender,
            events,
            labels,
            demo_messages: page.messages.clone(),
            page,
            selected,
            label: "INBOX".into(),
            query: String::new(),
            unread_only: false,
            marking_all_read: false,
            page_token: String::new(),
            previous: vec![],
            ticket: 0,
            loading: false,
            last_sync: Instant::now(),
            email,
            status: if demo {
                "Demo · synthetic mail".into()
            } else {
                "Opening cached inbox…".into()
            },
            error: String::new(),
            oauth_path,
            connecting: false,
            search,
            to,
            cc,
            bcc,
            subject,
            body,
            instruction,
            draft,
            composing,
            compose_busy: false,
            review_send: false,
            contacts: vec![],
            expanded_contact: None,
            contact_cursor: None,
            contact_photos: Default::default(),
            contact_field: 0,
            contacts_due: None,
            contact_query: String::new(),
            images: vec![],
            light_images: false,
            proposal: None,
            proposal_target: None,
            proposal_draft: None,
            assistant_busy: false,
            assistant_events: None,
            cancel: Arc::new(AtomicBool::new(false)),
            _subscriptions: subscriptions,
        };
        this.migrate_contact_defaults();
        if !demo {
            this.load(cx);
            if let Some(link) = open {
                this.dispatch(Command::OpenMail(link));
            }
        } else {
            this.refresh_reader();
        }
        if contacts_demo {
            this.to.read(cx).focus_handle(cx).focus(window, cx);
        }
        cx.spawn_in(window, async move |view, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
                if view
                    .update_in(cx, |this, window, cx| this.poll(window, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        this
    }
    fn key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();
        if key == "escape" {
            self.shortcuts = false;
            self.label_menu = None;
            self.focus.focus(window, cx);
        } else if event.keystroke.modifiers.control && key == "f" {
            self.search.read(cx).focus_handle(cx).focus(window, cx);
        } else if event.keystroke.modifiers.control && key == "n" {
            self.composing = true;
            self.to.read(cx).focus_handle(cx).focus(window, cx);
        } else if !self.focus.is_focused(window) {
            return;
        } else {
            if self.label_menu.is_some() {
                match key {
                    "up" => self.label_menu_index = self.label_menu_index.saturating_sub(1),
                    "down" => self.label_menu_index = (self.label_menu_index + 1).min(2),
                    "enter" | "space" => self.set_label_visibility(self.label_menu_index, cx),
                    _ => {}
                }
                cx.notify();
                return;
            }
            if event.keystroke.modifiers.control
                || event.keystroke.modifiers.alt
                || event.keystroke.modifiers.platform
            {
                return;
            }
            match key {
                "r" => {
                    if let Some(message) = self.selected.clone() {
                        self.reply(&message, window, cx);
                    }
                }
                "a" => self.archive_selected(cx),
                "d" => self.trash_selected(cx),
                "s" if self.zone == 0 => {
                    self.label_menu = Some(self.label_cursor.clone());
                    self.label_menu_index = 0;
                }
                "?" => self.shortcuts = !self.shortcuts,
                "enter" | "space" if self.zone == 0 => {
                    let path = self.label_cursor.clone();
                    self.toggle_label(&path, cx);
                }
                "left" => self.zone = self.zone.saturating_sub(1),
                "right" => self.zone = (self.zone + 1).min(2),
                "up" | "down" => {
                    let down = key == "down";
                    if self.zone == 0 {
                        let labels = crate::labels::visible_rows(
                            &self.labels,
                            &self.collapsed_labels,
                            &self.label_visibility,
                            self.show_hidden_labels,
                        );
                        if !labels.is_empty() {
                            let index = labels
                                .iter()
                                .position(|l| l.path == self.label_cursor)
                                .unwrap_or(0);
                            let next = if down {
                                (index + 1).min(labels.len() - 1)
                            } else {
                                index.saturating_sub(1)
                            };
                            self.label_cursor = labels[next].path.clone();
                            let Some(id) = labels[next].id.clone() else {
                                cx.notify();
                                return;
                            };
                            self.label = id;
                            self.query.clear();
                            self.page_token.clear();
                            self.previous.clear();
                            self.load(cx);
                        }
                    } else if self.zone == 2 {
                        let offset = self.reader_scroll.offset();
                        self.reader_scroll.set_offset(point(
                            offset.x,
                            (offset.y + px(if down { -80. } else { 80. })).min(px(0.)),
                        ));
                    } else if !self.page.messages.is_empty() {
                        let index =
                            self.selected
                                .as_ref()
                                .and_then(|m| {
                                    self.page.representatives().iter().position(|row| {
                                        row.conversation_key() == m.conversation_key()
                                    })
                                })
                                .unwrap_or(0);
                        let next = if down {
                            (index + 1).min(self.page.conversations().len() - 1)
                        } else {
                            index.saturating_sub(1)
                        };
                        self.select(next, cx);
                        self.mail_scroll
                            .scroll_to_item(next, ScrollStrategy::Nearest);
                    }
                }
                _ => return,
            }
        }
        cx.stop_propagation();
        cx.notify();
    }
    fn reply(&mut self, message: &Message, window: &mut Window, cx: &mut Context<Self>) {
        let reply = Draft::reply(message);
        let has_draft = self.draft != Draft::default();
        let same_reply = !reply.in_reply_to.is_empty()
            && self.draft.in_reply_to == reply.in_reply_to
            && self.draft.thread_id == reply.thread_id;
        if has_draft && !same_reply {
            self.status = "Finish or clear your current draft before starting a reply.".into();
        } else {
            if !same_reply {
                self.set_draft(reply, window, cx);
            }
            self.composing = true;
            self.body.read(cx).focus_handle(cx).focus(window, cx);
        }
        cx.notify();
    }
    fn archive_selected(&mut self, cx: &mut Context<Self>) {
        if let Some(message) = &self.selected {
            self.modify(message.id.clone(), vec![], vec!["INBOX".into()], cx);
        }
    }
    fn trash_selected(&mut self, cx: &mut Context<Self>) {
        if let Some(message) = &self.selected {
            if message.label_ids.iter().any(|label| label == "TRASH") {
                return;
            }
            let id = message.id.clone();
            if self.demo {
                let ids = self.conversation_ids(&id);
                self.modified(&ids, &["TRASH".into()], &["INBOX".into()], cx);
            } else {
                self.status = "Moving email to Trash…".into();
                let thread = message.thread_id.clone();
                self.dispatch(Command::Trash { id, thread });
            }
            cx.notify();
        }
    }
    fn changed(&mut self, cx: &mut Context<Self>) {
        self.draft.to = self.to.read(cx).value().to_string();
        self.draft.cc = self.cc.read(cx).value().to_string();
        self.draft.bcc = self.bcc.read(cx).value().to_string();
        self.draft.subject = self.subject.read(cx).value().to_string();
        self.draft.body = self.body.read(cx).value().to_string();
        self.review_send = false;
        if let Err(error) = self.store.put("composer", &self.draft) {
            self.error = format!("Draft could not be saved locally: {error}");
        }
        cx.notify();
    }
    fn set_draft(&mut self, draft: Draft, window: &mut Window, cx: &mut Context<Self>) {
        for (input, value) in [
            (&self.to, &draft.to),
            (&self.cc, &draft.cc),
            (&self.bcc, &draft.bcc),
            (&self.subject, &draft.subject),
        ] {
            input.update(cx, |input, cx| input.set_value(value.clone(), window, cx));
        }
        self.body.update(cx, |input, cx| {
            input.set_value(draft.body.clone(), window, cx)
        });
        self.draft = draft;
        self.composing = true;
        self.contacts.clear();
        self.changed(cx);
    }
    fn dispatch(&mut self, command: Command) {
        if let Some(sender) = &self.sender {
            if sender.send(command).is_err() {
                self.error="Gmail connection is unavailable. Restart gmailtjik after checking your connection.".into();
                self.loading = false;
                self.compose_busy = false;
                self.marking_all_read = false;
            }
        }
    }
    fn load(&mut self, cx: &mut Context<Self>) {
        self.ticket += 1;
        self.error.clear();
        self.images.clear();
        let query = if self.unread_only {
            crate::google::unread_query(&self.query)
        } else {
            self.query.clone()
        };
        if self.demo {
            let mut page = Page {
                messages: self.demo_messages.clone(),
                next: String::new(),
                threaded: true,
            };
            page.retain_conversations(|m| {
                (!self.unread_only || m.unread())
                    && (self.label.is_empty() || m.label_ids.contains(&self.label))
                    && (self.label == "TRASH" || !m.label_ids.iter().any(|l| l == "TRASH"))
                    && (self.query.is_empty()
                        || format!("{} {} {}", m.subject(), m.header("From"), m.text())
                            .to_lowercase()
                            .contains(&self.query.to_lowercase()))
            });
            self.page = page;
            self.selected = self.page.representatives().first().cloned();
            self.status = "Demo · synthetic mail · plain-text search".into();
            self.refresh_reader();
        } else {
            self.loading = true;
            self.status = "Syncing with Gmail…".into();
            if let Ok(Some(page)) =
                self.store
                    .page(&worker::key(&self.label, &query, &self.page_token))
            {
                self.page = page;
                self.selected = self.page.representatives().first().cloned();
            } else {
                self.page = Page::default();
                self.selected = None;
            }
            self.dispatch(Command::Load {
                ticket: self.ticket,
                label: self.label.clone(),
                query,
                page: self.page_token.clone(),
            });
        }
        self.last_sync = Instant::now();
        cx.notify();
    }
    fn mark_all_read(&mut self, cx: &mut Context<Self>) {
        if self.marking_all_read {
            return;
        }
        if self.demo {
            let ids: Vec<_> = self
                .demo_messages
                .iter()
                .filter(|m| {
                    m.unread()
                        && (self.label.is_empty() || m.label_ids.contains(&self.label))
                        && (self.label == "TRASH" || !m.label_ids.iter().any(|l| l == "TRASH"))
                        && (self.query.is_empty()
                            || format!("{} {} {}", m.subject(), m.header("From"), m.text())
                                .to_lowercase()
                                .contains(&self.query.to_lowercase()))
                })
                .map(|m| m.id.clone())
                .collect();
            for id in &ids {
                self.modified(std::slice::from_ref(id), &[], &["UNREAD".into()], cx);
            }
            self.load(cx);
            self.status = format!("Marked {} messages read", ids.len());
        } else {
            self.marking_all_read = true;
            self.error.clear();
            self.status = "Marking all matching unread mail as read…".into();
            self.dispatch(Command::MarkAllRead {
                label: self.label.clone(),
                query: self.query.clone(),
            });
        }
        cx.notify();
    }
    fn refresh_reader(&mut self) {
        let Some(message) = self.selected.clone() else {
            self.conversation.clear();
            self.images.clear();
            self.thread_images.clear();
            self.html_resources.clear();
            return;
        };
        if self
            .conversation
            .first()
            .is_none_or(|row| row.thread_id != message.thread_id)
        {
            self.conversation = vec![message.clone()];
            self.images.clear();
            self.thread_images.clear();
            self.html_resources.clear();
            self.image_generation += 1;
            self.expanded_quotes.clear();
            self.expanded_messages.clear();
            self.manual_images.clear();
        }
        if self.demo {
            self.conversation = if std::env::var("GMAILTJIK_DEMO_SCENE").as_deref() == Ok("threads")
            {
                let mut messages: Vec<_> = self
                    .demo_messages
                    .iter()
                    .filter(|m| m.conversation_key() == message.conversation_key())
                    .cloned()
                    .collect();
                messages.sort_by_key(|m| m.internal_date.parse::<i64>().unwrap_or(0));
                messages
            } else {
                model::demo_thread(&message)
            };
            self.demo_image();
        } else {
            self.images = self
                .thread_images
                .get(&message.id)
                .cloned()
                .unwrap_or_default();
            if !message.thread_id.is_empty() {
                self.dispatch(Command::Thread(message.thread_id.clone()));
            }
            if !self.thread_images.contains_key(&message.id) {
                self.dispatch(Command::Images {
                    generation: self.image_generation,
                    message,
                    remote: self.auto_images,
                });
            }
        }
    }
    fn select(&mut self, index: usize, cx: &mut Context<Self>) {
        self.selected = self.page.representatives().get(index).cloned();
        self.images.clear();
        self.proposal = None;
        self.refresh_reader();
        cx.notify();
    }
    fn modify(
        &mut self,
        id: String,
        add: Vec<String>,
        remove: Vec<String>,
        cx: &mut Context<Self>,
    ) {
        if self.demo {
            let ids = self.conversation_ids(&id);
            self.modified(&ids, &add, &remove, cx);
        } else {
            self.status = "Saving mailbox change…".into();
            let thread = self
                .page
                .messages
                .iter()
                .chain(self.selected.iter())
                .find(|m| m.id == id)
                .map(|m| m.thread_id.clone())
                .unwrap_or_default();
            self.dispatch(Command::Modify {
                id,
                thread,
                add,
                remove,
            });
        }
        cx.notify();
    }
    fn conversation_ids(&self, id: &str) -> Vec<String> {
        let all = || {
            self.page
                .messages
                .iter()
                .chain(self.selected.iter())
                .chain(self.demo_messages.iter())
        };
        let Some(target) = all().find(|m| m.id == id) else {
            return vec![id.into()];
        };
        all()
            .filter(|m| m.conversation_key() == target.conversation_key())
            .map(|m| m.id.clone())
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect()
    }
    fn modified(
        &mut self,
        ids: &[String],
        add: &[String],
        remove: &[String],
        cx: &mut Context<Self>,
    ) {
        let selected = self
            .selected
            .as_ref()
            .map(|m| (m.id.clone(), m.conversation_key().to_owned()));
        let successor = selected
            .as_ref()
            .and_then(|(id, _)| successor(&self.page.representatives(), id));
        let mut seen = std::collections::HashSet::new();
        for before in self
            .page
            .messages
            .iter()
            .chain(self.selected.iter())
            .chain(self.demo_messages.iter())
            .chain(self.conversation.iter())
        {
            if ids.contains(&before.id) && seen.insert(before.id.clone()) {
                crate::labels::update_unread(&mut self.labels, before, add, remove);
            }
        }
        for message in self
            .page
            .messages
            .iter_mut()
            .chain(self.selected.iter_mut())
            .chain(self.demo_messages.iter_mut())
            .chain(self.conversation.iter_mut())
        {
            if ids.contains(&message.id) {
                message.label_ids.retain(|l| !remove.contains(l));
                for label in add {
                    if !message.label_ids.contains(label) {
                        message.label_ids.push(label.clone());
                    }
                }
            }
        }
        self.page.retain_conversations(|m| {
            (!self.unread_only || m.unread())
                && (self.label.is_empty() || m.label_ids.contains(&self.label))
                && (self.label == "TRASH" || !m.label_ids.iter().any(|l| l == "TRASH"))
        });
        let rows = self.page.representatives();
        if selected
            .as_ref()
            .is_some_and(|(_, key)| !rows.iter().any(|m| m.conversation_key() == key))
        {
            if let Some(index) = successor.and_then(|id| rows.iter().position(|m| m.id == id)) {
                self.select(index, cx);
                self.mail_scroll
                    .scroll_to_item(index, ScrollStrategy::Nearest);
            } else {
                self.selected = None;
                self.proposal = None;
                self.refresh_reader();
            }
            self.reader_scroll.set_offset(point(px(0.), px(0.)));
        }
        self.status = if self.demo {
            "Demo conversation updated".into()
        } else {
            "Conversation saved to Gmail".into()
        };
    }
    fn poll(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.html_renderer.poll() {
            cx.notify();
        }
        if let Some(size) = self.panes.read(cx).sizes().get(1) {
            let width = (f32::from(*size) - 82.).max(160.) as u32;
            if width != self.html_width_candidate.0 {
                self.html_width_candidate = (width, Instant::now());
            }
            if width != self.html_width
                && self.html_width_candidate.1.elapsed() >= Duration::from_millis(180)
            {
                self.html_width = width;
                cx.notify();
            }
        }
        let events: Vec<_> = self
            .events
            .as_ref()
            .map(|r| r.try_iter().collect())
            .unwrap_or_default();
        let notify = !events.is_empty();
        for event in events {
            match event {
                Event::AllRead(result) => {
                    self.marking_all_read = false;
                    self.page_token.clear();
                    self.previous.clear();
                    self.load(cx);
                    match result {
                        Ok(count) => self.status = format!("Marked {count} messages read"),
                        Err(error) => self.error = error,
                    }
                }
                Event::OpenMail(message) => {
                    self.ticket += 1;
                    self.loading = false;
                    self.label.clear();
                    self.query.clear();
                    self.unread_only = false;
                    self.page_token.clear();
                    self.page = Page {
                        messages: vec![message.clone()],
                        ..Default::default()
                    };
                    self.selected = Some(message);
                    self.zone = 1;
                    self.refresh_reader();
                    window.activate_window();
                }
                Event::Connected => {
                    self.connecting = false;
                    self.error.clear();
                    self.load(cx);
                }
                Event::ConnectionFailed(error) => {
                    self.connecting = false;
                    self.error = error;
                }
                Event::Inbox(page) => {
                    if self.label == "INBOX"
                        && !self.unread_only
                        && self.query.is_empty()
                        && self.page_token.is_empty()
                        && !self.loading
                    {
                        let selected = self
                            .selected
                            .as_ref()
                            .map(|m| m.conversation_key().to_owned());
                        self.page = page;
                        self.selected = selected
                            .and_then(|id| {
                                self.page
                                    .representatives()
                                    .into_iter()
                                    .find(|m| m.conversation_key() == id)
                            })
                            .or_else(|| self.page.representatives().first().cloned());
                    }
                    self.refresh_reader();
                    self.error.clear();
                    self.status = "Inbox synced · checks every 30 seconds".into();
                }
                Event::SyncError(error) => {
                    self.error = error;
                }
                Event::Ready { labels, email } => {
                    self.labels = labels;
                    self.email = email;
                }
                Event::Page {
                    ticket,
                    cached,
                    page,
                } if ticket == self.ticket => {
                    let selected = self
                        .selected
                        .as_ref()
                        .map(|m| m.conversation_key().to_owned());
                    self.page = page;
                    self.selected = selected
                        .and_then(|id| {
                            self.page
                                .representatives()
                                .into_iter()
                                .find(|m| m.conversation_key() == id)
                        })
                        .or_else(|| self.page.representatives().first().cloned());
                    self.status = if cached {
                        "Cached mail · syncing…".into()
                    } else {
                        "Up to date".into()
                    };
                    if !cached {
                        self.refresh_reader();
                    }
                }

                Event::Loaded { ticket } if ticket == self.ticket => {
                    self.loading = false;
                }
                Event::Error(error) => {
                    self.marking_all_read = false;
                    self.error = error;
                }
                Event::Modified { ids, add, remove } => {
                    self.modified(&ids, &add, &remove, cx);
                }
                Event::ContactBook(rows) => {
                    self.known_contacts = rows;
                    self.migrate_contact_defaults();
                }
                Event::Contacts { query, rows } if query == self.contact_query => {
                    let urls = crate::contacts::people(&rows)
                        .into_iter()
                        .map(|person| person.photo)
                        .filter(|url| !url.is_empty() && !self.contact_photos.contains_key(url))
                        .collect();
                    for contact in &rows {
                        if let Some(known) = self.known_contacts.iter_mut().find(|known| {
                            (known.resource_name == contact.resource_name
                                || known.resource_name.is_empty())
                                && known.email.eq_ignore_ascii_case(&contact.email)
                        }) {
                            *known = contact.clone();
                        } else {
                            self.known_contacts.push(contact.clone());
                        }
                    }
                    self.contacts = rows;
                    self.dispatch(Command::ContactPhotos(urls));
                }
                Event::ContactPhoto { url, bytes } => {
                    if let Some(image) = image_from_bytes(bytes) {
                        if self.contact_photos.len() >= 128 {
                            self.contact_photos.clear();
                        }
                        self.contact_photos.insert(url, image);
                    }
                }
                Event::Thread { id, messages }
                    if self
                        .selected
                        .as_ref()
                        .is_some_and(|message| message.thread_id == id) =>
                {
                    self.conversation = messages;
                    for message in self.conversation.clone() {
                        if self.message_open(&message.id)
                            && !self.thread_images.contains_key(&message.id)
                        {
                            self.dispatch(Command::Images {
                                generation: self.image_generation,
                                message: message.clone(),
                                remote: self.auto_images
                                    || self.manual_images.contains(&message.id),
                            });
                        }
                    }
                }
                Event::Images {
                    id,
                    images,
                    remote,
                    generation,
                } if generation == self.image_generation
                    && self.conversation.iter().any(|message| message.id == id)
                    && (!remote || self.auto_images || self.manual_images.contains(&id)) =>
                {
                    self.html_resources.insert(
                        id.clone(),
                        images
                            .iter()
                            .filter(|(_, bytes)| image_from_bytes(bytes.clone()).is_some())
                            .cloned()
                            .collect(),
                    );
                    let images: Vec<_> = images
                        .into_iter()
                        .filter_map(|(name, bytes)| {
                            image_from_bytes(bytes).map(|image| (name, image))
                        })
                        .collect();
                    if self
                        .selected
                        .as_ref()
                        .is_some_and(|message| message.id == id)
                    {
                        self.images = images.clone();
                    }
                    self.thread_images.insert(id, images);
                }
                Event::Saved { original, id } => {
                    self.compose_busy = false;
                    if self.draft == original {
                        self.draft.gmail_id = id;
                        let _ = self.store.put("composer", &self.draft);
                    }
                    self.status = "Draft saved to Gmail".into();
                }
                Event::Sent => {
                    self.compose_busy = false;
                    self.set_draft(Draft::default(), window, cx);
                    self.composing = false;
                    self.status = "Sent".into();
                }
                Event::ComposeFailed(error) => {
                    self.compose_busy = false;
                    self.review_send = false;
                    self.error = error;
                }
                _ => {}
            }
        }
        let agent_events: Vec<_> = self
            .assistant_events
            .as_ref()
            .map(|r| r.try_iter().collect())
            .unwrap_or_default();
        let agent_notify = !agent_events.is_empty();
        for event in agent_events {
            match event {
                codex::Event::Progress(status) => self.status = status,
                codex::Event::Done(result) => {
                    self.assistant_busy = false;
                    match result {
                        Ok(proposal) => {
                            self.proposal = Some(proposal);
                            self.status = "Codex proposal ready".into();
                        }
                        Err(error) => self.error = error.to_string(),
                    }
                }
            }
        }
        if self.contacts_due.is_some_and(|due| Instant::now() >= due) {
            self.contacts_due = None;
            if self.contact_query.len() >= 2 {
                if self.demo {
                    self.contacts = vec![
                        Contact {
                            resource_name: "people/demo-mira".into(),
                            name: "Mira Chen".into(),
                            email: "mira@example.test".into(),
                            ..Default::default()
                        },
                        Contact {
                            resource_name: "people/demo-mira".into(),
                            name: "Mira Chen".into(),
                            email: "mira.work@example.test".into(),
                            ..Default::default()
                        },
                        Contact {
                            resource_name: "people/demo-jun".into(),
                            name: "Jun Park".into(),
                            email: "jun@example.test".into(),
                            ..Default::default()
                        },
                    ]
                    .into_iter()
                    .filter(|c| {
                        format!("{} {}", c.name, c.email)
                            .to_lowercase()
                            .contains(&self.contact_query.to_lowercase())
                    })
                    .collect();
                    cx.notify();
                } else {
                    self.dispatch(Command::Contacts {
                        query: self.contact_query.clone(),
                    });
                }
            }
        }
        if notify || agent_notify {
            cx.notify();
        }
    }
    fn assist(&mut self, cx: &mut Context<Self>) {
        if self.assistant_busy {
            return;
        }
        let instruction = self.instruction.read(cx).value().to_string();
        if instruction.trim().is_empty() {
            return;
        }
        self.proposal_target = self.selected.as_ref().map(|m| m.id.clone());
        self.proposal_draft = Some(self.draft.clone());
        self.proposal = None;
        self.error.clear();
        if self.demo {
            self.proposal=Some(Proposal{explanation:"Synthetic demo proposal. A connected session uses Codex to follow your instruction.".into(),to:"Mira Chen <mira@example.test>".into(),subject:"Re: A quieter place for your inbox".into(),body:"Hi Mira,\n\nI like this direction. A fast inbox and a calm reading space are exactly what I had in mind. I'll send you my notes before Thursday.\n\nThanks!".into(),action:Action::Draft,reply_to_selected:true});
            cx.notify();
            return;
        }
        let Some(dir) = &self.dir else {
            return;
        };
        let dir = dir.join("assistant");
        let selected = self.selected.clone();
        let draft = self.draft.clone();
        let (tx, rx) = mpsc::channel();
        self.assistant_events = Some(rx);
        self.cancel = Arc::new(AtomicBool::new(false));
        let cancel = self.cancel.clone();
        self.assistant_busy = true;
        self.status = "Connecting to Codex…".into();
        std::thread::spawn(move || {
            let result = codex::run(
                &dir,
                &instruction,
                selected.as_ref(),
                &draft,
                cancel,
                tx.clone(),
            );
            let _ = tx.send(codex::Event::Done(result));
        });
        cx.notify();
    }
    fn apply_proposal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(proposal) = self.proposal.take() else {
            return;
        };
        if self.selected.as_ref().map(|m| &m.id) != self.proposal_target.as_ref() {
            self.error = "Select the original email before applying this proposal.".into();
            cx.notify();
            return;
        }
        match proposal.action {
            Action::Draft => {
                if self.proposal_draft.as_ref() != Some(&self.draft) {
                    self.error="Your draft changed while Codex was working. Ask again to preserve those edits.".into();
                    cx.notify();
                    return;
                }
                let mut draft = if proposal.reply_to_selected {
                    self.selected.as_ref().map(Draft::reply).unwrap_or_default()
                } else {
                    let mut draft = self.draft.clone();
                    draft.thread_id.clear();
                    draft.in_reply_to.clear();
                    draft.references.clear();
                    draft
                };
                draft.to = proposal.to;
                draft.subject = proposal.subject;
                draft.body = proposal.body;
                self.set_draft(draft, window, cx);
            }
            Action::None => {}
            action => {
                if let Some(id) = self.proposal_target.clone() {
                    let (add, remove) = match action {
                        Action::Archive => (vec![], vec!["INBOX".into()]),
                        Action::MarkRead => (vec![], vec!["UNREAD".into()]),
                        Action::MarkUnread => (vec!["UNREAD".into()], vec![]),
                        Action::Star => (vec!["STARRED".into()], vec![]),
                        _ => (vec![], vec![]),
                    };
                    self.modify(id, add, remove, cx);
                }
            }
        }
        cx.notify();
    }
    fn demo_image(&mut self) {
        if std::env::var("GMAILTJIK_DEMO_SCENE").as_deref() == Ok("html") {
            let image = image::RgbaImage::from_fn(240, 60, |x, _| {
                if x < 120 {
                    image::Rgba([255, 83, 166, 255])
                } else {
                    image::Rgba([70, 160, 220, 255])
                }
            });
            let mut bytes = std::io::Cursor::new(Vec::new());
            if image.write_to(&mut bytes, image::ImageFormat::Png).is_ok() {
                self.html_resources.insert(
                    "demo-0".into(),
                    vec![("cid:demo-art".into(), bytes.into_inner())],
                );
            }
        }
        if self.selected.as_ref().is_some_and(|m| m.id == "demo-0") {
            let image = Image::from_bytes(
                ImageFormat::Svg,
                include_bytes!("../assets/demo-card.svg").to_vec(),
            );
            self.images = vec![(
                "Synthetic transparent artwork · original colours".into(),
                Arc::new(image),
            )];
        }
    }
    fn set_label_visibility(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(path) = self.label_menu.take() {
            let mode = [
                crate::labels::Visibility::Visible,
                crate::labels::Visibility::Unread,
                crate::labels::Visibility::Hidden,
            ][index];
            self.label_visibility.insert(path, mode);
            if let Err(error) = self.store.put("label_visibility", &self.label_visibility) {
                self.error = error.to_string();
            }
        }
        cx.notify();
    }

    fn toggle_label(&mut self, path: &str, cx: &mut Context<Self>) {
        if !self.collapsed_labels.remove(path) {
            self.collapsed_labels.insert(path.into());
        }
        let _ = self.store.put("collapsed_labels", &self.collapsed_labels);
        cx.notify();
    }
    fn select_rule(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.capture_rule(cx);
        self.rule_index = index;
        if let Some(rule) = self.rules.get(index) {
            for (input, value) in
                self.rule_fields
                    .iter()
                    .zip([&rule.name, &rule.label, &rule.sender, &rule.subject])
            {
                input.update(cx, |input, cx| input.set_value(value.clone(), window, cx));
            }
        }
        cx.notify();
    }
    fn capture_rule(&mut self, cx: &Context<Self>) {
        if let Some(rule) = self.rules.get_mut(self.rule_index) {
            rule.name = self.rule_fields[0].read(cx).value().trim().into();
            rule.label = self.rule_fields[1].read(cx).value().trim().into();
            rule.sender = self.rule_fields[2].read(cx).value().trim().into();
            rule.subject = self.rule_fields[3].read(cx).value().trim().into();
        }
    }
    fn remote_images_control(&self, cx: &mut Context<Self>) -> AnyElement {
        Button::new("auto-images")
            .ghost()
            .small()
            .label(if self.auto_images {
                "Remote images: Auto"
            } else {
                "Remote images: Ask"
            })
            .on_click(cx.listener(|this, _, _, cx| {
                this.auto_images = !this.auto_images;
                this.image_generation += 1;
                let _ = this.store.put("auto_remote_images", &this.auto_images);
                this.manual_images.clear();
                this.images.clear();
                this.thread_images.clear();
                this.html_resources.clear();
                if this.demo {
                    this.demo_image();
                } else {
                    for message in this
                        .conversation
                        .clone()
                        .into_iter()
                        .filter(|message| this.message_open(&message.id))
                        .collect::<Vec<_>>()
                    {
                        this.dispatch(Command::Images {
                            generation: this.image_generation,
                            message: message.clone(),
                            remote: this.auto_images,
                        });
                    }
                }
                cx.notify();
            }))
            .into_any_element()
    }
    fn sync_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let status = self
            .store
            .get::<String>("push_status")
            .ok()
            .flatten()
            .unwrap_or_else(|| "Polling every 30 seconds · push not configured".into());
        let mut form=div().flex().flex_col().gap_3().p_5().rounded_xl().bg(rgb(SURFACE))
            .child(div().text_lg().child("Gmail Pub/Sub"))
            .child(div().text_color(rgb(MUTED)).child("A pull subscription receives mailbox-change signals without a public webhook. The topic must belong to your Gmail OAuth project's Google Cloud project. Use a separate subscription for each account instance."))
            .child(Button::new("push-enabled").label(if self.push_enabled {"Push enabled"}else{"Push disabled"}).on_click(cx.listener(|this,_,_,cx|{this.push_enabled=!this.push_enabled;this.push_note="Unsaved changes".into();cx.notify();})));
        for (index, label) in [
            "Topic · projects/PROJECT/topics/TOPIC",
            "Pull subscription · projects/PROJECT/subscriptions/SUBSCRIPTION",
            "Private Cloud credentials file · authorized_user or service_account JSON",
        ]
        .into_iter()
        .enumerate()
        {
            form = form.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, window, cx| {
                            this.push_fields[index]
                                .read(cx)
                                .focus_handle(cx)
                                .focus(window, cx)
                        }),
                    )
                    .child(div().child(label))
                    .child(Input::new(&self.push_fields[index])),
            );
        }
        form = form
            .child(
                Button::new("save-push")
                    .primary()
                    .label("Save sync settings")
                    .on_click(cx.listener(|this, _, _, cx| {
                        let config = crate::push::Config {
                            enabled: this.push_enabled,
                            topic: this.push_fields[0].read(cx).value().trim().into(),
                            subscription: this.push_fields[1].read(cx).value().trim().into(),
                            credentials_file: PathBuf::from(
                                this.push_fields[2].read(cx).value().trim(),
                            ),
                        };
                        this.push_note = if let Some(dir) = &this.dir {
                            match crate::push::save(dir, &config) {
                                Ok(()) => {
                                    "Saved · the background service will apply this automatically"
                                        .into()
                                }
                                Err(e) => e.to_string(),
                            }
                        } else {
                            "Saved for this synthetic preview".into()
                        };
                        cx.notify();
                    })),
            )
            .child(div().text_color(rgb(ACCENT)).child(self.push_note.clone()));
        div().id("sync-settings-scroll").flex_1().min_w_0().h_full().overflow_y_scroll().p_6().flex().flex_col().gap_4()
            .child(div().text_2xl().child("Sync"))
            .child(div().text_color(rgb(ACCENT)).child(status))
            .child(div().text_color(rgb(MUTED)).child("Push wakes the background service promptly. Watches renew daily. A catch-up runs every five minutes while push is connected; polling resumes every 30 seconds if push fails."))
            .child(form).into_any_element()
    }

    fn settings_layout(&self, content: AnyElement, cx: &mut Context<Self>) -> AnyElement {
        div()
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .bg(rgb(BG))
            .text_color(rgb(TEXT))
            .text_sm()
            .font_family("tijpset")
            .child(
                div()
                    .w(px(210.))
                    .h_full()
                    .p_4()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .child(div().text_lg().py_4().child("Settings"))
                    .child(
                        Button::new("back-to-mail")
                            .ghost()
                            .icon(proicon("arrow-left"))
                            .label("Back to mail")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.capture_rule(cx);
                                this.settings = false;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("settings-rules")
                            .ghost()
                            .label("Rules")
                            .bg(rgb(if self.settings_reading || self.settings_sync {
                                BG
                            } else {
                                RAISED
                            }))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.settings_reading = false;
                                this.settings_sync = false;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("settings-reading")
                            .ghost()
                            .label("Reading")
                            .bg(rgb(if self.settings_reading { RAISED } else { BG }))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.capture_rule(cx);
                                this.settings_reading = true;
                                this.settings_sync = false;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("settings-sync")
                            .ghost()
                            .label("Sync")
                            .bg(rgb(if self.settings_sync { RAISED } else { BG }))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.capture_rule(cx);
                                this.settings_reading = false;
                                this.settings_sync = true;
                                if let Some(config) = this
                                    .dir
                                    .as_ref()
                                    .and_then(|dir| crate::push::load(dir).ok().flatten())
                                {
                                    this.push_enabled = config.enabled;
                                    for (field, value) in this.push_fields.iter().zip([
                                        config.topic,
                                        config.subscription,
                                        config.credentials_file.display().to_string(),
                                    ]) {
                                        field.update(cx, |field, cx| {
                                            field.set_value(value, window, cx)
                                        });
                                    }
                                }
                                cx.notify();
                            })),
                    ),
            )
            .child(content)
            .into_any_element()
    }
    fn reading_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        div().flex_1().min_w_0().h_full().p_6().flex().flex_col().gap_4()
            .child(div().text_2xl().child("Reading"))
            .child(div().text_color(rgb(MUTED)).child("Choose how email content loads. Preferences are saved for this account."))
            .child(div().p_5().rounded_xl().bg(rgb(SURFACE)).flex().flex_col().gap_3()
                .child(div().text_lg().child("Remote images"))
                .child(div().text_color(rgb(MUTED)).child("Automatically load remote images when opening mail, or ask before loading them."))
                .child(self.remote_images_control(cx)))
            .into_any_element()
    }

    fn settings_view(&self, cx: &mut Context<Self>) -> AnyElement {
        if self.settings_sync {
            return self.settings_layout(self.sync_settings(cx), cx);
        }
        if self.settings_reading {
            return self.settings_layout(self.reading_settings(cx), cx);
        }
        let sync_status = self
            .store
            .get::<i64>("background_last_sync")
            .ok()
            .flatten()
            .and_then(chrono::DateTime::from_timestamp_millis)
            .map(|time| {
                format!(
                    "Last email sync: {}",
                    time.with_timezone(&chrono::Local).format("%H:%M:%S")
                )
            })
            .unwrap_or_else(|| "Waiting for the first email sync".into());
        let mut list = div().flex().flex_col().gap_2();
        for (index, rule) in self.rules.iter().enumerate() {
            list = list.child(
                Button::new(("rule", index))
                    .when(index == self.rule_index, |button| button.primary())
                    .label(format!(
                        "{}  {}{}",
                        index + 1,
                        rule.name,
                        if rule.enabled { "" } else { " · disabled" }
                    ))
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.select_rule(index, window, cx)),
                    ),
            );
        }
        let mut editor = div()
            .flex()
            .flex_col()
            .gap_3()
            .p_5()
            .rounded_xl()
            .bg(rgb(SURFACE))
            .min_w_0()
            .flex_1();
        if let Some(rule) = self.rules.get(self.rule_index) {
            editor = editor
                .child(
                    div()
                        .text_lg()
                        .child(format!("Rule {}", self.rule_index + 1)),
                )
                .child(
                    Button::new("rule-enabled")
                        .label(if rule.enabled { "Enabled" } else { "Disabled" })
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.rules[this.rule_index].enabled =
                                !this.rules[this.rule_index].enabled;
                            this.rules_note = "Unsaved changes".into();
                            cx.notify();
                        })),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, window, cx| {
                                this.rule_fields[0]
                                    .read(cx)
                                    .focus_handle(cx)
                                    .focus(window, cx)
                            }),
                        )
                        .child(div().child("Name"))
                        .child(Input::new(&self.rule_fields[0])),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, window, cx| {
                                this.rule_fields[1]
                                    .read(cx)
                                    .focus_handle(cx)
                                    .focus(window, cx)
                            }),
                        )
                        .child(div().child("Label name or ID · leave blank for all mail"))
                        .child(Input::new(&self.rule_fields[1])),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, window, cx| {
                                this.rule_fields[2]
                                    .read(cx)
                                    .focus_handle(cx)
                                    .focus(window, cx)
                            }),
                        )
                        .child(div().child("Sender contains · optional"))
                        .child(Input::new(&self.rule_fields[2])),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, window, cx| {
                                this.rule_fields[3]
                                    .read(cx)
                                    .focus_handle(cx)
                                    .focus(window, cx)
                            }),
                        )
                        .child(div().child("Subject contains · optional"))
                        .child(Input::new(&self.rule_fields[3])),
                )
                .child(
                    div()
                        .text_color(rgb(MUTED))
                        .child("All filters must match. Text matching ignores case."),
                )
                .child(div().text_lg().mt_2().child("Handler"));
            for (index, handler) in [
                crate::rules::Handler::AuthCode,
                crate::rules::Handler::EmailNotification,
                crate::rules::Handler::Silent,
            ]
            .into_iter()
            .enumerate()
            {
                editor = editor.child(
                    Button::new(("handler", index))
                        .label(format!(
                            "{} {}",
                            if handler == rule.handler {
                                "●"
                            } else {
                                "○"
                            },
                            handler.label()
                        ))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.rules[this.rule_index].handler = handler;
                            this.rules_note = "Unsaved changes".into();
                            cx.notify();
                        })),
                );
            }
            editor=editor.child(div().text_color(rgb(MUTED)).child("The authentication handler copies a code on the receiving device. If no code is found, the next rule is tried. A handled message produces no additional email notification."))
                .child(div().flex().gap_2()
                    .child(Button::new("rule-up").label("Move up").disabled(self.rule_index==0).on_click(cx.listener(|this,_,_,cx| {this.capture_rule(cx);let i=this.rule_index;if i>0{this.rules.swap(i,i-1);this.rule_index-=1;}this.rules_note="Unsaved changes".into();cx.notify();})))
                    .child(Button::new("rule-down").label("Move down").disabled(self.rule_index+1==self.rules.len()).on_click(cx.listener(|this,_,_,cx| {this.capture_rule(cx);let i=this.rule_index;if i+1<this.rules.len(){this.rules.swap(i,i+1);this.rule_index+=1;}this.rules_note="Unsaved changes".into();cx.notify();})))
                    .child(Button::new("rule-delete").label("Remove rule").on_click(cx.listener(|this,_,window,cx| {this.rules.remove(this.rule_index);this.rule_index=usize::MAX;this.select_rule(0,window,cx);this.rules_note="Unsaved changes · Save rules to apply".into();cx.notify();}))));
        } else {
            editor=editor.child("No rules. New mail will sync without notifications. Add a rule to choose a handler.");
        }
        let content = div().id("rules-scroll").flex_1().min_w_0().h_full().overflow_y_scroll().p_6().flex().flex_col().gap_4()
                .child(div().text_2xl().child("Rules"))
                    .child(div().text_color(rgb(MUTED)).child(sync_status))
                .child(div().text_color(rgb(MUTED)).child("Choose what happens when new unread mail arrives. Rules run in order; the first successful handler wins."))
                .child(div().flex().gap_3()
                    .child(Button::new("add-rule").label("+ Add rule").disabled(self.rules.len()>=100).on_click(cx.listener(|this,_,window,cx| {this.capture_rule(cx);this.rules.push(crate::rules::Rule{name:"New rule".into(),enabled:false,..Default::default()});this.select_rule(this.rules.len()-1,window,cx);this.rules_note="Unsaved changes".into();cx.notify();})))
                    .child(Button::new("save-rules").primary().label("Save rules").on_click(cx.listener(|this,_,_,cx| {this.capture_rule(cx);this.rules_note=match crate::rules::save(&this.store,&this.rules){Ok(())=>"Saved · applies on the next sync; existing mail is not replayed".into(),Err(error)=>error.to_string()};cx.notify();}))))
                .child(div().text_color(rgb(ACCENT)).child(self.rules_note.clone()))
                .child(div().flex().gap_4().child(div().w(px(240.)).flex_shrink_0().child(list)).child(editor))
                .child(div().text_color(rgb(MUTED)).child(if self.demo {"Demo settings · synthetic account".into()} else {format!("Account: {} · rules are stored only for this account",self.email)}))
            .into_any_element();
        self.settings_layout(content, cx)
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> AnyElement {
        let sidebar = div()
            .flex()
            .flex_col()
            .w(px(210.))
            .flex_shrink_0()
            .h_full()
            .p_4()
            .gap_2()
            .bg(rgb(BG))
            .child(
                div().flex().items_center().gap_2().h(px(28.)).child(
                    img(Arc::new(Image::from_bytes(
                        ImageFormat::Svg,
                        include_bytes!("../assets/gmailtjik.svg").to_vec(),
                    )))
                    .size(px(28.))
                    .flex_shrink_0(),
                ),
            )
            .child(
                div()
                    .mt_6()
                    .mb_2()
                    .text_xs()
                    .text_color(rgb(MUTED))
                    .truncate()
                    .child(if self.email.is_empty() {
                        "Not connected".into()
                    } else {
                        self.email.clone()
                    }),
            );
        let labels = crate::labels::visible_rows(
            &self.labels,
            &self.collapsed_labels,
            &self.label_visibility,
            self.show_hidden_labels,
        );
        let mut navigation = div()
            .id("label-scroll")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll();
        for label in labels {
            let id = label.id.clone().unwrap_or_default();
            let active = (!id.is_empty() && self.label == id)
                || (self.zone == 0 && self.label_cursor == label.path);
            let path = label.path.clone();
            let color = label.color;
            let expanded = !self.collapsed_labels.contains(&path);
            let icon = match id.as_str() {
                "INBOX" => "mail",
                "STARRED" => "star",
                "SENT" => "send",
                "DRAFT" => "compose",
                "TRASH" => "delete",
                _ => "folder",
            };
            let menu_path = path.clone();
            let context_path = path.clone();
            let manage_path = path.clone();
            navigation = navigation.child(
                div()
                    .id(SharedString::from(format!("label-{path}")))
                    .flex()
                    .items_center()
                    .gap_2()
                    .pl(px(8. + label.depth as f32 * 14.))
                    .pr_2()
                    .py_2()
                    .rounded_lg()
                    .cursor_pointer()
                    .bg(rgb(if active { RAISED } else { BG }))
                    .hover(|s| s.bg(rgb(SURFACE)))
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, _, window, cx| {
                            this.zone = 0;
                            this.label_cursor = context_path.clone();
                            this.label_menu = Some(context_path.clone());
                            this.label_menu_index = 0;
                            this.focus.focus(window, cx);
                            cx.stop_propagation();
                            cx.notify();
                        }),
                    )
                    .text_color(rgb(color))
                    .child({
                        let path = path.clone();
                        div()
                            .id(SharedString::from(format!("toggle-{path}")))
                            .w_4()
                            .flex_shrink_0()
                            .text_color(rgb(color))
                            .child(
                                proicon(if label.children {
                                    if expanded {
                                        "chevron-down"
                                    } else {
                                        "chevron-right"
                                    }
                                } else {
                                    icon
                                })
                                .small(),
                            )
                            .on_click(cx.listener(move |this, _, window, cx| {
                                if label.children {
                                    this.zone = 0;
                                    this.focus.focus(window, cx);
                                    this.label_cursor = path.clone();
                                    this.toggle_label(&path, cx);
                                    cx.stop_propagation();
                                }
                            }))
                    })
                    .child(
                        div()
                            .id(SharedString::from(format!("label-name-{path}")))
                            .flex_1()
                            .truncate()
                            .child(label.name)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.zone = 0;
                                this.focus.focus(window, cx);
                                this.label_cursor = path.clone();
                                if id.is_empty() {
                                    this.toggle_label(&path, cx);
                                    return;
                                }
                                this.label = id.clone();
                                this.query.clear();
                                this.search.update(cx, |s, cx| s.set_value("", window, cx));
                                this.previous.clear();
                                this.page_token.clear();
                                this.load(cx);
                            })),
                    )
                    .when(label.unread > 0, |row| {
                        row.child(
                            div()
                                .flex_shrink_0()
                                .text_xs()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(label.unread.to_string()),
                        )
                    })
                    .when(self.show_hidden_labels, |row| {
                        row.child(
                            Button::new(SharedString::from(format!("manage-{manage_path}")))
                                .ghost()
                                .small()
                                .icon(proicon("settings"))
                                .tooltip("Label visibility")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.label_menu = Some(manage_path.clone());
                                    this.label_menu_index = 0;
                                    cx.notify();
                                })),
                        )
                    }),
            );
            if self.label_menu.as_deref() == Some(&menu_path) {
                let selected = crate::labels::preference(&menu_path, &self.label_visibility);
                let mut menu = div()
                    .flex()
                    .flex_col()
                    .p_2()
                    .gap_1()
                    .bg(rgb(SURFACE))
                    .border_1()
                    .border_color(rgb(BORDER))
                    .rounded_lg();
                for (index, (title, mode)) in [
                    ("Always show", crate::labels::Visibility::Visible),
                    ("Only when unread", crate::labels::Visibility::Unread),
                    ("Hidden", crate::labels::Visibility::Hidden),
                ]
                .into_iter()
                .enumerate()
                {
                    menu = menu.child(
                        Button::new(SharedString::from(format!("label-visibility-{index}")))
                            .ghost()
                            .small()
                            .label(format!(
                                "{} {}",
                                if selected == mode { "✓" } else { " " },
                                title
                            ))
                            .bg(rgb(if self.label_menu_index == index {
                                RAISED
                            } else {
                                SURFACE
                            }))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.set_label_visibility(index, cx)
                            })),
                    );
                }
                navigation = navigation.child(menu);
            }
        }
        sidebar
            .child(navigation)
            .child(
                Button::new("manage-labels")
                    .ghost()
                    .small()
                    .label(if self.show_hidden_labels {
                        "Done managing labels"
                    } else {
                        "Manage labels"
                    })
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.show_hidden_labels = !this.show_hidden_labels;
                        this.label_menu = None;
                        cx.notify();
                    })),
            )
            .child(
                div().flex().items_center().child(
                    Button::new("settings")
                        .ghost()
                        .icon(proicon("settings"))
                        .tooltip("Settings")
                        .w(px(36.))
                        .h(px(36.))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.settings = true;
                            cx.notify();
                        })),
                ),
            )
            .into_any_element()
    }
    fn inbox(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let title = if self.label.is_empty() {
            "Search results".into()
        } else {
            self.labels
                .iter()
                .find(|l| l.id == self.label)
                .map(|l| crate::labels::display_name(&l.name))
                .unwrap_or_else(|| "Inbox".into())
        };
        div()
            .flex()
            .flex_col()
            .w_full()
            .min_w_0()
            .h_full()
            .bg(rgb(SURFACE))
            .rounded_xl()
            .overflow_hidden()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .p_4()
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(title),
                    )
                    .child(
                        Button::new("refresh")
                            .ghost()
                            .small()
                            .icon(proicon("arrow-sync"))
                            .tooltip("Refresh mail")
                            .disabled(self.loading)
                            .on_click(cx.listener(|this, _, _, cx| this.load(cx))),
                    ),
            )
            .child(
                div().flex().flex_wrap().gap_2().px_4().pb_3()
                    .child(Button::new("unread-only").small().icon(proicon("mail"))
                        .label("Unread").tooltip("Show unread emails only")
                        .bg(rgb(if self.unread_only { ACCENT } else { RAISED }))
                        .text_color(rgb(if self.unread_only { BG } else { TEXT }))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.unread_only = !this.unread_only;
                            this.page_token.clear();
                            this.previous.clear();
                            this.load(cx);
                        })))
                    .child(Button::new("mark-all-read").small().ghost().icon(proicon("mail-open"))
                        .label(if self.marking_all_read { "Marking read…" } else { "Mark all read" })
                        .tooltip("Mark all unread messages in this label or search as read, across all pages")
                        .disabled(self.marking_all_read || self.loading)
                        .on_click(cx.listener(|this, _, _, cx| this.mark_all_read(cx))))
            )
            .child(
                div()
                    .px_4()
                    .pb_3()
                    .text_xs()
                    .text_color(rgb(MUTED))
                    .child(format!(
                        "{} conversation{}{}",
                        self.page.conversations().len(),
                        if self.page.conversations().len() == 1 { "" } else { "s" },
                        if self.loading { " · syncing" } else { "" }
                    )),
            )
            .child(
                uniform_list(
                    "mail-list",
                    self.page.conversations().len(),
                    cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                        let conversations = this.page.conversations();
                        range
                            .map(|index| {
                                let row = &conversations[index];
                                let message = row.message();
                                let active =
                                    this.selected.as_ref().is_some_and(|m| m.conversation_key() == message.conversation_key());
                                div()
                                    .id(("mail", index))
                                    .h(px(104.))
                                    .p_4()
                                    .border_b_1()
                                    .border_color(rgb(BORDER))
                                    .cursor_pointer()
                                    .bg(rgb(if active { RAISED } else { SURFACE }))
                                    .hover(|s| s.bg(rgb(RAISED)))
                                    .child(
                                        div()
                                            .flex()
                                            .gap_2()
                                            .items_center()
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .truncate()
                                                    .font_weight(if row.unread() {
                                                        FontWeight::SEMIBOLD
                                                    } else {
                                                        FontWeight::NORMAL
                                                    })
                                                    .child(row.participants(&this.email)),
                                            )
                                            .when(row.messages.len() > 1, |d| d.child(
                                                div().flex_shrink_0().text_xs().text_color(rgb(MUTED))
                                                    .child(row.messages.len().to_string())
                                            ))
                                            .when(row.unread(), |d| {
                                                d.child(
                                                    div()
                                                        .size(px(6.))
                                                        .rounded_full()
                                                        .bg(rgb(ACCENT)),
                                                )
                                            }),
                                    )
                                    .child(
                                        div().mt_1().truncate().text_sm().child(row.messages[0].subject()),
                                    )
                                    .child(
                                        div()
                                            .mt_1()
                                            .truncate()
                                            .text_sm()
                                            .text_color(rgb(MUTED))
                                            .child(row.latest().snippet.clone()),
                                    )
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.zone = 1;
                                        this.focus.focus(window, cx);
                                        this.select(index, cx);
                                    }))
                            })
                            .collect::<Vec<_>>()
                    }),
                )
                .track_scroll(&self.mail_scroll)
                .flex_1()
                .min_h_0(),
            )
            .when(self.page.messages.is_empty(), |d| {
                d.child(div().p_6().text_color(rgb(MUTED)).child(if self.loading {
                    "Loading mail…"
                } else {
                    "No messages in this view."
                }))
            })
            .child(
                div()
                    .flex()
                    .justify_between()
                    .p_3()
                    .child(
                        Button::new("previous")
                            .ghost()
                            .small()
                            .icon(proicon("arrow-left"))
                            .label("Newer")
                            .disabled(self.previous.is_empty() || self.loading)
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(token) = this.previous.pop() {
                                    this.page_token = token;
                                    this.load(cx);
                                }
                            })),
                    )
                    .child(
                        Button::new("next")
                            .ghost()
                            .small()
                            .icon(proicon("arrow-right"))
                            .label("Older")
                            .disabled(self.page.next.is_empty() || self.loading)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.previous.push(this.page_token.clone());
                                this.page_token = this.page.next.clone();
                                this.load(cx);
                            })),
                    ),
            )
            .into_any_element()
    }
    fn message_open(&self, id: &str) -> bool {
        self.selected
            .as_ref()
            .is_some_and(|message| message.id == id)
            || self
                .conversation
                .last()
                .is_some_and(|message| message.id == id)
            || self.expanded_messages.contains(id)
    }
    fn reader(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let Some(message) = self.selected.clone() else {
            return div()
                .flex_1()
                .p_8()
                .text_color(rgb(MUTED))
                .child("A little room to read. Select an email to begin.")
                .into_any_element();
        };
        let id = message.id.clone();
        let unread = self
            .page
            .conversations()
            .iter()
            .find(|row| row.message().conversation_key() == message.conversation_key())
            .map(|row| row.unread())
            .unwrap_or_else(|| message.unread());
        let starred = message.label_ids.iter().any(|l| l == "STARRED");
        let read_id = id.clone();
        let mut reader = div()
            .id("reader")
            .track_scroll(&self.reader_scroll)
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .p_6()
            .gap_4()
            .child(
                div()
                    .flex()
                    .gap_2()
                    .flex_wrap()
                    .child(
                        Button::new("reply")
                            .icon(proicon("arrow-reply"))
                            .label("Reply")
                            .tooltip("Reply (r)")
                            .on_click(cx.listener(|this, _, window, cx| {
                                if let Some(message) = this.selected.clone() {
                                    this.reply(&message, window, cx);
                                }
                            })),
                    )
                    .child(
                        Button::new("archive")
                            .ghost()
                            .icon(proicon("archive"))
                            .label("Archive")
                            .tooltip("Archive (a)")
                            .on_click(cx.listener(|this, _, _, cx| this.archive_selected(cx))),
                    )
                    .child(
                        Button::new("trash")
                            .ghost()
                            .icon(proicon("delete"))
                            .label("Delete")
                            .tooltip("Move to Trash (d)")
                            .disabled(message.label_ids.iter().any(|l| l == "TRASH"))
                            .on_click(cx.listener(|this, _, _, cx| this.trash_selected(cx))),
                    )
                    .child(
                        Button::new("read-state")
                            .ghost()
                            .icon(proicon(if unread { "mail-open" } else { "mail" }))
                            .label(if unread { "Mark read" } else { "Mark unread" })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.modify(
                                    read_id.clone(),
                                    if unread {
                                        vec![]
                                    } else {
                                        vec!["UNREAD".into()]
                                    },
                                    if unread {
                                        vec!["UNREAD".into()]
                                    } else {
                                        vec![]
                                    },
                                    cx,
                                )
                            })),
                    )
                    .child(
                        Button::new("star")
                            .ghost()
                            .icon(proicon("star"))
                            .text_color(rgb(if starred { ACCENT } else { TEXT }))
                            .tooltip(if starred { "Unstar" } else { "Star" })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.modify(
                                    id.clone(),
                                    if starred {
                                        vec![]
                                    } else {
                                        vec!["STARRED".into()]
                                    },
                                    if starred {
                                        vec!["STARRED".into()]
                                    } else {
                                        vec![]
                                    },
                                    cx,
                                )
                            })),
                    ),
            )
            .child(
                div()
                    .mt_3()
                    .text_2xl()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(message.subject()),
            );
        reader = reader.child(
            div()
                .flex()
                .justify_between()
                .text_xs()
                .text_color(rgb(MUTED))
                .child(format!(
                    "{} messages · oldest first",
                    self.conversation.len().max(1)
                ))
                .child(
                    Button::new("image-backing")
                        .ghost()
                        .small()
                        .label(if self.light_images {
                            "Dark email / image backing"
                        } else {
                            "Light email / image backing"
                        })
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.light_images = !this.light_images;
                            cx.notify();
                        })),
                ),
        );
        let messages = if self.conversation.is_empty() {
            vec![message]
        } else {
            self.conversation.clone()
        };
        for message in messages {
            let id = message.id.clone();
            if !self.message_open(&id) {
                let message = message.clone();
                reader = reader.child(
                    Button::new(SharedString::from(format!("expand-{id}")))
                        .ghost()
                        .label(format!(
                            "▸ {} · {}",
                            message.header("From"),
                            crate::quotes::body(&message)
                                .0
                                .chars()
                                .take(65)
                                .collect::<String>()
                        ))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.expanded_messages.insert(message.id.clone());
                            if !this.demo && !this.thread_images.contains_key(&message.id) {
                                this.dispatch(Command::Images {
                                    generation: this.image_generation,
                                    message: message.clone(),
                                    remote: this.auto_images,
                                });
                            }
                            cx.notify();
                        })),
                );
                continue;
            }
            let (trimmed, quoted) = crate::quotes::body(&message);
            let expanded = self.expanded_quotes.contains(&id);
            let body = if expanded { message.text() } else { trimmed };
            let mut card = div()
                .flex()
                .flex_col()
                .gap_3()
                .p_4()
                .rounded_lg()
                .border_1()
                .border_color(rgb(BORDER))
                .child(
                    div()
                        .flex()
                        .justify_between()
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(message.header("From")),
                        )
                        .child(
                            div().text_xs().text_color(rgb(MUTED)).child(
                                message
                                    .internal_date
                                    .parse::<i64>()
                                    .ok()
                                    .and_then(chrono::DateTime::from_timestamp_millis)
                                    .map(|date| {
                                        date.with_timezone(&chrono::Local)
                                            .format("%d %b · %H:%M")
                                            .to_string()
                                    })
                                    .unwrap_or_default(),
                            ),
                        ),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(MUTED))
                        .child(format!("To: {}", message.header("To"))),
                );
            let html = crate::html::source(&message);
            let use_html = html.is_some() && !self.text_messages.contains(&id);
            if let Some(html) = html {
                let toggle_id = id.clone();
                card = card.child(
                    Button::new(SharedString::from(format!("html-toggle-{id}")))
                        .ghost()
                        .small()
                        .label(if use_html {
                            "Text · select and copy"
                        } else {
                            "HTML · Blitz"
                        })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if !this.text_messages.remove(&toggle_id) {
                                this.text_messages.insert(toggle_id.clone());
                            }
                            cx.notify();
                        })),
                );
                if use_html {
                    let key = self.html_renderer.request(
                        html,
                        self.html_width,
                        expanded,
                        self.light_images,
                        self.html_resources.get(&id).cloned().unwrap_or_default(),
                    );
                    match self.html_renderer.cache.get(&key) {
                        Some(Ok(page)) => {
                            let mut tiles = div()
                                .relative()
                                .flex()
                                .flex_col()
                                .gap_0()
                                .w_full()
                                .overflow_hidden();
                            if self.html_tiles.len() >= 12 && !self.html_tiles.contains_key(&key) {
                                self.html_tiles.clear();
                            }
                            let images = self.html_tiles.entry(key).or_insert_with(|| {
                                page.tiles
                                    .iter()
                                    .map(|(h, bytes)| {
                                        (
                                            *h,
                                            Arc::new(Image::from_bytes(
                                                ImageFormat::Png,
                                                bytes.clone(),
                                            )),
                                        )
                                    })
                                    .collect()
                            });
                            for (height, image) in images.iter() {
                                tiles = tiles.child(
                                    img(image.clone())
                                        .w(px(self.html_width as f32))
                                        .h(px(*height as f32))
                                        .flex_shrink_0(),
                                );
                            }
                            for (index, area) in page.link_areas.iter().enumerate() {
                                let url = area.url.clone();
                                let [x, y, width, height] = area.rect;
                                tiles = tiles.child(
                                    div()
                                        .id(SharedString::from(format!("html-hit-{id}-{index}")))
                                        .absolute()
                                        .left(px(x))
                                        .top(px(y))
                                        .w(px(width))
                                        .h(px(height))
                                        .cursor_pointer()
                                        .on_click(move |_, _, cx| cx.open_url(&url)),
                                );
                            }
                            card = card.child(tiles);
                            for (index, (label, url)) in page.links.iter().enumerate() {
                                let url = url.clone();
                                card = card.child(
                                    Button::new(SharedString::from(format!(
                                        "html-link-{id}-{index}"
                                    )))
                                    .ghost()
                                    .small()
                                    .icon(proicon("open"))
                                    .label(format!(
                                        "{}",
                                        if label.trim().is_empty() { &url } else { label }
                                    ))
                                    .on_click(move |_, _, cx| cx.open_url(&url)),
                                );
                            }
                        }
                        Some(Err(error)) => {
                            card =
                                card.child(div().text_color(rgb(MUTED)).child(error.to_string()));
                        }
                        None => {
                            card = card.child("Laying out HTML…");
                        }
                    }
                }
            }
            if !use_html {
                card = card.child(
                    TextView::html(
                        SharedString::from(format!("thread-body-{id}-{expanded}")),
                        escaped_body(if body.trim().is_empty() && quoted {
                            "(Quoted reply)"
                        } else {
                            &body
                        }),
                    )
                    .selectable(true),
                );
            }
            if quoted {
                let id = id.clone();
                card = card.child(
                    Button::new(SharedString::from(format!("quote-{id}")))
                        .ghost()
                        .small()
                        .label(if expanded {
                            "Hide quoted history"
                        } else {
                            "Show quoted history"
                        })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if !this.expanded_quotes.remove(&id) {
                                this.expanded_quotes.insert(id.clone());
                            }
                            cx.notify();
                        })),
                );
            }
            let reply = message.clone();
            card = card.child(
                Button::new(SharedString::from(format!("reply-{id}")))
                    .ghost()
                    .small()
                    .icon(proicon("arrow-reply"))
                    .label("Reply to this message")
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.reply(&reply, window, cx);
                    })),
            );
            if !self.auto_images
                && !message.remote_images().is_empty()
                && !self.manual_images.contains(&id)
            {
                let message = message.clone();
                card = card.child(
                    Button::new(SharedString::from(format!("images-{id}")))
                        .ghost()
                        .small()
                        .label("Load remote images")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.manual_images.insert(message.id.clone());
                            this.dispatch(Command::Images {
                                generation: this.image_generation,
                                message: message.clone(),
                                remote: true,
                            });
                            cx.notify();
                        })),
                );
            }
            let images = if self
                .selected
                .as_ref()
                .is_some_and(|selected| selected.id == id)
            {
                self.thread_images.get(&id).or(Some(&self.images))
            } else {
                self.thread_images.get(&id)
            };
            if let Some(images) = images.filter(|_| !use_html) {
                for (name, image) in images {
                    card = card
                        .child(
                            div()
                                .rounded_lg()
                                .overflow_hidden()
                                .p_3()
                                .bg(rgb(if self.light_images { 0xf4f1ec } else { BG }))
                                .child(
                                    img(image.clone())
                                        .w_full()
                                        .h(px(210.))
                                        .object_fit(ObjectFit::Contain),
                                ),
                        )
                        .child(div().text_xs().text_color(rgb(MUTED)).child(name.clone()));
                }
            }
            reader = reader.child(card);
        }
        reader.into_any_element()
    }
    fn migrate_contact_defaults(&mut self) {
        let mut defaults = self.contact_defaults.clone();
        if crate::contacts::migrate_defaults(&mut defaults, &self.known_contacts) {
            match self.store.put("contact_defaults", &defaults) {
                Ok(()) => self.contact_defaults = defaults,
                Err(error) => self.error = format!("Could not migrate default addresses: {error}"),
            }
        }
    }
    fn toggle_contact_default(
        &mut self,
        person: &crate::contacts::Person,
        contact: &Contact,
        cx: &mut Context<Self>,
    ) {
        if self.compose_busy {
            return;
        }
        let mut defaults = self.contact_defaults.clone();
        crate::contacts::toggle_default(&mut defaults, person, contact);
        if let Err(error) = self.store.put("contact_defaults", &defaults) {
            self.error = format!("Could not save default address: {error}");
            cx.notify();
            return;
        }
        self.contact_defaults = defaults;
        let mut index = 0;
        for group in crate::contacts::ordered_people(&self.contacts, &self.contact_defaults) {
            if group.key == person.key && group.addresses.len() == 1 {
                self.contact_cursor = Some(index);
                break;
            }
            index += 1;
            if self.expanded_contact.as_ref() == Some(&group.key) {
                if group.key == person.key {
                    self.contact_cursor = group
                        .addresses
                        .iter()
                        .position(|address| address.email.eq_ignore_ascii_case(&contact.email))
                        .map(|offset| index + offset);
                    break;
                }
                index += group.addresses.len();
            }
        }
        cx.notify();
    }
    fn contact_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.contacts.is_empty()
            || self.compose_busy
            || ![&self.to, &self.cc, &self.bcc]
                .iter()
                .any(|field| field.read(cx).focus_handle(cx).is_focused(window))
            || event.keystroke.modifiers.control
            || event.keystroke.modifiers.alt
            || event.keystroke.modifiers.platform
        {
            return;
        }
        let people = crate::contacts::ordered_people(&self.contacts, &self.contact_defaults);
        let mut rows = Vec::new();
        for (person, group) in people.iter().enumerate() {
            rows.push((person, None));
            if self.expanded_contact.as_ref() == Some(&group.key) {
                for address in 0..group.addresses.len() {
                    rows.push((person, Some(address)));
                }
            }
        }
        let index = self.contact_cursor.unwrap_or(0).min(rows.len() - 1);
        match event.keystroke.key.as_str() {
            "down" => {
                self.contact_cursor = Some(
                    self.contact_cursor
                        .map_or(0, |_| (index + 1).min(rows.len() - 1)),
                )
            }
            "up" => self.contact_cursor = Some(index.saturating_sub(1)),
            "enter" => {
                let (person, address) = rows[index];
                let person = &people[person];
                if let Some(address) = address {
                    self.choose_contact(&person.addresses[address], window, cx);
                } else if person.addresses.len() == 1 {
                    self.choose_contact(&person.addresses[0], window, cx);
                } else {
                    self.expanded_contact = Some(person.key.clone());
                    self.contact_cursor = Some(index + 1);
                }
            }
            "d" if self.contact_cursor.is_some() => {
                let (person, address) = rows[index];
                let person = &people[person];
                let address = address.or_else(|| (person.addresses.len() == 1).then_some(0));
                let Some(address) = address else {
                    return;
                };
                self.toggle_contact_default(person, &person.addresses[address], cx);
            }
            "escape" => {
                self.contacts.clear();
                self.contact_cursor = None;
                self.contacts_due = None;
                self.contact_query.clear();
            }
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }
    fn choose_contact(&mut self, contact: &Contact, window: &mut Window, cx: &mut Context<Self>) {
        if self.compose_busy {
            return;
        }
        let field = [&self.to, &self.cc, &self.bcc][self.contact_field].clone();
        let value = field.read(cx).value();
        let prefix = value
            .rfind([',', ';'])
            .map(|index| format!("{} ", &value[..=index]))
            .unwrap_or_default();
        field.update(cx, |state, cx| {
            state.set_value(format!("{prefix}{}", contact.email), window, cx)
        });
        self.contacts.clear();
        self.expanded_contact = None;
        self.contact_cursor = None;
        self.contacts_due = None;
        self.contact_query.clear();
        self.changed(cx);
        field.read(cx).focus_handle(cx).focus(window, cx);
    }
    fn default_address_button(
        &self,
        person: &crate::contacts::Person,
        contact: &Contact,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let preferred = self
            .contact_defaults
            .get(&person.key)
            .is_some_and(|default| default.email.eq_ignore_ascii_case(&contact.email));
        let person = person.clone();
        let contact = contact.clone();
        Button::new(SharedString::from(format!(
            "default-{}-{}",
            person.key, contact.email
        )))
        .ghost()
        .small()
        .disabled(self.compose_busy)
        .label(if preferred {
            "★ Default · d"
        } else {
            "☆ Set default · d"
        })
        .on_click(cx.listener(move |this, _, window, cx| {
            this.toggle_contact_default(&person, &contact, cx);
            [&this.to, &this.cc, &this.bcc][this.contact_field]
                .read(cx)
                .focus_handle(cx)
                .focus(window, cx);
        }))
        .into_any_element()
    }
    fn composer(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut composer = div()
            .id("composer")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .p_6()
            .gap_3()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_xl()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Compose"),
                    )
                    .child(
                        Button::new("close-compose")
                            .ghost()
                            .label("Close · keep draft")
                            .disabled(self.compose_busy)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.composing = false;
                                this.focus.focus(window, cx);
                                cx.notify();
                            })),
                    ),
            )
            .child(Input::new(&self.to).readonly(self.compose_busy));
        for warning in
            crate::contacts::warnings(&self.draft.to, &self.known_contacts, &self.contact_defaults)
        {
            composer = composer.child(
                div()
                    .px_3()
                    .py_2()
                    .rounded_lg()
                    .bg(rgb(RAISED))
                    .text_sm()
                    .text_color(rgb(0xf6c177))
                    .child(format!("⚠ {warning}")),
            );
        }
        let people = crate::contacts::ordered_people(&self.contacts, &self.contact_defaults);
        if !people.is_empty() {
            let mut suggestions = div()
                .id("recipient-suggestions")
                .flex()
                .flex_col()
                .max_h(px(280.))
                .overflow_y_scroll()
                .rounded_lg()
                .border_1()
                .border_color(rgb(BORDER))
                .bg(rgb(BG))
                .p_2()
                .gap_1();
            let mut row_index = 0;
            for person in people {
                let expanded = self.expanded_contact.as_ref() == Some(&person.key);
                let multiple = person.addresses.len() > 1;
                let avatar = if let Some(photo) = self.contact_photos.get(&person.photo) {
                    div()
                        .size(px(32.))
                        .flex_shrink_0()
                        .rounded_full()
                        .overflow_hidden()
                        .child(img(photo.clone()).size_full().object_fit(ObjectFit::Cover))
                        .into_any_element()
                } else {
                    div()
                        .size(px(32.))
                        .flex_shrink_0()
                        .rounded_full()
                        .bg(rgb(RAISED))
                        .text_color(rgb(ACCENT))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_xs()
                        .child(crate::contacts::initials(&person.name))
                        .into_any_element()
                };
                let target = person.clone();
                let person_index = row_index;
                row_index += 1;
                suggestions = suggestions.child(
                    div()
                        .id(SharedString::from(format!("person-{}", person.key)))
                        .flex()
                        .items_center()
                        .gap_3()
                        .p_2()
                        .rounded_lg()
                        .cursor_pointer()
                        .hover(|style| style.bg(rgb(RAISED)))
                        .bg(rgb(if self.contact_cursor == Some(person_index) {
                            RAISED
                        } else {
                            BG
                        }))
                        .child(avatar)
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .flex_1()
                                .min_w_0()
                                .child(div().truncate().child(person.name.clone()))
                                .child(div().text_xs().text_color(rgb(MUTED)).truncate().child(
                                    if multiple {
                                        format!(
                                            "{} addresses{}",
                                            person.addresses.len(),
                                            self.contact_defaults
                                                .get(&person.key)
                                                .map(|default| format!(
                                                    " · Default: {}",
                                                    default.email
                                                ))
                                                .unwrap_or_default()
                                        )
                                    } else {
                                        person.addresses[0].email.clone()
                                    },
                                )),
                        )
                        .child(if multiple {
                            if expanded { "▾" } else { "▸" }
                        } else {
                            ""
                        })
                        .on_click(cx.listener(move |this, _, window, cx| {
                            if this.compose_busy {
                                return;
                            }
                            if target.addresses.len() == 1 {
                                this.choose_contact(&target.addresses[0], window, cx);
                            } else {
                                this.expanded_contact =
                                    if this.expanded_contact.as_ref() == Some(&target.key) {
                                        None
                                    } else {
                                        Some(target.key.clone())
                                    };
                                cx.notify();
                            }
                        })),
                );
                if !multiple {
                    suggestions =
                        suggestions.child(div().ml(px(44.)).child(self.default_address_button(
                            &person,
                            &person.addresses[0],
                            cx,
                        )));
                }
                if multiple && expanded {
                    for contact in &person.addresses {
                        let contact = contact.clone();
                        let address_index = row_index;
                        row_index += 1;
                        let choose = contact.clone();
                        suggestions = suggestions.child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .ml(px(44.))
                                .rounded_lg()
                                .bg(rgb(if self.contact_cursor == Some(address_index) {
                                    RAISED
                                } else {
                                    BG
                                }))
                                .child(
                                    div()
                                        .id(SharedString::from(format!(
                                            "address-{}-{}",
                                            person.key, contact.email
                                        )))
                                        .flex_1()
                                        .min_w_0()
                                        .px_3()
                                        .py_2()
                                        .rounded_lg()
                                        .cursor_pointer()
                                        .truncate()
                                        .hover(|style| style.bg(rgb(RAISED)))
                                        .text_sm()
                                        .child(contact.email.clone())
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.choose_contact(&choose, window, cx)
                                        })),
                                )
                                .child(self.default_address_button(&person, &contact, cx)),
                        );
                    }
                }
            }
            composer = composer.child(suggestions.child(
                div().px_2().py_1().text_xs().text_color(rgb(MUTED)).child(
                    "↑/↓ choose · Enter select/expand · d toggle default for highlighted address",
                ),
            ));
        }
        composer
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(Input::new(&self.cc).readonly(self.compose_busy))
                    .child(Input::new(&self.bcc).readonly(self.compose_busy)),
            )
            .child(Input::new(&self.subject).readonly(self.compose_busy))
            .child(
                Textarea::new(&self.body)
                    .h(px(300.))
                    .readonly(self.compose_busy),
            )
            .child(div().text_xs().text_color(rgb(MUTED)).child(if self.demo {
                "Synthetic draft · nothing is sent"
            } else {
                "Draft saved on this device as you type"
            }))
            .when(self.review_send, |d| {
                d.child(div().p_3().rounded_lg().bg(rgb(RAISED)).child(format!(
                    "Send “{}” to {}{}{}?",
                    self.draft.subject,
                    self.draft.to,
                    if self.draft.cc.is_empty() {
                        String::new()
                    } else {
                        format!(" · Cc: {}", self.draft.cc)
                    },
                    if self.draft.bcc.is_empty() {
                        String::new()
                    } else {
                        format!(" · Bcc: {}", self.draft.bcc)
                    }
                )))
            })
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        Button::new("send")
                            .primary()
                            .label(if self.compose_busy {
                                "Working…"
                            } else if self.review_send {
                                "Confirm send"
                            } else {
                                "Send →"
                            })
                            .disabled(self.compose_busy || (!self.demo && self.email.is_empty()))
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Err(error) = this.draft.validate() {
                                    this.error = error.to_string();
                                } else if !this.review_send {
                                    this.review_send = true;
                                } else if this.demo {
                                    this.review_send = false;
                                    this.status = "Demo only: no message sent".into();
                                } else {
                                    this.compose_busy = true;
                                    this.dispatch(Command::Send(this.draft.clone()));
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("save-gmail")
                            .label("Save to Gmail")
                            .disabled(self.compose_busy || self.demo || self.email.is_empty())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.compose_busy = true;
                                this.dispatch(Command::Save(this.draft.clone()));
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("clear-draft")
                            .ghost()
                            .label("Clear text")
                            .disabled(self.compose_busy)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.set_draft(Draft::default(), window, cx);
                            })),
                    ),
            )
            .into_any_element()
    }
    fn assistant(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .p_4()
            .gap_3()
            .border_t_1()
            .border_color(rgb(BORDER))
            .bg(rgb(SURFACE))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(proicon("sparkle").text_color(rgb(ACCENT)))
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Codex"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(MUTED))
                            .child("Draft, refine, or organise this email"),
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(Input::new(&self.instruction))
                    .child(
                        Button::new("ask-codex")
                            .label(if self.assistant_busy {
                                "Cancel"
                            } else {
                                "Ask →"
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                if this.assistant_busy {
                                    this.cancel.store(true, Ordering::Relaxed);
                                } else {
                                    this.assist(cx);
                                }
                            })),
                    ),
            )
            .when_some(self.proposal.clone(), |d, proposal| {
                d.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .p_3()
                        .rounded_lg()
                        .bg(rgb(RAISED))
                        .child(proposal.explanation)
                        .when(!proposal.body.is_empty(), |d| {
                            d.child(
                                div()
                                    .max_h(px(120.))
                                    .id("proposal-preview")
                                    .overflow_y_scroll()
                                    .text_sm()
                                    .child(proposal.body),
                            )
                        })
                        .child(
                            Button::new("apply-proposal")
                                .small()
                                .label(match proposal.action {
                                    Action::Draft => "Use this draft",
                                    Action::Archive => "Archive selected email",
                                    Action::MarkRead => "Mark selected email read",
                                    Action::MarkUnread => "Mark selected email unread",
                                    Action::Star => "Star selected email",
                                    Action::None => "No action proposed",
                                })
                                .disabled(
                                    self.compose_busy || matches!(proposal.action, Action::None),
                                )
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.apply_proposal(window, cx)
                                })),
                        ),
                )
            })
            .into_any_element()
    }
}
impl Drop for MailApp {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}
impl Render for MailApp {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.settings {
            return self.settings_view(cx);
        }
        let inbox = self.inbox(cx);
        let content = if self.composing {
            self.composer(cx)
        } else {
            self.reader(cx)
        };
        div()
            .track_focus(&self.focus)
            .capture_key_down(cx.listener(Self::contact_key))
            .on_key_down(cx.listener(Self::key))
            .size_full()
            .flex()
            .bg(rgb(BG))
            .text_color(rgb(TEXT))
            .text_sm()
            .font_family("tijpset")
            .child(self.sidebar(cx))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .pr_4()
                    .py_4()
                    .gap_3()
                    .when(!self.demo && self.email.is_empty(), |d| d.child(
                        div().flex().flex_col().gap_2().p_4().bg(rgb(RAISED)).rounded_lg()
                            .child("Connect Gmail · Enable Gmail and People APIs in your Google Cloud project, then choose a Desktop OAuth client.")
                            .child(Input::new(&self.oauth_path))
                            .child(Button::new("connect-google").primary()
                                .label(if self.connecting { "Waiting for browser sign-in…" } else { "Connect Google + contacts" })
                                .disabled(self.connecting)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    let path = this.oauth_path.read(cx).value().trim().to_string();
                                    if path.is_empty() { this.error = "Enter the private OAuth client JSON path.".into(); }
                                    else { this.connecting = true; this.error.clear(); this.dispatch(Command::Connect(PathBuf::from(path))); }
                                    cx.notify();
                                })))
                    ))
                    .when(self.shortcuts, |d| d.child(
                        div().p_4().rounded_lg().bg(rgb(RAISED))
                            .child("Keyboard shortcuts · ? help · Esc return to navigation · ←/→ labels / messages / reader · ↑/↓ select · Enter/Space fold folder · r reply · a archive · d move to Trash (default address in suggestions) · S label visibility · Ctrl+F search · Ctrl+N compose")
                    ))
                    .child(
                        div().flex().items_center().gap_3()
                            .child(div().flex_1().min_w_0().child(Input::new(&self.search)))
                    .child(
                        Button::new("compose")
                            .primary()
                            .icon(proicon("compose"))
                            .label(if self.composing {
                                "Continue draft"
                            } else {
                                "Compose"
                            })
                            .flex_shrink_0()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.composing = true;
                                this.to.read(cx).focus_handle(cx).focus(window, cx);
                                cx.notify();
                            })),
                    )
                    )
                    .child(
                        div().flex().flex_1().min_h_0().child(
                        h_resizable("mail-panes").with_state(&self.panes)
                            .on_resize(cx.listener(|this, state: &Entity<ResizableState>, _, cx| {
                                if let Some(width) = state.read(cx).sizes().first() {
                                    this.inbox_width = f32::from(*width);
                                    let _ = this.store.put("inbox_width", &this.inbox_width);
                                }
                                cx.notify();
                            }))
                            .child(resizable_panel().size(px(self.inbox_width)).size_range(px(220.)..px(700.)).child(inbox))
                            .child(resizable_panel().size_range(px(360.)..Pixels::MAX).child(
                            div()
                                .flex()
                                .flex_col()
                                .flex_1()
                                .min_w_0()
                                .h_full()
                                .rounded_xl()
                                .bg(rgb(SURFACE))
                                .overflow_hidden()
                                .child(content)
                                .child(self.assistant(cx)),
                        ))),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(if self.error.is_empty() {
                                MUTED
                            } else {
                                0xeb6f92
                            }))
                            .child(if self.error.is_empty() {
                                format!("{} · Keyboard: {} · ? shortcuts", self.status, ["labels", "messages", "reader"][self.zone])
                            } else {
                                self.error.clone()
                            }),
                    ),
            ).into_any_element()
    }
}

fn image_from_bytes(bytes: Vec<u8>) -> Option<Arc<Image>> {
    let reader = image::ImageReader::new(std::io::Cursor::new(&bytes))
        .with_guessed_format()
        .ok()?;
    let (width, height) = reader.into_dimensions().ok()?;
    if width > 8192 || height > 8192 || u64::from(width) * u64::from(height) > 32_000_000 {
        return None;
    }
    let format = if bytes.starts_with(b"\x89PNG") {
        ImageFormat::Png
    } else if bytes.starts_with(b"\xff\xd8") {
        ImageFormat::Jpeg
    } else if bytes.starts_with(b"GIF8") {
        ImageFormat::Gif
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        ImageFormat::Webp
    } else {
        return None;
    };
    Some(Arc::new(Image::from_bytes(format, bytes)))
}

fn escaped_body(text: &str) -> String {
    format!(
        "<p>{}</p>",
        text.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('\n', "<br>")
    )
}

// Capture the neighbour before filtering: archive may remove the row from Inbox,
// but retain it in search results or a custom label.
fn successor(messages: &[Message], id: &str) -> Option<String> {
    let index = messages.iter().position(|message| message.id == id)?;
    messages
        .get(index + 1)
        .or_else(|| {
            index
                .checked_sub(1)
                .and_then(|previous| messages.get(previous))
        })
        .map(|message| message.id.clone())
}

#[cfg(test)]
mod navigation_tests {
    use super::successor;
    use crate::model;
    #[test]
    fn archive_selects_next_then_previous_at_end_and_clears_last() {
        let messages = model::demo().1.messages;
        assert_eq!(
            successor(&messages, &messages[0].id),
            Some(messages[1].id.clone())
        );
        assert_eq!(
            successor(&messages, &messages[2].id),
            Some(messages[3].id.clone())
        );
        assert_eq!(
            successor(&messages, &messages[4].id),
            Some(messages[3].id.clone())
        );
        assert_eq!(successor(&messages[..1], &messages[0].id), None);
        assert_eq!(successor(&messages, "no-longer-visible"), None);
    }
}
