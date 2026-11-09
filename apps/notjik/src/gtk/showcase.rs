//! Private synthetic demo controls. Uses production cards, never live actions.
use super::{model::*, primitives::*, Card};
use anyhow::{ensure, Context, Result};
use gtk::{gdk, prelude::*};
use gtk4_layer_shell::{KeyboardMode, Layer, LayerShell};
use serde_json::{json, Value};
use std::{collections::HashMap, path::Path, rc::Rc, time::Instant};

pub(super) struct Showcase {
    pub states: HashMap<String, ViewState>,
    stage: Option<Stage>,
    last: Instant,
}
struct Stage {
    _window: gtk::ApplicationWindow,
    title: gtk::Label,
    caption: gtk::Label,
    counter: gtk::Label,
    montage: gtk::Fixed,
    headings: [gtk::Label; 2],
    pointer: gtk::ApplicationWindow,
    canvas: gtk::Fixed,
    cards: Vec<(Rc<Card>, gtk::Label, Instant)>,
    intro: bool,
    intro_changed: Instant,
}
impl Showcase {
    pub fn new(app: &gtk::Application, config: &Value) -> Self {
        let stage = std::env::var_os("NOTJIK_DEMO_CYCLE_DIR").map(|_| {
            let window = gtk::ApplicationWindow::builder().application(app).title("Notjik GTK showcase").default_width(1000).default_height(680).decorated(false).resizable(false).build();
            window.add_css_class("notjik-demo-stage");
            window.add_css_class("notjik");
            window.init_layer_shell();
            window.set_namespace(Some("notjik-demo-stage"));
            window.set_layer(Layer::Top);
            window.set_exclusive_zone(-1);
            window.set_keyboard_mode(KeyboardMode::None);
            tjik_gtk::surface::monitor(&window, &text(&config["placement"]["primaryMonitor"]));
            let css = gtk::CssProvider::new();
            css.load_from_string("window.notjik-demo-stage {background:#15131f;color:#e5e5e5;} .demo-title {font-size:25px;font-weight:bold;} .demo-caption {font-size:14px;color:#a5a0ba;} .demo-brand {font-size:16px;color:#ff53a6;font-weight:bold;} window.notjik-demo-pointer {background:transparent;} .demo-pointer {color:#fff;font-size:24px;} ");
            gtk::style_context_add_provider_for_display(&gdk::Display::default().unwrap(), &css, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION+2);
            let canvas = gtk::Fixed::new();
            let brand = stage_label("Dunst vs Notjik", "demo-brand");canvas.put(&brand,48.,20.);
            let counter = stage_label("", "demo-caption");canvas.put(&counter,860.,20.);
            let title = stage_label("Notjik", "demo-title");title.set_size_request(1000,-1);title.set_xalign(0.5);canvas.put(&title,0.,55.);
            let caption = stage_label("A robot for every feeling", "demo-caption");caption.set_size_request(1000,-1);caption.set_xalign(0.5);canvas.put(&caption,0.,100.);
            let headings = [stage_label("DUNST", "demo-caption"), stage_label("NOTJIK", "demo-brand")];
            canvas.put(&headings[0],240.,140.);
            canvas.put(&headings[1],690.,140.);
            for heading in &headings { heading.set_visible(false); }
            let montage=gtk::Fixed::new();montage.set_size_request(1000,500);canvas.put(&montage,0.,145.);
            let pointer=gtk::ApplicationWindow::builder().application(app).default_width(32).default_height(32).decorated(false).resizable(false).build();
            pointer.add_css_class("notjik-demo-pointer");
            pointer.init_layer_shell();pointer.set_namespace(Some("notjik-demo-pointer"));pointer.set_layer(Layer::Overlay);pointer.set_exclusive_zone(-1);pointer.set_keyboard_mode(KeyboardMode::None);pointer.set_anchor(gtk4_layer_shell::Edge::Left,true);pointer.set_anchor(gtk4_layer_shell::Edge::Top,true);pointer.set_monitor(window.monitor().as_ref());
            pointer.set_child(Some(&stage_label("➤","demo-pointer")));
            pointer.connect_map(|w|{if let Some(surface)=w.surface(){surface.set_input_region(&gtk::cairo::Region::create());}});
            let overlay=gtk::Overlay::new();overlay.set_child(Some(&gtk::Box::new(gtk::Orientation::Vertical,0)));overlay.add_overlay(&canvas);overlay.set_measure_overlay(&canvas,false);overlay.set_clip_overlay(&canvas,true);window.set_child(Some(&overlay));window.present();
            window.connect_map(|w| { if let Some(surface)=w.surface(){surface.set_input_region(&gtk::cairo::Region::create());} });
            if let Some(surface)=window.surface(){surface.set_input_region(&gtk::cairo::Region::create());}
            Stage { _window: window,title,caption,counter,montage,headings,pointer,canvas,cards:vec![],intro:false,intro_changed:Instant::now() }
        });
        Self {
            states: HashMap::new(),
            stage,
            last: Instant::now(),
        }
    }
    pub fn tick(&mut self) {
        let dt = self.last.elapsed().as_millis() as u64;
        self.last = Instant::now();
        for state in self.states.values_mut() {
            if state.playback.playing {
                state.playback.position_ms =
                    (state.playback.position_ms + dt).min(state.playback.duration_ms);
                if state.playback.position_ms == state.playback.duration_ms {
                    state.playback.playing = false;
                }
            }
            if state.recording.phase == "recording" {
                state.recording.duration_ms += dt;
            }
        }
        if let Some(stage) = &self.stage {
            let opacity = if stage.intro {
                1.
            } else {
                (1. - stage.intro_changed.elapsed().as_secs_f64() / 0.45).clamp(0., 1.)
            };
            stage
                .montage
                .set_visible(opacity > 0. && !stage.cards.is_empty());
            stage.montage.set_opacity(opacity);
            let mut columns = [0.; 4];
            for (i, (card, label, at)) in stage.cards.iter().enumerate() {
                let progress =
                    ((at.elapsed().as_secs_f64() - i as f64 * 0.065) / 0.48).clamp(0., 1.);
                let eased = 1. - (1. - progress).powi(3);
                let x = 48. + (i % 4) as f64 * 232.;
                let y = columns[i % 4];
                columns[i % 4] += card.widget.height().max(80) as f64 * 0.495 + 37.;
                stage.montage.move_(label, x, y);
                label.set_opacity(progress);
                card.widget.set_opacity(progress);
                let scale = (0.495 * (0.82 + 0.18 * eased)) as f32;
                stage.montage.set_child_transform(
                    &card.widget,
                    Some(
                        &gtk::gsk::Transform::new()
                            .translate(&gtk::graphene::Point::new(
                                x as f32,
                                (y + 22. + 38. * (1. - eased)) as f32,
                            ))
                            .scale(scale, scale),
                    ),
                );
            }
        }
    }

