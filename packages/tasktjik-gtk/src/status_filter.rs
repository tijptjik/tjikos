//! Three-state status visibility shared by task and reading views.
use std::{cell::Cell, rc::Rc};
use tjik_gtk::gtk::{self, prelude::*};
#[derive(Clone)]
pub(crate) struct StatusFilter {
    pub button: gtk::Button,
    mode: Rc<Cell<u8>>,
    name: &'static str,
}
impl StatusFilter {
    pub fn new(name: &'static str, icon: &'static str) -> Self {
        let button = gtk::Button::new();
        button.set_child(Some(&super::icons::icon(icon)));
        button.add_css_class("icon-button");
        button.set_widget_name(&format!("filter-{icon}"));
        let this = Self {
            button,
            mode: Rc::new(Cell::new(0)),
            name,
        };
        this.set_mode(0);
        let mode = this.mode.clone();
        let name = this.name;
        this.button.connect_clicked(move |button| {
            mode.set((mode.get() + 1) % 3);
            Self::paint(button, name, mode.get());
        });
        this
    }
    pub fn mode(&self) -> u8 {
        self.mode.get()
    }
    pub fn set_mode(&self, mode: u8) {
        self.mode.set(mode.min(2));
        Self::paint(&self.button, self.name, self.mode());
    }
    pub fn accepts(&self, present: bool) -> bool {
        accepts(self.mode(), present)
    }
    fn paint(button: &gtk::Button, name: &str, mode: u8) {
        for class in ["filter-included", "filter-only"] {
            button.remove_css_class(class);
        }
        if mode == 1 {
            button.add_css_class("filter-included");
        } else if mode == 2 {
            button.add_css_class("filter-only");
        }
        let state = ["Hidden", "Included", "Only"][mode as usize];
        let next = ["include", "show only", "hide"][mode as usize];
        let label = format!("{name}: {state}. Click to {next}.");
        button.set_tooltip_text(Some(&label));
        button.update_property(&[gtk::accessible::Property::Label(&label)]);
    }
}
pub(crate) fn accepts(mode: u8, present: bool) -> bool {
    match mode {
        0 => !present,
        2 => present,
        _ => true,
    }
}

/// Only filters combine as a union, so requesting only completed and cancelled
/// shows both categories. Hidden categories remain excluded otherwise.
pub(crate) fn matches(modes: [u8; 3], flags: [bool; 3]) -> bool {
    if modes.contains(&2) {
        return modes.iter().zip(flags).any(|(m, p)| *m == 2 && p);
    }
    modes.iter().zip(flags).all(|(m, p)| accepts(*m, p))
}
#[test]
fn status_filters_cycle_visibility_and_combine_only_categories() {
    assert!(!accepts(0, true));
    assert!(accepts(0, false));
    assert!(accepts(1, true));
    assert!(accepts(1, false));
    assert!(accepts(2, true));
    assert!(!accepts(2, false));
    assert!(matches([2, 2, 0], [false, true, false]));
    assert!(matches([2, 2, 0], [true, false, false]));
    assert!(!matches([2, 2, 0], [false, false, false]));
    assert!(!matches([1, 0, 0], [true, true, false]));
}
