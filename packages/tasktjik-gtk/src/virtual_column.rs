//! Variable-height virtual columns sharing their board's single scroll adjustment.
//! Only visible rows (plus one viewport of overscan) own widgets. Measured heights
//! are retained in a prefix tree, so scrolling and height corrections stay logarithmic.
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::Rc,
};
use tjik_gtk::gtk::{self, glib, prelude::*, subclass::prelude::*};

struct Heights {
    values: Vec<f64>,
    tree: Vec<f64>,
}
impl Heights {
    fn new(count: usize) -> Self {
        let mut h = Self {
            values: vec![0.; count],
            tree: vec![0.; count + 1],
        };
        for i in 0..count {
            h.set(i, 180.);
        }
        h
    }
    fn set(&mut self, index: usize, value: f64) -> bool {
        let delta = value - self.values[index];
        if delta.abs() < 0.5 {
            return false;
        }
        self.values[index] = value;
        let mut i = index + 1;
        while i < self.tree.len() {
            self.tree[i] += delta;
            i += i & i.wrapping_neg();
        }
        true
    }
    fn offset(&self, end: usize) -> f64 {
        let mut i = end;
        let mut sum = 0.;
        while i > 0 {
            sum += self.tree[i];
            i &= i - 1;
        }
        sum
    }
    fn total(&self) -> f64 {
        self.offset(self.values.len())
    }
    fn index(&self, position: f64) -> usize {
        let mut index = 0;
        let mut sum = 0.;
        let mut step = self.tree.len().next_power_of_two();
        while step > 0 {
            let next = index + step;
            if next < self.tree.len() && sum + self.tree[next] <= position {
                sum += self.tree[next];
                index = next;
            }
            step >>= 1;
        }
        index.min(self.values.len().saturating_sub(1))
    }
}
impl Default for Heights {
    fn default() -> Self {
        Self::new(0)
    }
}
mod imp {
    use super::*;
    #[derive(Default)]
    pub struct VirtualColumn {
        pub(super) heights: RefCell<Heights>,
        pub rows: RefCell<BTreeMap<usize, gtk::Widget>>,
        pub factory: RefCell<Option<Rc<dyn Fn(usize) -> gtk::Widget>>>,
        pub viewport: glib::WeakRef<gtk::ScrolledWindow>,
        pub outer: RefCell<Option<(gtk::Adjustment, glib::SignalHandlerId)>>,
        pub pending: Cell<bool>,
        pub width: Cell<i32>,
        pub reveal: Cell<Option<usize>>,
        pub pinned: Cell<Option<usize>>,
    }
    #[glib::object_subclass]
    impl ObjectSubclass for VirtualColumn {
        const NAME: &'static str = "TasktjikVirtualColumn";
        type Type = super::VirtualColumn;
        type ParentType = gtk::Widget;
    }
    impl ObjectImpl for VirtualColumn {
        fn dispose(&self) {
            if let Some((adjustment, signal)) = self.outer.borrow_mut().take() {
                adjustment.disconnect(signal);
            }
            for (_, row) in self.rows.take() {
                row.unparent();
            }
            self.factory.borrow_mut().take();
        }
    }
    impl WidgetImpl for VirtualColumn {
        fn request_mode(&self) -> gtk::SizeRequestMode {
            gtk::SizeRequestMode::HeightForWidth
        }
        fn measure(&self, orientation: gtk::Orientation, _: i32) -> (i32, i32, i32, i32) {
            let size = if orientation == gtk::Orientation::Horizontal {
                280
            } else {
                self.heights.borrow().total().ceil().max(1.) as i32
            };
            (size, size, -1, -1)
        }
        fn size_allocate(&self, width: i32, _: i32, _: i32) {
            let Some(viewport) = self.viewport.upgrade() else {
                return;
            };
            let top = self
                .obj()
                .compute_bounds(&viewport)
                .map_or(0., |b| b.y() as f64);
            let page = viewport.height().max(1) as f64;
            let count = self.heights.borrow().values.len();
            if count == 0 {
                return;
            }
            let start = self.heights.borrow().index((-top - page).max(0.));
            let end = self.heights.borrow().index((-top + page * 2.).max(0.)) + 1;
            let intersects = top < page * 2. && top + self.heights.borrow().total() > -page;
            let focus = self
                .obj()
                .root()
                .and_then(|r| r.downcast::<gtk::Window>().ok())
                .and_then(|w| gtk::prelude::GtkWindowExt::focus(&w));
            if self.width.replace(width) != width {
                if let Some(index) = self.rows.borrow().iter().find_map(|(index, row)| {
                    focus
                        .as_ref()
                        .is_some_and(|f| f == row || f.is_ancestor(row))
                        .then_some(*index)
                }) {
                    self.reveal.set(Some(index));
                }
            }
            self.rows.borrow_mut().retain(|i, row| {
                let keep = (intersects && *i >= start && *i < end)
                    || self.reveal.get() == Some(*i)
                    || self.pinned.get() == Some(*i)
                    || focus
                        .as_ref()
                        .is_some_and(|f| f == row || f.is_ancestor(row));
                if !keep {
                    row.unparent();
                }
                keep
            });
            let mut wanted: Vec<_> = if intersects {
                (start..end).collect()
            } else {
                Vec::new()
            };
            if let Some(index) = self.reveal.get() {
                wanted.push(index);
            }
            for index in wanted {
                if !self.rows.borrow().contains_key(&index) {
                    let factory = self.factory.borrow().as_ref().unwrap().clone();
                    let row = factory(index);
                    row.set_parent(&*self.obj());
                    self.rows.borrow_mut().insert(index, row);
                }
            }
            let mut changed = false;
            for (index, row) in self.rows.borrow().iter() {
                let height = row.measure(gtk::Orientation::Vertical, width).1.max(1);
                changed |= self.heights.borrow_mut().set(*index, height as f64 + 6.);
                let y = self.heights.borrow().offset(*index);
                row.allocate(
                    width,
                    height,
                    -1,
                    Some(
                        gtk::gsk::Transform::new()
                            .translate(&gtk::graphene::Point::new(0., y as f32)),
                    ),
                );
            }
            if !changed {
                if let Some(index) = self.reveal.get() {
                    if let Some(row) = self.rows.borrow().get(&index) {
                        if row.compute_bounds(&viewport).is_some_and(|b| {
                            b.y() >= -1. && b.y() + b.height().min(page as f32) <= page as f32 + 1.
                        }) {
                            self.reveal.set(None);
                            row.grab_focus();
                        }
                    }
                }
            }
            if changed || self.reveal.get().is_some() {
                self.schedule_layout();
            }
        }
    }
    impl VirtualColumn {
        fn schedule_layout(&self) {
            if self.pending.replace(true) {
                return;
            }
            let weak = self.obj().downgrade();
            glib::idle_add_local_once(move || {
                let Some(column) = weak.upgrade() else { return };
                let imp = column.imp();
                imp.pending.set(false);
                column.queue_resize();
                if let (Some(index), Some(viewport)) = (imp.reveal.get(), imp.viewport.upgrade()) {
                    let origin = column
                        .compute_bounds(&viewport)
                        .map_or(0., |b| b.y() as f64)
                        + viewport.vadjustment().value();
                    let offset = imp.heights.borrow().offset(index);
                    if let Some(row) = imp.rows.borrow().get(&index) {
                        row.grab_focus();
                    }
                    viewport.vadjustment().set_value(origin + offset);
                }
            });
        }
    }
}
glib::wrapper! {
    pub struct VirtualColumn(ObjectSubclass<imp::VirtualColumn>) @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}
