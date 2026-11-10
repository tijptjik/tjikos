//! GTK gestures resolve to the same configured intent vocabulary as the bar input contract.
use super::Event;
use crate::{action_choice, util::s};
use serde_json::{json, Value};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use tjik_gtk::gtk::{self, gdk, glib, prelude::*};
pub type Resolve = Rc<dyn Fn(f64, f64) -> Value>;
pub struct Binding {
    widget: gtk::Widget,
    controllers: Vec<gtk::EventController>,
    resolve: Rc<RefCell<Resolve>>,
    alive: Rc<Cell<bool>>,
    appearance: Rc<RefCell<Option<Rc<dyn Fn(bool)>>>>,
    hovered: Rc<Cell<bool>>,
}
impl Drop for Binding {
    fn drop(&mut self) {
        self.alive.set(false);
        for c in &self.controllers {
            self.widget.remove_controller(c);
        }
    }
}
impl Binding {
    pub fn appearance(&self, callback: Rc<dyn Fn(bool)>) {
        callback(self.hovered.get());
        *self.appearance.borrow_mut() = Some(callback);
    }
    pub fn update(&self, resolve: Resolve) {
        *self.resolve.borrow_mut() = resolve;
    }
    pub fn same(&self, w: &gtk::Widget) -> bool {
        self.widget == *w
    }
    pub fn new(widget: &gtk::Widget, resolve: Resolve, send: async_channel::Sender<Event>) -> Self {
        let resolve = Rc::new(RefCell::new(resolve));
        let controllers = vec![];
        let hovered = Rc::new(Cell::new(false));
        let alive = Rc::new(Cell::new(true));
        let appearance: Rc<RefCell<Option<Rc<dyn Fn(bool)>>>> = Rc::new(RefCell::new(None));
        let mut result = Self {
            widget: widget.clone(),
            controllers,
            resolve: resolve.clone(),
            alive: alive.clone(),
            appearance: appearance.clone(),
            hovered: hovered.clone(),
        };
        let epoch = Rc::new(Cell::new(0u64));
        let click = gtk::GestureClick::new();
        click.set_button(0);
        let weak_widget = widget.downgrade();
        let fire = Rc::new(move |mut payload: Value, event: &str| {
            if !alive.get() {
                return;
            }
            if let Some(widget) = weak_widget.upgrade() {
                if let Some(root) = widget.root().and_downcast::<gtk::Window>() {
                    if let Some(bounds) = widget.compute_bounds(&root) {
                        payload["context"]["localAnchor"] = json!({"x":bounds.x(),"y":bounds.y(),"width":bounds.width(),"height":bounds.height()});
                    }
                }
            }
            payload["context"]["event"] = json!(event);
            let _ = send.try_send(Event::Input(payload["context"].clone()));
        });
        let r = resolve.clone();
        let f = fire.clone();
        let e = epoch.clone();
        click.connect_released(move |gesture, n, x, y| {
            let payload = r.borrow()(x, y);
            if payload["row"]["enabled"] == false {
                return;
            }
            let event = match gesture.current_button() {
                1 => "leftClick",
                2 => "middleClick",
                3 => "rightClick",
                _ => return,
            };
            if gesture.current_button() == 1
                && !action_choice::choose(
                    &payload["module"],
                    &payload["row"],
                    s(&payload["context"]["part"]),
                    "doubleClick",
                )
                .is_null()
            {
                e.set(e.get() + 1);
                let serial = e.get();
                if n == 2 {
                    f(payload, "doubleClick");
                } else {
                    let e = e.clone();
                    let f = f.clone();
                    let delay = payload["module"]["doubleClickInterval"]
                        .as_u64()
                        .unwrap_or(250);
                    glib::timeout_add_local_once(
                        std::time::Duration::from_millis(delay),
                        move || {
                            if e.get() == serial {
                                f(payload, "leftClick");
                            }
                        },
                    );
                }
            } else {
                f(payload, event);
            }
            gesture.set_state(gtk::EventSequenceState::Claimed);
        });
        result.add(click.upcast());
        let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
        let r = resolve.clone();
        let f = fire.clone();
        let amount = Rc::new(Cell::new(0.));
        let at = Rc::new(Cell::new((0., 0.)));
        let entered_at = at.clone();
        let leave_at = at.clone();
        let a = at.clone();
        let motion = gtk::EventControllerMotion::new();
        motion.connect_motion(move |_, x, y| a.set((x, y)));
        let accum = amount.clone();
        scroll.connect_scroll(move |c, _, dy| {
            let (x, y) = at.get();
            let payload = r.borrow()(x, y);
            if payload["row"]["enabled"] == false {
                return glib::Propagation::Proceed;
            }
            let value = if c.unit() == gdk::ScrollUnit::Surface {
                dy / 40.
            } else {
                dy
            };
            let total = (accum.get() + value).clamp(-8., 8.);
            let steps = total.trunc() as i32;
            accum.set(total - steps as f64);
            for _ in 0..steps.abs() {
                f(
                    payload.clone(),
                    if steps < 0 { "scrollUp" } else { "scrollDown" },
                );
            }
            glib::Propagation::Stop
        });
        let r = resolve.clone();
        let f = fire.clone();
        let hover_epoch = Rc::new(Cell::new(0u64));
        let hover_entered = Rc::new(Cell::new(false));
        let epoch_in = hover_epoch.clone();
        let entered_in = hover_entered.clone();
        let entered_style = appearance.clone();
        let entered_hover = hovered.clone();
        motion.connect_enter(move |_, x, y| {
            entered_hover.set(true);
            // GTK is walking its crossing-event target chain here. Never replace artwork
            // until that walk has completed; coalesce to the latest hover state.
            let styles = entered_style.clone();
            let hover = entered_hover.clone();
            glib::idle_add_local_once(move || {
                if let Some(style) = styles.borrow().as_ref() {
                    style(hover.get());
                }
            });
            entered_at.set((x, y));
            epoch_in.set(epoch_in.get() + 1);
            let serial = epoch_in.get();
            let payload = r.borrow()(x, y);
            let delay = payload["module"]["hoverDelay"].as_u64().unwrap_or(250);
            let epoch = epoch_in.clone();
            let entered = entered_in.clone();
            let fire = f.clone();
            glib::timeout_add_local_once(std::time::Duration::from_millis(delay), move || {
                if epoch.get() == serial {
                    entered.set(true);
                    fire(payload, "hoverEnter");
                }
            });
        });
        let r = resolve.clone();
        let f = fire.clone();
        motion.connect_leave(move |_| {
            hovered.set(false);
            let styles = appearance.clone();
            let hover = hovered.clone();
            glib::idle_add_local_once(move || {
                if let Some(style) = styles.borrow().as_ref() {
                    style(hover.get());
                }
            });
            hover_epoch.set(hover_epoch.get() + 1);
            if hover_entered.replace(false) {
                let (x, y) = leave_at.get();
                f(r.borrow()(x, y), "hoverLeave");
            }
        });
        result.add(scroll.upcast());
        result.add(motion.upcast());
        let keys = gtk::EventControllerKey::new();
        let r = resolve;
        keys.connect_key_pressed(move |_, key, _, modifiers| {
            let event = match key {
                gdk::Key::Return | gdk::Key::space => "leftClick",
                gdk::Key::Menu => "rightClick",
                gdk::Key::F10 if modifiers.contains(gdk::ModifierType::SHIFT_MASK) => "rightClick",
                _ => return glib::Propagation::Proceed,
            };
            fire(r.borrow()(0., 0.), event);
            glib::Propagation::Stop
        });
        result.add(keys.upcast());
        widget.set_focusable(true);
        result
    }
    fn add(&mut self, c: gtk::EventController) {
        self.widget.add_controller(c.clone());
        self.controllers.push(c);
    }
}
pub fn payload(module: &Value, row: &Value, part: &str) -> Value {
    let mut c = module["inputContext"].clone();
    c["item"] = row["id"].clone();
    c["part"] = json!(part);
    c["nativeOwner"] = row["native"]["owner"].clone();
    json!({"context":c,"row":row,"module":module})
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn overrides_disable_and_part_priority() {
        let row = json!({"id":"item","actions":{"leftClick":"base","scrollDown":"player.next"},"icon":{"actions":{"leftClick":"part"}}});
        let module = json!({"actions":{"click":"module"},"items":{"item":{"icon":{"actions":{"leftClick":null}}}}});
        assert_eq!(
            action_choice::choose(&module, &row, "icon", "leftClick"),
            Value::Null
        );
        assert_eq!(
            action_choice::choose(&module, &row, "track", "click"),
            "module"
        );
        assert_eq!(
            action_choice::choose(&module, &row, "track", "scrollDown"),
            "player.next"
        );
    }
}
