//! Swallow the remainder of a command the user was already typing when it opened.
use std::time::{Duration, Instant};

#[derive(Default)]
pub struct Completion {
    remaining: String,
    last_input: Option<Instant>,
}
impl Completion {
    pub fn clear(&mut self) {
        self.remaining.clear();
    }
    pub fn arm(&mut self, typed: &str, command: &str) {
        self.clear();
        let typed = typed.trim_start_matches('/').to_ascii_lowercase();
        let command = command.trim_start_matches('/');
        if !typed.is_empty() && command.starts_with(&typed) {
            self.remaining = format!("{} ", &command[typed.len()..]);
        }
    }
    pub fn edit(&mut self, previous: &str, next: &str, now: Instant) -> Option<String> {
        let fresh = self
            .last_input
            .is_some_and(|last| now.duration_since(last) < Duration::from_millis(500));
        self.last_input = Some(now);
        if !fresh {
            self.clear();
        }
        let Some(added) = next.strip_prefix(previous) else {
            self.clear();
            return None;
        };
        let mut consumed = 0;
        for c in added.chars() {
            // The canonical query already includes its argument separator.
            if c == ' ' && previous.ends_with(' ') && !self.remaining.is_empty() {
                self.clear();
                consumed += 1;
                continue;
            }
            if !c.is_ascii_alphabetic() || !self.remaining.starts_with(c.to_ascii_lowercase()) {
                self.clear();
                break;
            }
            self.remaining.drain(..c.len_utf8());
            consumed += c.len_utf8();
        }
        (consumed > 0).then(|| format!("{previous}{}", &added[consumed..]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fast_tail_and_idle_boundary() {
        let mut c = Completion::default();
        let t = Instant::now();
        c.edit("/", "/d", t);
        c.arm("/d", "/define");
        assert_eq!(
            c.edit("/define ", "/define e", t + Duration::from_millis(100))
                .as_deref(),
            Some("/define ")
        );
        assert_eq!(
            c.edit("/define ", "/define f", t + Duration::from_millis(499))
                .as_deref(),
            Some("/define ")
        );
        assert_eq!(
            c.edit("/define ", "/define i", t + Duration::from_millis(999)),
            None
        );
    }
    #[test]
    fn mismatch_and_edit_release_the_guard() {
        let t = Instant::now();
        let mut c = Completion::default();
        c.edit("/", "/d", t);
        c.arm("/d", "/define");
        assert_eq!(
            c.edit("/define ", "/define efmonster", t).as_deref(),
            Some("/define monster")
        );
        assert_eq!(c.edit("/define monster", "/define monstere", t), None);
        c.arm("/d", "/define");
        assert_eq!(c.edit("/define ", "/define", t), None);
        assert_eq!(c.edit("/define", "/definee", t), None);
    }
    #[test]
    fn redundant_argument_separator_is_consumed_once() {
        let t = Instant::now();
        let mut c = Completion::default();
        c.edit("/", "/d", t);
        c.arm("/d", "/define");
        assert_eq!(
            c.edit("/define ", "/define efine monster", t).as_deref(),
            Some("/define monster")
        );
        assert_eq!(c.edit("/define monster", "/define monster ", t), None);
    }
    #[test]
    fn unrelated_aliases_and_slow_responses_do_not_swallow_text() {
        let t = Instant::now();
        let mut c = Completion::default();
        c.edit("", "words", t);
        c.arm("words", "/define");
        assert_eq!(c.edit("/define ", "/define e", t), None);
        c.arm("/D", "/define");
        assert_eq!(
            c.edit("/define ", "/define E", t + Duration::from_millis(500)),
            None
        );
    }
}
