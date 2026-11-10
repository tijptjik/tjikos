//! Resolve recycled rows through their live GTK list item, never through model IDs.
use gtk::{glib, prelude::*, subclass::prelude::*};

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct ResultRow {
        pub item: glib::WeakRef<gtk::ListItem>,
        pub pulse: std::cell::RefCell<Option<crate::motion::ClosingPulse>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ResultRow {
        const NAME: &'static str = "TjikResultRow";
        type Type = super::ResultRow;
        type ParentType = gtk::Box;
    }
    impl ObjectImpl for ResultRow {
        fn dispose(&self) {
            self.pulse.borrow_mut().take();
        }
    }
    impl WidgetImpl for ResultRow {}
    impl BoxImpl for ResultRow {}
}

glib::wrapper! {
    pub struct ResultRow(ObjectSubclass<imp::ResultRow>)
        @extends gtk::Widget, gtk::Box,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Orientable;
}

impl ResultRow {
    pub fn new(item: &gtk::ListItem, orientation: gtk::Orientation, spacing: i32) -> Self {
        let row: Self = glib::Object::builder()
            .property("orientation", orientation)
            .property("spacing", spacing)
            .build();
        row.imp().item.set(Some(item));
        *row.imp().pulse.borrow_mut() = Some(crate::motion::ClosingPulse::new(&row));
        row
    }

    pub fn set_closing(&self, pending: bool) {
        if let Some(pulse) = self.imp().pulse.borrow().as_ref() {
            pulse.set_pending(pending);
        }
    }

    pub fn position(&self) -> Option<u32> {
        let item = self.imp().item.upgrade()?;
        let position = item.position();
        (item.item().is_some() && position != gtk::INVALID_LIST_POSITION).then_some(position)
    }
}
