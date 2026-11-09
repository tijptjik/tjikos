use super::{model::*, primitives::*, Context};
use gtk::prelude::*;
use std::rc::Rc;

pub(super) fn content(ctx: &Rc<Context>, parent: &gtk::Box) {
    let n = ctx.notice.borrow().clone();
    let d = n.data();
    match n.kind() {
        "status" => parent.append(&status(d, "Status", "detail")),
        "results-table" => {
            for r in d["results"].as_array().into_iter().flatten() {
                parent.append(&field_row(
                    &fallback(&r["label"], "Result"),
                    &text(&r["value"]),
                    if r["tone"] == "critical" {
                        "critical"
                    } else {
                        "accent"
                    },
                    false,
                ));
                for i in r["ingredients"].as_array().into_iter().flatten() {
                    parent.append(&field_row(
                        &text(&i["label"]),
                        &text(&i["value"]),
                        "muted",
                        true,
                    ));
                }
            }
            parent.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
            parent.append(&status(d, "Running", "duration"));
        }
        "key-value" => {
            let heading = label(&text(&d["heading"]), "strong");
            parent.append(&heading);
            parent.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
            for f in d["fields"].as_array().into_iter().flatten() {
                parent.append(&field_row(
                    &text(&f["label"]),
                    &text(&f["value"]),
                    "strong",
                    false,
                ));
            }
        }
        "auth-code" => {
            let b = column(4);
            b.add_css_class("auth-panel");
            let code = label(&text(&d["code"]), "auth-code");
            code.set_xalign(0.5);
            b.append(&code);
            let caption = label("Authentication code", "caption");
            caption.set_xalign(0.5);
            caption.add_css_class("auth-caption");
            b.append(&caption);
            let copy = button("Copy code", "copy-code");
            let c = ctx.clone();
            copy.connect_clicked(move |b| {
                if c.allowed() {
                    b.clipboard()
                        .set_text(&text(&c.notice.borrow().data()["code"]));
                    b.set_label("Copied");
                }
            });
            copy.set_sensitive(ctx.allowed());
            copy.add_css_class("auth-copy");
            parent.append(&b);
            copy.set_hexpand(true);
            parent.append(&copy);
        }
        "progress" => {
            let top = row(8);
            top.append(&label(&fallback(&d["label"], "Progress"), "muted"));
            let percentage = label(&format!("{}%", number(&d["value"]).round()), "accent");
            percentage.set_xalign(1.);
            top.append(&percentage);
            parent.append(&top);
            let bar = gtk::ProgressBar::new();
            bar.set_widget_name("progress-value");
            bar.set_fraction(number(&d["value"]).clamp(0., 100.) / 100.);
            parent.append(&bar);
        }
        "timer" => {
            let r = row(10);
            let digits = label("", "countdown");
            digits.set_widget_name("countdown-value");
            digits.set_hexpand(false);
            digits.set_max_width_chars(-1);
            let caption = label("", "muted");
            r.append(&digits);
            r.append(&caption);
            parent.append(&r);
            *ctx.timer.borrow_mut() = Some((digits, caption));
            ctx.tick_timer();
        }
        "todo" => todo(ctx, parent),
        "telegram" => super::telegram::content(ctx, parent),
        _ => {}
    }
}
fn todo(ctx: &Rc<Context>, parent: &gtk::Box) {
    let n = ctx.notice.borrow().clone();
    let d = n.data();
    parent.append(&label(
        &format!(
            "{} · Due {}",
            fallback(&d["group"], "General"),
            fallback(&d["due"], "today")
        ),
        "muted",
    ));
    let controls = flow();
    let snooze = button("Snooze", "todo-snooze");
    append(&controls, &snooze);
    for (title, action) in [("Done", "done"), ("Dismiss", "dismiss")] {
        let b = button(title, &format!("todo-{action}"));
        let c = ctx.clone();
        b.connect_clicked(move |_| {
            c.emit(Intent::Task {
                action: action.into(),
                until: None,
            })
        });
        append(&controls, &b);
    }
    controls.set_sensitive(ctx.allowed());
    parent.append(&controls);
    let picker = snooze_picker(ctx);
    picker.set_visible(ctx.snoozing.get());
    let c = ctx.clone();
    let p = picker.clone();
    snooze.connect_clicked(move |_| {
        let visible = !p.is_visible();
        c.snoozing.set(visible);
        p.set_visible(visible);
        c.emit(Intent::Hold(visible));
    });
    parent.append(&picker);
}
fn snooze_picker(ctx: &Rc<Context>) -> gtk::Box {
    let out = column(8);
    let choices = flow();
    let calendar = gtk::Calendar::new();
    calendar.set_visible(false);
    for title in [
        "Afternoon",
        "Evening",
        "Night",
        "Tomorrow",
        "Weekend",
        "Next Week",
    ] {
        let b = button(
            title,
            &format!("snooze-{}", title.to_lowercase().replace(' ', "-")),
        );
        let c = ctx.clone();
        b.connect_clicked(move |_| {
            if let Some(until) = snooze_time(title, chrono::Local::now()) {
                c.emit(Intent::Task {
                    action: "snooze".into(),
                    until: Some(until),
                });
            }
        });
        append(&choices, &b);
    }
    let later = button("Later…", "snooze-later");
    let cal = calendar.clone();
    later.connect_clicked(move |_| cal.set_visible(!cal.is_visible()));
    append(&choices, &later);
    out.append(&choices);
    out.append(&calendar);
    let choose = button("Snooze until selected day at 09:00", "snooze-date");
    choose.set_visible(false);
    let ch = choose.clone();
    calendar.connect_notify_local(Some("visible"), move |cal, _| {
        ch.set_visible(cal.is_visible())
    });
    let c = ctx.clone();
    choose.connect_clicked(move |_| {
        use chrono::{TimeZone, Timelike};
        let date = calendar.date();
        if let Some(dt) = chrono::Local
            .with_ymd_and_hms(
                date.year(),
                date.month() as u32,
                date.day_of_month() as u32,
                9,
                0,
                0,
            )
            .earliest()
        {
            if dt > chrono::Local::now() {
                c.emit(Intent::Task {
                    action: "snooze".into(),
                    until: Some(dt.with_nanosecond(0).unwrap().to_rfc3339()),
                });
            }
        }
    });
    out.append(&choose);
    out.set_sensitive(ctx.allowed());
    out
}
pub fn snooze_time(title: &str, now: chrono::DateTime<chrono::Local>) -> Option<String> {
    use chrono::{Datelike, Days, TimeZone};
    let hour = match title {
        "Afternoon" => 14,
        "Evening" => 18,
        "Night" => 21,
        _ => 9,
    };
    let mut date = now.date_naive();
    let days = match title {
        "Tomorrow" => 1,
        "Weekend" => {
            let d = (5 + 7 - now.weekday().num_days_from_monday()) % 7;
            if d == 0 {
                7
            } else {
                d
            }
        }
        "Next Week" => 7 - now.weekday().num_days_from_monday(),
        _ => 0,
    };
    date = date.checked_add_days(Days::new(days as u64))?;
    let mut target = chrono::Local
        .from_local_datetime(&date.and_hms_opt(hour, 0, 0)?)
        .earliest()?;
    if target <= now {
        date = date.checked_add_days(Days::new(1))?;
        target = chrono::Local
            .from_local_datetime(&date.and_hms_opt(hour, 0, 0)?)
            .earliest()?;
    }
    Some(target.to_rfc3339())
}
