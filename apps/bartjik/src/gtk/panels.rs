//! Toolkit-independent attached-panel geometry and hover lifecycle.
use serde_json::Value;
use tjik_desktop::Rect;
#[derive(Clone, Debug)]
pub struct Target {
    pub key: String,
    pub bar: String,
    pub module: String,
    pub item: String,
    pub output: String,
    pub side: String,
    pub bounds: Rect,
    pub anchor: Option<(f64, f64)>,
    pub source: Rect,
    pub bar_bounds: Rect,
    pub screen: Rect,
    pub options: Value,
    pub row: Value,
    pub provider: String,
    pub style: Value,
    pub enabled: bool,
}
pub fn contains(r: Rect, p: (f64, f64)) -> bool {
    r.width > 0.
        && r.height > 0.
        && p.0 >= r.x
        && p.0 <= r.x + r.width
        && p.1 >= r.y
        && p.1 <= r.y + r.height
}
pub fn nearest(targets: &[Target], point: (f64, f64)) -> Option<Target> {
    targets
        .iter()
        .filter(|t| contains(t.source, point))
        .min_by(|a, b| {
            let distance = |r: Rect| {
                let x = (r.x - point.0).max(0.).max(point.0 - r.x - r.width);
                let y = (r.y - point.1).max(0.).max(point.1 - r.y - r.height);
                x * x + y * y
            };
            distance(a.bounds).total_cmp(&distance(b.bounds))
        })
        .filter(|t| t.enabled)
        .cloned()
        .map(|mut target| {
            target.anchor = Some(point);
            target
        })
}
pub fn place(target: &Target, width: f64, height: f64) -> Rect {
    let screen = target.screen;
    let width = width.min((screen.width - 24.).max(1.));
    let height = height.min((screen.height - 24.).max(1.));
    let anchor = target.anchor.unwrap_or((
        target.bounds.x + target.bounds.width / 2.,
        target.bounds.y + target.bounds.height / 2.,
    ));
    let mut x = anchor.0 - width / 2.;
    let mut y = anchor.1 - height / 2.;
    match target.side.as_str() {
        "t" => y = target.bar_bounds.y + target.bar_bounds.height + 8.,
        "b" => y = target.bar_bounds.y - height - 8.,
        "l" => x = target.bar_bounds.x + target.bar_bounds.width + 8.,
        _ => x = target.bar_bounds.x - width - 8.,
    }
    Rect {
        x: x.clamp(
            screen.x + 12.,
            (screen.x + screen.width - width - 12.).max(screen.x + 12.),
        ),
        y: y.clamp(
            screen.y + 12.,
            (screen.y + screen.height - height - 12.).max(screen.y + 12.),
        ),
        width,
        height,
    }
}
pub fn held(target: &Target, panel: Rect, point: (f64, f64)) -> bool {
    let source = target.source;
    if contains(source, point) || contains(panel, point) {
        return true;
    }
    let horizontal = target.side == "t" || target.side == "b";
    let corridor = if horizontal {
        let y = if target.side == "b" {
            panel.y + panel.height
        } else {
            source.y + source.height
        };
        let bottom = if target.side == "b" {
            source.y
        } else {
            panel.y
        };
        Rect {
            x: source.x.min(panel.x),
            y,
            width: (source.x + source.width).max(panel.x + panel.width) - source.x.min(panel.x),
            height: bottom - y,
        }
    } else {
        let x = if target.side == "r" {
            panel.x + panel.width
        } else {
            source.x + source.width
        };
        let right = if target.side == "r" {
            source.x
        } else {
            panel.x
        };
        Rect {
            x,
            y: source.y.min(panel.y),
            width: right - x,
            height: (source.y + source.height).max(panel.y + panel.height) - source.y.min(panel.y),
        }
    };
    contains(corridor, point)
}
#[derive(Default)]
pub struct Deck {
    pub active: Option<Target>,
    pub pinned: bool,
    pending: Option<(Target, u64)>,
    outside: Option<u64>,
    suppressed: Option<String>,
}
impl Deck {
    pub fn busy(&self) -> bool {
        self.active.is_some() || self.pending.is_some()
    }
    pub fn pending_key(&self) -> Option<&str> {
        self.pending.as_ref().map(|(t, _)| t.key.as_str())
    }
    pub fn close(&mut self) {
        self.suppressed = self.active.as_ref().map(|t| t.key.clone());
        self.active = None;
        self.pending = None;
        self.pinned = false;
        self.outside = None;
    }
    pub fn open(&mut self, target: Target) {
        self.active = Some(target);
        self.pending = None;
        self.outside = None;
        self.pinned = true;
        self.suppressed = None;
    }
    pub fn refresh(&mut self, targets: &[Target], detached: bool) {
        if let Some(active) = self.active.as_ref() {
            if let Some(current) = targets.iter().find(|t| t.key == active.key) {
                let mut current = current.clone();
                current.anchor = active.anchor;
                self.active = Some(current);
            } else if !detached {
                self.close();
            }
        }
    }
    pub fn step(
        &mut self,
        target: Option<Target>,
        point: Option<(f64, f64)>,
        panel: Option<Rect>,
        inside: bool,
        now: u64,
    ) {
        if target.as_ref().map(|t| t.key.as_str()) != self.suppressed.as_deref() {
            self.suppressed = None;
        }
        if self.pinned {
            return;
        }
        if let Some(target) = target.filter(|t| self.suppressed.as_deref() != Some(&t.key)) {
            self.outside = None;
            if self
                .active
                .as_ref()
                .is_some_and(|a| a.bar == target.bar && a.output == target.output)
            {
                self.active = Some(target);
                self.pending = None;
                return;
            }
            // Crossing outputs/bars gets a fresh dwell; never carry a source corridor across them.
            self.active = None;
            if self
                .pending
                .as_ref()
                .is_none_or(|(p, _)| p.key != target.key)
            {
                self.pending = Some((target.clone(), now));
            }
            let delay = target.options["delay"].as_u64().unwrap_or(300);
            if self
                .pending
                .as_ref()
                .is_some_and(|(_, at)| now.saturating_sub(*at) >= delay)
            {
                self.active = Some(target);
                self.pending = None;
            }
            return;
        }
        self.pending = None;
        if let Some(active) = &self.active {
            if inside || point.zip(panel).is_some_and(|(p, r)| held(active, r, p)) {
                self.outside = None;
                return;
            }
            // Missing/stale global coordinates are not evidence that the pointer left.
            if point.is_none() {
                return;
            }
            let close = active.options["closeDelay"]
                .as_u64()
                .unwrap_or(160)
                .max(250);
            if now.saturating_sub(*self.outside.get_or_insert(now)) >= close {
                self.active = None;
                self.outside = None;
            }
        }
    }
}
#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use serde_json::json;
    pub(crate) fn target(key: &str) -> Target {
        Target {
            key: key.into(),
            bar: "bar".into(),
            module: key.into(),
            item: "row".into(),
            output: "display".into(),
            side: "b".into(),
            anchor: None,
            bounds: Rect {
                x: 100.,
                y: 700.,
                width: 20.,
                height: 40.,
            },
            source: Rect {
                x: 90.,
                y: 700.,
                width: 160.,
                height: 40.,
            },
            bar_bounds: Rect {
                x: 0.,
                y: 700.,
                width: 900.,
                height: 40.,
            },
            screen: Rect {
                x: 0.,
                y: 0.,
                width: 1000.,
                height: 800.,
            },
            options: json!({"delay":300,"closeDelay":160}),
            row: json!({}),
            provider: "datetime".into(),
            style: json!({}),
            enabled: true,
        }
    }
    #[test]
    fn detached_panel_survives_workspace_bar_removal() {
        let original = target("persistent");
        let mut deck = Deck::default();
        deck.open(original.clone());
        deck.refresh(&[], true);
        deck.step(None, None, None, false, 10_000);
        assert_eq!(deck.active.as_ref().unwrap().key, original.key);
        assert!(deck.pinned);
        deck.refresh(&[original.clone()], true);
        assert_eq!(deck.active.as_ref().unwrap().anchor, original.anchor);
        deck.close();
        assert!(deck.active.is_none());
        deck.open(original);
        deck.refresh(&[], false);
        assert!(
            deck.active.is_none(),
            "attached panels still close with their bar"
        );
    }
    #[test]
    fn island_padding_and_midpoint_do_not_expand_to_desktop() {
        let a = target("a");
        let mut b = target("b");
        b.bounds.x = 200.;
        assert_eq!(
            nearest(&[a.clone(), b.clone()], (94., 720.)).unwrap().key,
            "a"
        );
        assert_eq!(
            nearest(&[a.clone(), b.clone()], (180., 720.)).unwrap().key,
            "b"
        );
        b.enabled = false;
        assert!(nearest(&[a.clone(), b], (220., 720.)).is_none());
        assert!(nearest(&[a], (80., 720.)).is_none());
    }
    #[test]
    fn dwell_gap_crossing_switch_and_exit() {
        let mut d = Deck::default();
        let a = target("a");
        let r = place(&a, 420., 300.);
        d.step(Some(a.clone()), Some((110., 720.)), None, false, 0);
        assert!(d.active.is_none());
        d.step(Some(a.clone()), Some((110., 720.)), None, false, 299);
        assert!(d.active.is_none());
        d.step(Some(a.clone()), Some((110., 720.)), None, false, 300);
        assert!(d.active.is_some());
        d.step(None, Some((110., 696.)), Some(r), false, 600);
        assert!(d.active.is_some());
        d.step(None, Some((600., 720.)), Some(r), false, 700);
        let b = target("b");
        d.step(Some(b), Some((200., 720.)), Some(r), false, 900);
        assert_eq!(d.active.as_ref().unwrap().key, "b");
        d.step(None, Some((800., 100.)), Some(r), false, 1000);
        d.step(None, Some((800., 100.)), Some(r), false, 1249);
        assert!(d.active.is_some());
        d.step(None, Some((800., 100.)), Some(r), false, 1250);
        assert!(d.active.is_none());
    }
    #[test]
    fn four_edges_clamp_offset_outputs_and_corridors() {
        for side in ["t", "b", "l", "r"] {
            let mut t = target("a");
            t.side = side.into();
            t.screen.x = -1000.;
            t.source.x = -600.;
            t.bounds.x = -590.;
            t.bar_bounds = Rect {
                x: -600.,
                y: 350.,
                width: 40.,
                height: 40.,
            };
            t.source = t.bar_bounds;
            t.bounds = t.bar_bounds;
            let r = place(&t, 240., 200.);
            assert!(r.x >= -988. && r.x + r.width <= -12.);
            let p = match side {
                "t" => (-580., 394.),
                "b" => (-580., 346.),
                "l" => (-556., 370.),
                _ => (-604., 370.),
            };
            assert!(held(&t, r, p), "{side}");
            assert!(!held(&t, r, (-990., 10.)));
        }
    }
    #[test]
    fn cancellation_pin_stale_and_suppression() {
        let a = target("a");
        let mut d = Deck::default();
        d.step(Some(a.clone()), None, None, false, 0);
        d.step(None, Some((0., 0.)), None, false, 500);
        assert!(!d.busy());
        d.open(a.clone());
        d.step(None, Some((0., 0.)), None, false, 1000);
        assert!(d.active.is_some());
        d.close();
        d.step(Some(a.clone()), None, None, false, 2000);
        assert!(!d.busy());
        d.step(None, None, None, false, 2100);
        d.step(Some(a.clone()), None, None, false, 2200);
        d.step(Some(a), None, None, false, 2500);
        assert!(d.active.is_some());
        d.step(None, None, None, false, 9999);
        assert!(d.active.is_some());
    }
}
