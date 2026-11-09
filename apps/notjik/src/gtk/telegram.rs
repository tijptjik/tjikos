use super::{model::*, primitives::*, Context};
use gtk::{gdk, glib, prelude::*};
use std::rc::Rc;

pub(super) struct Voice {
    play: gtk::Button,
    wave: gtk::DrawingArea,
    seek: gtk::Scale,
    elapsed: gtk::Label,
    speed: gtk::Button,
    error: gtk::Label,
}
impl Voice {
    pub fn update(&self, ctx: &Context, playing_changed: bool) {
        let state = ctx.state.borrow();
        let p = &state.playback;
        let duration = p
            .duration_ms
            .max((number(&ctx.notice.borrow().data()["duration"]) * 1000.) as u64);
        if playing_changed {
            super::icons::set_playing(&self.play, p.playing);
        }
        self.wave.queue_draw();
        self.seek.set_range(0., duration.max(1) as f64);
        self.seek.set_value(p.position_ms as f64);
        self.seek
            .set_sensitive(ctx.allowed() && p.seekable && ctx.revealed());
        // sync_reveal must keep the updated seek capability on subsequent hover changes.
        for (widget, enabled) in ctx.quiet.borrow_mut().iter_mut() {
            if widget.upgrade().is_some_and(|w| w == self.seek) {
                *enabled = ctx.allowed() && p.seekable;
            }
        }
        self.elapsed.set_text(&format!(
            "{} / {}",
            stamp(p.position_ms / 1000),
            stamp(duration / 1000)
        ));
        self.speed.set_label(&format!("{}×", p.rate));
        self.error.set_text(&p.error);
        self.error.set_visible(!p.error.is_empty());
    }
}

