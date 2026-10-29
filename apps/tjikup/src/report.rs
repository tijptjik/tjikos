//! Status protocol shared by existing hooks and the native renderer.
#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    // Resolved by Progress before queuing; empty events fall back to their subject.
    pub icon_key: String,
    pub verb: String,
    pub icon: String,
    pub subject: String,
    pub note: String,
    pub section: String,
}
pub fn plain(text: &str) -> String {
    let mut result = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            match chars.next() {
                Some('[') => {
                    for c in chars.by_ref() {
                        if ('@'..='~').contains(&c) {
                            break;
                        }
                    }
                }
                Some(']' | '_' | 'P') => {
                    while let Some(c) = chars.next() {
                        if c == '\x07' || c == '\x1b' && chars.peek() == Some(&'\\') {
                            if c == '\x1b' {
                                chars.next();
                            }
                            break;
                        }
                    }
                }
                _ => (),
            }
        } else if !c.is_control() {
            result.push(c);
        }
    }
    result
}
impl Event {
    pub fn visible(&self) -> bool {
        self.issue()
            || (!matches!(self.verb.as_str(), "SKIP" | "CHECK" | "DONE" | "COMPLETE")
                && self.icon == "✓"
                && !self
                    .note
                    .split(';')
                    .any(|n| matches!(n.trim(), "no changes" | "no updates")))
    }
    pub fn issue(&self) -> bool {
        matches!(self.icon.as_str(), "!" | "✗" | "X")
    }
    pub fn line(text: &str) -> Option<Self> {
        let text = plain(text);
        let mut fields = text.trim().splitn(2, char::is_whitespace);
        let verb = fields.next()?;
        if verb.is_empty() || !verb.bytes().all(|b| b.is_ascii_uppercase()) {
            return None;
        }
        let mut rest = fields.next()?.trim_start().splitn(2, char::is_whitespace);
        let icon = rest.next()?;
        if !["✓", "!", "✗", "X", "-", "..."].contains(&icon) {
            return None;
        }
        let content = rest.next().unwrap_or("").trim();
        let (subject, note) = content.split_once("  ").unwrap_or((content, ""));
        Some(Self {
            icon_key: String::new(),
            verb: verb.into(),
            icon: icon.into(),
            subject: subject.trim().into(),
            note: note.trim().into(),
            section: String::new(),
        })
    }
}
pub fn parse(report: &str, log: &str, show_skips: bool) -> Vec<Event> {
    let mut events = Vec::new();
    let mut log_events = Vec::new();
    let mut log_section = String::new();
    for line in log.lines() {
        if let Some(title) = plain(line).strip_prefix("__tjikup_section__:") {
            log_section = title.into();
        } else if let Some(mut event) = Event::line(line) {
            event.section = log_section.clone();
            log_events.push(event);
        }
    }
    let mut section = String::new();
    for line in report.lines() {
        let fields: Vec<_> = line.splitn(4, '\t').collect();
        if fields.len() != 4 {
            continue;
        }
        if let Some(title) = fields[2].strip_prefix("__tjikup_section__:") {
            section = plain(title);
            continue;
        }
        let event = Event {
            icon_key: String::new(),
            verb: plain(fields[0]),
            icon: plain(fields[1]),
            subject: plain(fields[2]),
            note: plain(fields[3]),
            section: if section.is_empty() {
                log_events
                    .iter()
                    .find(|e| e.subject == plain(fields[2]))
                    .map(|e| e.section.clone())
                    .unwrap_or_default()
            } else {
                section.clone()
            },
        };
        events.push(event);
    }
    // Some standalone capture helpers only emit status lines. Prefer their TSV
    // report when present; parsing both would duplicate per-service results.
    if events.is_empty() {
        events.extend(log_events);
    }
    let mut selected: Vec<Event> = Vec::new();
    for mut event in events {
        if !event.visible() && !(show_skips && matches!(event.icon.as_str(), "✓" | "-")) {
            continue;
        }
        if !event.visible()
            && (event.verb == "SKIP"
                || event
                    .note
                    .split(';')
                    .any(|n| matches!(n.trim(), "no changes" | "no updates")))
        {
            event.verb = "SKIP".into();
        }
        if let Some(old) = selected.iter_mut().find(|old| {
            old.subject == event.subject
                && old.section == event.section
                && old.issue() == event.issue()
        }) {
            // A later no-op must never hide an earlier mutation of this subject.
            if old.visible() && !event.visible() {
                continue;
            }
            if event.visible() && !old.visible() {
                *old = event;
                continue;
            }
            if old.verb != event.verb && !event.issue() && event.visible() {
                old.verb = "APPLY".into();
            }
            if !event.visible() {
                old.verb = event.verb;
                old.icon = event.icon;
            }
            old.note = event.note;
        } else {
            selected.push(event);
        }
    }
    selected
}

/// Inspect outcomes before presentation filters hide SKIP lines.
pub fn unchanged(report: &str, log: &str) -> bool {
    let events = parse(report, log, true);
    let mut skipped = false;
    for event in events {
        if event.issue() || !matches!(event.verb.as_str(), "SKIP" | "CHECK") {
            return false;
        }
        skipped |= event.verb == "SKIP";
    }
    for line in log.lines().map(plain) {
        if let Some(verb) = line.strip_prefix("__stage_verb__:") {
            if !matches!(verb, "SKIP" | "CHECK") {
                return false;
            }
            skipped |= verb == "SKIP";
        }
    }
    skipped
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn changes_survive_later_skips_and_issues_survive_checks() {
        let events = parse(
            "UPDATE\t✓\tBartjik\tupdated\nSKIP\t✓\tBartjik\tno changes\nCHECK\t!\tDisk\tfull\n",
            "",
            false,
        );
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].verb, "UPDATE");
        assert!(events[1].issue());
        assert!(parse("APPLY\t✓\tFonts\tchecked; no changes\n", "", false).is_empty());
    }
    #[test]
    fn verbose_reports_retain_skips_but_changes_win_in_either_order() {
        let events = parse("SKIP\t✓\tApp\tno changes\nUPDATE\t✓\tApp\tupdated\nSKIP\t✓\tApp\tno changes\nAPPLY\t✓\tFonts\tchecked; no changes\nCHECK\t!\tDisk\tfull\nCHECK\t...\tRunning\twait\n", "", true);
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].verb, "UPDATE");
        assert_eq!(events[0].note, "updated");
        assert_eq!(events[1].verb, "SKIP");
        assert!(events[2].issue());
        let fallback = parse("", "SKIP    - Optional task    not applicable", true);
        assert_eq!(fallback.len(), 1);
        assert_eq!(fallback[0].note, "not applicable");
    }
    #[test]
    fn ansi_and_stdout_protocol() {
        assert_eq!(
            Event::line("\x1b[32mSYNC    ✓ Assets (fonts)    refreshed\x1b[0m")
                .unwrap()
                .note,
            "refreshed"
        );
        assert!(Event::line("Overwrite this file? ").is_none());
        assert_eq!(plain("a\x1b]0;title\x07b\x1b[31mc"), "abc");
    }
}