    pub fn event(&mut self, card: &Card, intent: &Intent) -> bool {
        let n = card.notice();
        let state = self.states.entry(n.id()).or_default();
        match intent {
            Intent::PlaybackToggle => {
                state.playback.playing = !state.playback.playing;
                state.playback.duration_ms = n.0["data"]["duration"]
                    .as_f64()
                    .map(|v| (v * 1000.) as u64)
                    .unwrap_or(8000);
                state.playback.seekable = true;
            }
            Intent::Seek(ms) => state.playback.position_ms = *ms,
            Intent::PlaybackRate(rate) => state.playback.rate = *rate,
            Intent::Record => {
                state.playback.playing = false;
                state.recording = Recording {
                    phase: "recording".into(),
                    ..Default::default()
                };
            }
            Intent::RecordSend => {
                state.recording = Default::default();
                state.feedback = "Demo voice reply sent".into();
            }
            Intent::RecordDiscard => state.recording = Default::default(),
            Intent::Telegram { action, .. } => {
                state.feedback = format!("Demo {action} completed");
                if action == "reply" {
                    card.clear_reply();
                }
            }
            Intent::Task { .. } | Intent::Invoke(_) => {
                state.feedback = "Demo action completed".into()
            }
            _ => return false,
        }
        true
    }
    pub fn apply(&self, id: &str, state: &mut ViewState) {
        if let Some(demo) = self.states.get(id) {
            state.playback = demo.playback.clone();
            state.recording = demo.recording.clone();
            state.feedback = demo.feedback.clone();
        }
    }
    pub fn control(&mut self, root: &Path, r: &Value, card: Option<&Rc<Card>>) -> Result<Value> {
        if let Some(stage) = &mut self.stage {
            for (key, label) in [
                ("title", &stage.title),
                ("caption", &stage.caption),
                ("counter", &stage.counter),
            ] {
                if let Some(s) = r[key].as_str() {
                    label.set_text(s);
                }
            }
            if let Some(intro) = r["intro"].as_bool() {
                if stage.intro != intro {
                    stage.intro_changed = Instant::now();
                }
                stage.intro = intro;
                for heading in &stage.headings {
                    heading.set_visible(!intro);
                }
            }
            if r["outro"] == true {
                for heading in &stage.headings {
                    heading.set_visible(false);
                }
                stage.canvas.move_(&stage.title, 0., 300.);
                stage.canvas.move_(&stage.caption, 0., 345.);
            }
            if r["hidePointer"] == true {
                stage.pointer.set_visible(false);
            }
            if let Some(cards) = r["introCards"].as_array() {
                for (card, label, _) in stage.cards.drain(..) {
                    stage.montage.remove(&card.widget);
                    stage.montage.remove(&label);
                }
                for (i, n) in cards.iter().enumerate() {
                    let card = Rc::new(Card::new(
                        Notice(n.clone()),
                        ViewState {
                            locked: false,
                            ..Default::default()
                        },
                        root,
                        super::host::now() as f64,
                        |_| {},
                    ));
                    card.widget.set_size_request(420, -1);
                    card.widget.set_can_target(false);
                    let title = stage_label(&text(&n["demoLabel"]), "demo-caption");
                    title.set_size_request(208, -1);
                    title.set_max_width_chars(1);
                    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
                    stage.montage.put(&title, 48. + (i % 4) as f64 * 232., 0.);
                    stage.montage.put(&card.widget, 0., 0.);
                    stage.cards.push((card, title, Instant::now()));
                }
            }
        }
        if r["id"].is_null() {
            return Ok(json!({"ok":true,"renderer":"gtk"}));
        }
        let card = card.context("Demo card not ready")?;
        ensure!(
            card.notice().0["app"] == crate::demo::APP,
            "Only synthetic demo cards may be controlled"
        );
        if let Some(value) = r["text"].as_str() {
            let input = find(&card.widget, "telegram-reply")
                .context("Reply editor unavailable")?
                .downcast::<gtk::TextView>()
                .map_err(|_| anyhow::anyhow!("Reply editor is not a text view"))?;
            input.buffer().set_text(value);
        }
        if let Some(target) = r["target"].as_str() {
            card.set_hovered(true);
            let name = match target {
                "telegram-play" => "voice-play",
                "telegram-record-send" => "record-send",
                _ => target,
            };
            let name = name.replace("cycle-action-", "card-action-");
            let widget = if target == "card" || target == "group" {
                card.widget.clone().upcast::<gtk::Widget>()
            } else {
                find(&card.widget, &name).context(format!("Demo control unavailable: {name}"))?
            };
            if let Some(stage) = &self.stage {
                if let Some(origin) = widget.root().and_then(|root| {
                    widget.compute_point(
                        &root.downcast::<gtk::Window>().ok()?,
                        &gtk::graphene::Point::new(widget.width() as f32 / 2., 20.),
                    )
                }) {
                    stage
                        .pointer
                        .set_margin(gtk4_layer_shell::Edge::Left, origin.x().max(0.) as i32);
                    stage
                        .pointer
                        .set_margin(gtk4_layer_shell::Edge::Top, origin.y().max(0.) as i32);
                    stage.pointer.set_visible(true);
                }
            }
            if r["click"] == true {
                if target == "group" {
                    card.context.emit(Intent::OpenGroup);
                } else {
                    let button = widget
                        .downcast::<gtk::Button>()
                        .map_err(|_| anyhow::anyhow!("Demo target is not a button"))?;
                    ensure!(button.is_sensitive(), "Demo button disabled");
                    button.emit_clicked();
                }
            }
        }
        let state = self
            .states
            .get(&card.notice().id())
            .cloned()
            .unwrap_or_default();
        Ok(
            json!({"ok":true,"renderer":"gtk","revealed":card.context.revealed(),"draft":card.draft(),"replying":card.context.composing.get(),"feedback":state.feedback,"playing":state.playback.playing,"recording":state.recording.phase,"duration":state.recording.duration_ms as f64/1000.}),
        )
    }
}
fn find(widget: &impl IsA<gtk::Widget>, name: &str) -> Option<gtk::Widget> {
    let widget = widget.as_ref();
    if widget.widget_name() == name {
        return Some(widget.clone());
    }
    let mut child = widget.first_child();
    while let Some(w) = child {
        if let Some(found) = find(&w, name) {
            return Some(found);
        }
        child = w.next_sibling();
    }
    None
}

fn stage_label(text: &str, class: &str) -> gtk::Label {
    let label = label(text, class);
    label.set_wrap(false);
    label.set_max_width_chars(-1);
    label
}