pub(super) fn content(ctx: &Rc<Context>, parent: &gtk::Box) {
    let n = ctx.notice.borrow().clone();
    let d = n.data();
    let kind = d["kind"].as_str().unwrap_or("text");
    if ["photo", "sticker", "emoji", "animation"].contains(&kind) {
        let media = super::media::visual(
            &text(&d["source"]),
            &if kind == "photo" {
                "photo".into()
            } else {
                text(&d["format"])
            },
            &text(&d["fallback"]),
            if kind == "emoji" { 96 } else { 190 },
        );
        if kind == "photo" {
            media.aspect(0.61, i32::MAX);
        } else if kind != "emoji" {
            let width = d["width"].as_f64().unwrap_or(320.).max(1.);
            let height = d["height"].as_f64().unwrap_or(200.);
            media.aspect(height / width, 240);
        }
        media.widget.set_widget_name(&format!("telegram-{kind}"));
        parent.append(&media.widget);
        ctx.media.borrow_mut().push(media);
    }
    if kind == "voice" {
        voice(ctx, parent);
    }
    if let Some(parts) = d["parts"].as_array().filter(|p| !p.is_empty()) {
        let fragments = gtk::TextView::new();
        fragments.set_editable(false);
        fragments.set_cursor_visible(false);
        fragments.set_wrap_mode(gtk::WrapMode::WordChar);
        fragments.add_css_class("message-parts");
        let buffer = fragments.buffer();
        for part in parts {
            let mut end = buffer.end_iter();
            if part["kind"] == "emoji" {
                let media = super::media::visual(
                    &text(&part["source"]),
                    &text(&part["format"]),
                    &text(&part["text"]),
                    24,
                );
                media.widget.set_width_request(24);
                let anchor = buffer.create_child_anchor(&mut end);
                fragments.add_child_at_anchor(&media.widget, &anchor);
                ctx.media.borrow_mut().push(media);
            } else if let Some(markup) = styled_markup(&part["styledText"]) {
                buffer.insert_markup(&mut end, &markup);
            } else {
                buffer.insert(&mut end, &text(&part["text"]));
            }
        }
        parent.append(&fragments);
    } else if !n.body().is_empty() {
        let body = styled_label(&n.body(), &d["styledText"]);
        body.set_lines(6);
        body.set_ellipsize(gtk::pango::EllipsizeMode::End);
        parent.append(&body);
    }
    for key in [
        "transcript",
        "transcriptionError",
        "status",
        "voiceReplyStatus",
    ] {
        let s = text(&d[key]);
        if !s.is_empty() {
            parent.append(&label(
                &s,
                if key == "transcript" { "body" } else { "muted" },
            ));
        }
    }
    let controls = row(12);
    controls.set_widget_name("telegram-controls");
    for (title, action) in [("Mark read", "read")] {
        let b = super::icons::button(title, &format!("telegram-{action}"), action);
        let c = ctx.clone();
        b.connect_clicked(move |_| c.telegram(action, "", false, None));
        controls.append(&b);
    }
    let record = super::icons::button("Record a voice reply", "telegram-record", "record");
    let c = ctx.clone();
    record.connect_clicked(move |_| {
        c.composing.set(false);
        if let Some(input) = c.editor.borrow().upgrade() {
            if let Some(editor) = input.parent() {
                editor.set_visible(false);
            }
        }
        c.sync_reveal();
        c.emit(Intent::Record);
    });
    record.set_sensitive(
        ctx.state.borrow().recording.phase.is_empty()
            || ctx.state.borrow().recording.phase == "idle"
            || ctx.state.borrow().recording.phase == "error",
    );
    record
        .set_visible(["", "idle", "error"].contains(&ctx.state.borrow().recording.phase.as_str()));
    controls.append(&record);
    let editor = column(6);
    editor.set_widget_name("reply-composer");
    editor.set_visible(ctx.composing.get());
    let quote = label("", "caption");
    quote.set_visible(ctx.quoted.get());
    quote.set_text(&format!(
        "Reply to {}",
        ctx.reply_target
            .borrow()
            .as_ref()
            .map(|t| t.2.as_str())
            .unwrap_or(&n.title())
    ));
    editor.append(&quote);
    let input = gtk::TextView::new();
    input.set_widget_name("telegram-reply");
    input.set_wrap_mode(gtk::WrapMode::WordChar);
    input.set_height_request(76);
    input.buffer().set_text(&ctx.draft.borrow());
    let position = input
        .buffer()
        .iter_at_offset(ctx.cursor.get().min(input.buffer().char_count()));
    input.buffer().place_cursor(&position);
    *ctx.editor.borrow_mut() = input.downgrade();
    let c = ctx.clone();
    input
        .buffer()
        .connect_cursor_position_notify(move |buffer| c.cursor.set(buffer.cursor_position()));
    let c = ctx.clone();
    input.buffer().connect_changed(move |b| {
        *c.draft.borrow_mut() = b.text(&b.start_iter(), &b.end_iter(), true).to_string()
    });
    editor.append(&input);
    for (title, quoted) in [("Quote reply", true), ("Reply", false)] {
        let b = super::icons::button(
            title,
            if quoted {
                "telegram-quote"
            } else {
                "telegram-compose"
            },
            if quoted { "quote" } else { "reply" },
        );
        let c = ctx.clone();
        let e = editor.clone();
        let q = quote.clone();
        let i = input.clone();
        b.connect_clicked(move |_| {
            if !c.allowed() {
                return;
            }
            let show = !c.composing.get() || c.quoted.get() != quoted;
            c.composing.set(show);
            c.quoted.set(quoted);
            if show {
                let n = c.notice.borrow();
                *c.reply_target.borrow_mut() = Some((
                    text(&n.data()["chatId"]),
                    text(&n.data()["messageId"]),
                    n.title(),
                ));
                q.set_text(&format!("Reply to {}", n.title()));
            }
            e.set_visible(show);
            q.set_visible(quoted);
            if show {
                i.grab_focus();
            }
            c.sync_reveal();
            c.emit(Intent::Hold(show));
        });
        controls.append(&b);
    }
    let spacer = column(0);
    spacer.set_hexpand(true);
    controls.append(&spacer);
    let open = super::icons::button("Open Telegram", "telegram-open", "open");
    let c = ctx.clone();
    open.connect_clicked(move |_| c.telegram("open", "", false, None));
    controls.append(&open);
    controls.set_sensitive(ctx.allowed());
    let section = action_footer(&controls);
    parent.append(&ctx.reveal(&section, "telegram-controls-reveal"));
    let send = super::icons::button("Send reply", "telegram-send", "send");
    send.set_halign(gtk::Align::End);
    let c = ctx.clone();
    send.connect_clicked(move |_| submit(&c));
    let valid = |s: &str| !s.trim().is_empty() && s.encode_utf16().count() <= 4096;
    send.set_sensitive(ctx.allowed() && valid(&ctx.draft.borrow()));
    let c = ctx.clone();
    let b = send.clone();
    input.buffer().connect_changed(move |buffer| {
        let s = buffer.text(&buffer.start_iter(), &buffer.end_iter(), true);
        b.set_sensitive(c.allowed() && !s.trim().is_empty() && s.encode_utf16().count() <= 4096);
    });
    editor.append(&send);
    editor.set_sensitive(ctx.allowed());
    parent.append(&editor);
    let keys = gtk::EventControllerKey::new();
    let c = ctx.clone();
    let e = editor.clone();
    keys.connect_key_pressed(move |_, key, _, mods| {
        if key == gdk::Key::Escape {
            c.composing.set(false);
            e.set_visible(false);
            c.sync_reveal();
            c.emit(Intent::Hold(false));
            return glib::Propagation::Stop;
        }
        if (key == gdk::Key::Return || key == gdk::Key::KP_Enter)
            && mods.contains(gdk::ModifierType::CONTROL_MASK)
        {
            submit(&c);
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    input.add_controller(keys);
    recording(ctx, parent);
}
fn submit(ctx: &Context) {
    let draft = ctx.draft.borrow().clone();
    if draft.trim().is_empty() || draft.encode_utf16().count() > 4096 {
        return;
    }
    let target = ctx.reply_target.borrow().clone();
    ctx.telegram("reply", &draft, ctx.quoted.get(), target.as_ref());
}
fn recording(ctx: &Rc<Context>, parent: &gtk::Box) {
    let r = ctx.state.borrow().recording.clone();
    if r.phase.is_empty() || r.phase == "idle" {
        return;
    }
    let body = column(6);
    body.set_widget_name("voice-recorder");
    let actions = row(12);
    actions.append(&label(
        &format!(
            "{} · {}",
            match r.phase.as_str() {
                "recording" => "● Recording",
                "sending" => "Sending voice reply…",
                "ready" => "Voice reply ready",
                "error" => "Recording failed",
                _ => "Preparing voice reply…",
            },
            stamp(r.duration_ms / 1000)
        ),
        "accent",
    ));
    let discard = super::icons::button("Reset recording", "record-discard", "reset");
    let c = ctx.clone();
    discard.connect_clicked(move |_| c.emit(Intent::RecordDiscard));
    discard.set_sensitive(ctx.allowed() && !["sending", "discarding"].contains(&r.phase.as_str()));
    actions.append(&discard);
    let send = super::icons::button("Send voice", "record-send", "send");
    let c = ctx.clone();
    send.connect_clicked(move |_| c.emit(Intent::RecordSend));
    send.set_sensitive(
        ctx.allowed()
            && !r.uncertain
            && (r.phase == "ready" || (r.phase == "recording" && r.duration_ms > 0)),
    );
    actions.append(&send);
    body.append(&actions);
    if !r.error.is_empty() {
        body.append(&label(&r.error, "critical"));
    }
    parent.append(&body);
}
fn voice(ctx: &Rc<Context>, parent: &gtk::Box) {
    let n = ctx.notice.borrow().clone();
    let d = n.data();
    let state = ctx.state.borrow().clone();
    let p = &state.playback;
    let area = column(4);
    let top = row(12);
    let play = super::icons::play(p.playing);
    let c = ctx.clone();
    play.connect_clicked(move |_| c.emit(Intent::PlaybackToggle));
    play.set_sensitive(
        ctx.allowed()
            && local_path(&text(&d["source"])).is_some_and(|p| p.is_file())
            && state.recording.phase != "recording",
    );
    top.append(&play);
    let wave = gtk::DrawingArea::new();
    wave.set_content_height(30);
    wave.set_hexpand(true);
    let peaks = d["waveform"].as_array().cloned().unwrap_or_default();
    let c = ctx.clone();
    wave.set_draw_func(move |widget, cr, w, h| {
        let state = c.state.borrow();
        let p = &state.playback;
        let fraction = if p.duration_ms > 0 {
            p.position_ms as f64 / p.duration_ms as f64
        } else {
            0.
        };
        let count = peaks.len().min((w / 4).max(1) as usize);
        let bar_width = (w as f64 - count.saturating_sub(1) as f64 * 2.) / count.max(1) as f64;
        let ink = widget.color();
        for i in 0..count {
            let start = i * peaks.len() / count;
            let end = ((i + 1) * peaks.len() / count)
                .max(start + 1)
                .min(peaks.len());
            let peak = peaks
                .get(start..end)
                .unwrap_or(&[])
                .iter()
                .map(number)
                .fold(0., f64::max)
                / 31.;
            let height = (peak * h as f64).max(2.);
            let color = if (i as f64 + 0.5) / count as f64 <= fraction {
                widget
                    .style_context()
                    .lookup_color("notjik_accent")
                    .unwrap_or(ink)
            } else {
                ink
            };
            cr.set_source_rgba(
                color.red() as f64,
                color.green() as f64,
                color.blue() as f64,
                1.,
            );
            let x = i as f64 * (bar_width + 2.);
            let y = (h as f64 - height) / 2.;
            let radius = (bar_width / 2.).min(height / 2.);
            cr.new_sub_path();
            for (cx, cy, start) in [
                (x + bar_width - radius, y + radius, -90.),
                (x + bar_width - radius, y + height - radius, 0.),
                (x + radius, y + height - radius, 90.),
                (x + radius, y + radius, 180.),
            ] {
                cr.arc(
                    cx,
                    cy,
                    radius,
                    start * std::f64::consts::PI / 180.,
                    (start + 90.) * std::f64::consts::PI / 180.,
                );
            }
            cr.close_path();
            let _ = cr.fill();
        }
    });
    wave.add_css_class("muted");
    let right = column(0);
    right.set_hexpand(true);
    let waveform = gtk::Overlay::new();
    waveform.set_child(Some(&wave));
    right.append(&waveform);
    top.append(&right);
    area.append(&top);
    let duration = p.duration_ms.max((number(&d["duration"]) * 1000.) as u64);
    let seek = gtk::Scale::with_range(
        gtk::Orientation::Horizontal,
        0.,
        duration.max(1) as f64,
        100.,
    );
    seek.set_widget_name("voice-seek");
    seek.add_css_class("wave-seek");
    seek.set_hexpand(true);
    seek.set_valign(gtk::Align::Fill);
    seek.set_draw_value(false);
    seek.set_value(p.position_ms as f64);
    seek.set_sensitive(ctx.allowed() && p.seekable);
    let c = ctx.clone();
    seek.connect_change_value(move |_, _, value| {
        c.emit(Intent::Seek(value.max(0.) as u64));
        glib::Propagation::Proceed
    });
    ctx.quiet(&seek);
    waveform.add_overlay(&seek);
    waveform.set_measure_overlay(&seek, false);
    let actions = row(8);
    let elapsed = label(
        &format!(
            "{} / {}",
            stamp(p.position_ms / 1000),
            stamp(duration / 1000)
        ),
        "caption",
    );
    elapsed.set_wrap(false);
    elapsed.set_max_width_chars(-1);
    elapsed.set_hexpand(true);
    actions.append(&elapsed);
    let speed = button(&format!("{}×", p.rate), "voice-speed");
    let c = ctx.clone();
    speed.connect_clicked(move |_| {
        let rates = [1., 1.25, 1.5, 2.];
        let rate = c.state.borrow().playback.rate;
        let at = rates.iter().position(|r| *r == rate).unwrap_or(0);
        c.emit(Intent::PlaybackRate(rates[(at + 1) % 4]));
    });
    speed.add_css_class("quiet");
    speed.set_sensitive(ctx.allowed());
    ctx.quiet(&speed);
    let transcribe = button("→A", "voice-transcribe");
    transcribe.add_css_class("quiet");
    transcribe.set_tooltip_text(Some(if d["transcriptionState"] == "pending" {
        "Transcribing…"
    } else {
        "Transcribe"
    }));
    let c = ctx.clone();
    transcribe.connect_clicked(move |_| c.telegram("transcribe", "", false, None));
    transcribe
        .set_sensitive(d["transcriptionState"] != "pending" && d["transcriptionState"] != "done");
    transcribe.set_sensitive(ctx.allowed() && transcribe.is_sensitive());
    ctx.quiet(&transcribe);
    actions.append(&transcribe);
    actions.append(&speed);
    right.append(&actions);
    let error = label(&p.error, "critical");
    error.set_visible(!p.error.is_empty());
    area.append(&error);
    *ctx.voice.borrow_mut() = Some(Voice {
        play,
        wave,
        seek,
        elapsed,
        speed,
        error,
    });
    parent.append(&area);
}