impl VirtualColumn {
    pub fn new(
        viewport: &gtk::ScrolledWindow,
        count: usize,
        factory: impl Fn(usize) -> gtk::Widget + 'static,
    ) -> Self {
        let column: Self = glib::Object::new();
        column.set_overflow(gtk::Overflow::Hidden);
        column.imp().viewport.set(Some(viewport));
        *column.imp().heights.borrow_mut() = Heights::new(count);
        *column.imp().factory.borrow_mut() = Some(Rc::new(factory));
        let outer = viewport.vadjustment();
        let weak = column.downgrade();
        let signal = outer.connect_value_changed(move |_| {
            if let Some(column) = weak.upgrade() {
                column.queue_allocate();
            }
        });
        *column.imp().outer.borrow_mut() = Some((outer, signal));
        column
    }
    pub fn reveal(&self, index: usize) {
        if index >= self.imp().heights.borrow().values.len() {
            return;
        }
        self.imp().reveal.set(Some(index));
        self.queue_allocate();
    }
    pub fn pin(&self, index: Option<usize>) {
        self.imp().pinned.set(index);
        self.queue_allocate();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    fn settle() {
        glib::MainContext::default().block_on(glib::timeout_future(Duration::from_millis(300)));
    }
    #[test]
    fn height_index_tracks_variable_measurements() {
        let mut h = Heights::new(1000);
        h.set(0, 60.);
        h.set(3, 250.);
        assert_eq!(h.offset(4), 670.);
        for i in 0..1000 {
            assert_eq!(h.index(h.offset(i)), i);
        }
        assert_eq!(h.index(h.total() + 1.), 999);
    }
    #[test]
    #[ignore = "requires an isolated rendering display"]
    fn virtual_columns_share_scroll_and_bound_widgets() {
        gtk::init().unwrap();
        let window = gtk::Window::builder()
            .title("Tasktjik column fixture")
            .default_width(700)
            .default_height(600)
            .build();
        let viewport = gtk::ScrolledWindow::new();
        let board = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        viewport.set_child(Some(&board));
        window.set_child(Some(&viewport));
        let mut columns = Vec::new();
        for _ in 0..2 {
            let column = VirtualColumn::new(&viewport, 1000, |index| {
                let label = gtk::Label::new(Some(&format!("Task {index}")));
                let card = gtk::Box::new(gtk::Orientation::Vertical, 0);
                card.set_focusable(true);
                card.append(&label);
                card.set_size_request(
                    280,
                    if index == 500 {
                        1200
                    } else {
                        60 + (index % 4) as i32 * 30
                    },
                );
                card.upcast()
            });
            column.set_valign(gtk::Align::Start);
            column.set_hexpand(true);
            board.append(&column);
            columns.push(column);
        }
        window.present();
        settle();
        assert!(viewport.vadjustment().upper() > 60000.);
        viewport.vadjustment().set_value(40000.);
        settle();
        for column in &columns {
            let rows = column.imp().rows.borrow();
            assert!(rows.len() < 40);
            assert!(rows.keys().any(|i| *i > 200));
            assert!(rows.values().any(|r| r
                .compute_bounds(&viewport)
                .is_some_and(|b| b.y() < 600. && b.y() + b.height() > 0.)));
        }
        columns[0].reveal(999);
        settle();
        settle();
        let last = columns[0].imp().rows.borrow().get(&999).unwrap().clone();
        let bounds = last.compute_bounds(&viewport).unwrap();
        eprintln!(
            "last bounds={bounds:?} scroll={}",
            viewport.vadjustment().value()
        );
        assert!(bounds.y() >= -1. && bounds.y() + bounds.height() <= viewport.height() as f32 + 1.);
        assert!(last.has_focus());
        assert!(std::process::Command::new("timeout")
            .args(["5", "grim", "-o", "GTK", "/tmp/virtual-column-last.png"])
            .status()
            .unwrap()
            .success());
        columns[0].reveal(500);
        settle();
        settle();
        assert!(
            columns[0].imp().reveal.get().is_none(),
            "tall-row reveal must settle"
        );
        let tall = columns[0].imp().rows.borrow().get(&500).unwrap().clone();
        assert!(tall.compute_bounds(&viewport).unwrap().y().abs() <= 1.);
        columns[0].reveal(999);
        settle();
        settle();
        columns[0].pin(Some(999));
        viewport.vadjustment().set_value(0.);
        settle();
        assert!(columns[0].imp().rows.borrow().contains_key(&999));
        gtk::prelude::GtkWindowExt::set_focus(&window, None::<&gtk::Widget>);
        columns[0].pin(None);
        settle();
        assert!(!columns[0].imp().rows.borrow().contains_key(&999));
        window.close();
        settle();
    }
}
