use gtk::gdk;

#[derive(Default)]
pub(super) struct CancelKeyTap {
    pending: Option<gdk::Key>,
    used_as_modifier: bool,
}

impl CancelKeyTap {
    pub(super) fn press(&mut self, key: gdk::Key) {
        if let Some(pending) = self.pending {
            if pending != key {
                self.used_as_modifier = true;
            }
        } else if matches!(key, gdk::Key::Control_R | gdk::Key::Alt_R) {
            self.pending = Some(key);
            self.used_as_modifier = false;
        }
    }

    pub(super) fn release(&mut self, key: gdk::Key) -> bool {
        if self.pending == Some(key) {
            let cancel = !self.used_as_modifier;
            *self = Self::default();
            cancel
        } else {
            if self.pending.is_some() {
                self.used_as_modifier = true;
            }
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::CancelKeyTap;
    use gtk::gdk;

    #[test]
    fn right_control_and_right_alt_taps_cancel() {
        for key in [gdk::Key::Control_R, gdk::Key::Alt_R] {
            let mut tap = CancelKeyTap::default();
            tap.press(key);
            assert!(tap.release(key));
        }
    }

    #[test]
    fn left_side_modifiers_do_not_cancel() {
        for key in [gdk::Key::Control_L, gdk::Key::Alt_L] {
            let mut tap = CancelKeyTap::default();
            tap.press(key);
            assert!(!tap.release(key));
        }
    }

    #[test]
    fn modifier_chords_do_not_cancel() {
        for cancel_key in [gdk::Key::Control_R, gdk::Key::Alt_R] {
            let mut tap = CancelKeyTap::default();
            tap.press(cancel_key);
            tap.press(gdk::Key::A);
            assert!(!tap.release(gdk::Key::A));
            assert!(!tap.release(cancel_key));
        }
    }
}
