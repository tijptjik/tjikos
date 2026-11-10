//! Toolkit-independent auto-hide hysteresis, matching the legacy edge contract.
use crate::layout::pixels;
use serde_json::Value;
use std::collections::BTreeMap;
use tjik_desktop::Output;

#[derive(Clone, Debug, Default)]
pub struct State {
    pub concealed: bool,
    pub keyboard: Option<bool>,
    pub enter: Option<u64>,
    pub leave: Option<u64>,
    pub source: String,
}
pub struct Entry<'a> {
    pub key: &'a str,
    pub definition: &'a Value,
    pub output: &'a Output,
    pub hovered: bool,
    pub fading: bool,
}
// Activity is a local visibility override, never an input to synchronized edge state.
// Keeping that state intact restores the user's reveal/hide choice when work ends.
pub fn concealed(state: Option<&State>, definition: &Value) -> bool {
    let active_tray = definition["islands"].as_array().is_some_and(|islands| {
        islands.iter().any(|island| {
            island["modules"].as_array().is_some_and(|names| {
                names.iter().filter_map(Value::as_str).any(|name| {
                    let module = &definition["moduleConfigs"][name];
                    let activity = &module["comparisonActivity"];
                    module["provider"] == crate::recording::PROVIDER
                        && module["comparisonHidden"] != true
                        && (activity["screenRecording"] == true
                            || matches!(
                                activity["hyprvoice"].as_str(),
                                Some("recording" | "processing")
                            ))
                })
            })
        })
    });
    state.is_some_and(|s| s.concealed) && !active_tray
}

fn input(e: &Entry<'_>, pointer: Option<(f64, f64)>) -> (bool, bool) {
    let Some((x, y)) = pointer else {
        return (false, e.hovered);
    };
    let r = e.output.logical;
    let (x, y) = (x - r.x, y - r.y);
    let same = x >= 0. && y >= 0. && x < r.width && y < r.height;
    let b = e.definition;
    let edge = if b["activationEdge"] == "side" {
        &b["side"]
    } else {
        &b["activationEdge"]
    };
    let (extent, distance) = match edge.as_str() {
        Some("t") => (r.height, y),
        Some("b") => (r.height, r.height - y),
        Some("l") => (r.width, x),
        _ => (r.width, r.width - x),
    };
    let reveal = pixels(&b["revealSize"], extent, e.output.scale);
    let keep = pixels(&b["keepVisibleDistance"], extent, e.output.scale).max(reveal);
    (
        same && distance <= reveal.max(1.),
        e.hovered || (same && distance <= keep),
    )
}
pub fn update(
    previous: &BTreeMap<String, State>,
    entries: &[Entry<'_>],
    pointer: Option<(f64, f64)>,
    now: u64,
) -> BTreeMap<String, State> {
    let mut groups = BTreeMap::<String, Vec<&Entry<'_>>>::new();
    for e in entries {
        let group = if e.definition["autoHide"] == true && e.definition["syncMonitors"] != false {
            "shared".into()
        } else {
            format!("local/{}", e.key)
        };
        groups.entry(group).or_default().push(e);
    }
    let mut result = BTreeMap::new();
    for group in groups.values() {
        let mut state = group
            .iter()
            .find_map(|e| previous.get(e.key))
            .cloned()
            .unwrap_or_default();
        let last = group.iter().copied().find(|e| e.key == state.source);
        let source = last
            .filter(|e| input(e, pointer).0)
            .or_else(|| group.iter().copied().find(|e| input(e, pointer).0))
            .or_else(|| last.filter(|e| input(e, pointer).1))
            .or_else(|| group.iter().copied().find(|e| input(e, pointer).1))
            .or(last)
            .unwrap_or(group[0]);
        if state.source != source.key {
            state.enter = None;
        }
        state.source = source.key.into();
        let (at_edge, held) = input(source, pointer);
        let held = held || group.iter().any(|e| e.hovered);
        if let Some(reveal) = state.keyboard {
            if !at_edge {
                state.concealed = !reveal;
                state.enter = None;
                state.leave = None;
                for e in group {
                    result.insert(e.key.into(), state.clone());
                }
                continue;
            }
            state.keyboard = None;
        }
        if source.definition["autoHide"] != true
            || pointer.is_none()
            || (state.concealed && held && group.iter().any(|e| e.fading))
        {
            state.concealed = false;
            state.enter = None;
            state.leave = None;
        } else if state.concealed {
            state.leave = None;
            if !at_edge {
                state.enter = None;
            } else {
                let since = *state.enter.get_or_insert(now);
                if now.saturating_sub(since)
                    >= source.definition["activationDelay"].as_u64().unwrap_or(0)
                {
                    state.concealed = false;
                    state.enter = None;
                }
            }
        } else {
            state.enter = None;
            if held {
                state.leave = None;
            } else {
                let since = *state.leave.get_or_insert(now);
                if now.saturating_sub(since) >= source.definition["hideDelay"].as_u64().unwrap_or(0)
                {
                    state.concealed = true;
                    state.leave = None;
                }
            }
        }
        for e in group {
            result.insert(e.key.into(), state.clone());
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tjik_desktop::DesktopSession;
    #[test]
    fn activity_reveals_only_its_indicator_and_restores_concealment() {
        let hidden = State {
            concealed: true,
            keyboard: Some(false),
            ..State::default()
        };
        for phase in ["recording", "processing", "idle", "unavailable"] {
            let active = matches!(phase, "recording" | "processing");
            let tray = json!({"islands":[{"modules":["tray"]}],"moduleConfigs":{"tray":{"provider":"recording-indicator","comparisonActivity":{"hyprvoice":phase}}}});
            assert_eq!(concealed(Some(&hidden), &tray), !active);
            let mut other = tray.clone();
            other["islands"] = json!([{"modules":["clock"]}]);
            assert!(concealed(Some(&hidden), &other));
            assert!(concealed(Some(&hidden), &json!({"moduleConfigs":{}})));
            assert!(!concealed(Some(&State::default()), &tray));
        }
        let screen = json!({"islands":[{"modules":["tray"]}],"moduleConfigs":{"tray":{"provider":"recording-indicator","comparisonActivity":{"screenRecording":true}}}});
        assert!(!concealed(Some(&hidden), &screen));
        assert_eq!(hidden.keyboard, Some(false));
        assert!(hidden.concealed);
    }

    #[test]
    fn keyboard_reveal_and_hide_release_on_real_edge_contact() {
        let snapshot = tjik_desktop::fake::Fake::default().snapshot().unwrap();
        let output = &snapshot.outputs[0];
        let bar = json!({"autoHide":true,"side":"t","activationEdge":"side","activationDelay":0,"hideDelay":0,"revealSize":3});
        let entries = [Entry {
            key: "a",
            definition: &bar,
            output,
            hovered: false,
            fading: false,
        }];
        let center = Some((output.logical.x + 100., output.logical.y + 100.));
        for reveal in [true, false] {
            let mut previous = BTreeMap::new();
            previous.insert(
                "a".into(),
                State {
                    keyboard: Some(reveal),
                    ..State::default()
                },
            );
            let held = update(&previous, &entries, center, 1000);
            assert_eq!(held["a"].concealed, !reveal);
            let released = update(
                &held,
                &entries,
                Some((output.logical.x + 100., output.logical.y + 1.)),
                1001,
            );
            assert_eq!(released["a"].keyboard, None);
            assert!(!released["a"].concealed);
        }
    }
    #[test]
    fn four_edges_percent_distance_and_fade_reversal() {
        let snapshot = tjik_desktop::fake::Fake::default().snapshot().unwrap();
        let output = &snapshot.outputs[0];
        let r = output.logical;
        for (side, x, y) in [
            ("t", r.width / 2., 1.),
            ("b", r.width / 2., r.height - 1.),
            ("l", 1., r.height / 2.),
            ("r", r.width - 1., r.height / 2.),
        ] {
            let bar = json!({"autoHide":true,"side":side,"activationEdge":"side","activationDelay":0,"hideDelay":0,"revealSize":3,"keepVisibleDistance":"10%"});
            let mut entries = [Entry {
                key: "a",
                definition: &bar,
                output,
                hovered: false,
                fading: false,
            }];
            let center = Some((r.x + r.width / 2., r.y + r.height / 2.));
            let mut state = update(&BTreeMap::new(), &entries, center, 0);
            assert!(state["a"].concealed);
            state = update(&state, &entries, Some((r.x + x, r.y + y)), 1);
            assert!(!state["a"].concealed);
            state = update(&state, &entries, center, 2);
            assert!(state["a"].concealed);
            entries[0].hovered = true;
            entries[0].fading = true;
            state = update(&state, &entries, center, 3);
            assert!(!state["a"].concealed);
        }
    }
    #[test]
    fn dwell_cancels_and_pointer_loss_reveals() {
        let snapshot = tjik_desktop::fake::Fake::default().snapshot().unwrap();
        let output = &snapshot.outputs[0];
        let bar = json!({"autoHide":true,"side":"t","activationEdge":"side","activationDelay":500,"hideDelay":200,"revealSize":3,"keepVisibleDistance":"64px"});
        let entries = [Entry {
            key: "a",
            definition: &bar,
            output,
            hovered: false,
            fading: false,
        }];
        let point = |y| Some((output.logical.x + 10., output.logical.y + y));
        let mut s = update(&BTreeMap::new(), &entries, point(100.), 0);
        s = update(&s, &entries, point(100.), 200);
        assert!(s["a"].concealed);
        s = update(&s, &entries, point(1.), 300);
        s = update(&s, &entries, point(100.), 799);
        s = update(&s, &entries, point(1.), 800);
        s = update(&s, &entries, point(1.), 1299);
        assert!(s["a"].concealed);
        s = update(&s, &entries, point(1.), 1300);
        assert!(!s["a"].concealed);
        s = update(&s, &entries, point(100.), 1400);
        s = update(&s, &entries, point(100.), 1600);
        assert!(s["a"].concealed);
        s = update(&s, &entries, None, 1601);
        assert!(!s["a"].concealed);
    }
    #[test]
    fn shared_source_scale_independence_and_hotplug() {
        let snapshot = tjik_desktop::fake::Fake::default().snapshot().unwrap();
        let a = &snapshot.outputs[0];
        let mut b = a.clone();
        b.logical.x += a.logical.width;
        b.scale = 2.;
        let bar = json!({"autoHide":true,"side":"t","activationEdge":"side","activationDelay":100,"hideDelay":0,"revealSize":3,"keepVisibleDistance":"64px"});
        let mut local = bar.clone();
        local["syncMonitors"] = json!(false);
        let make = |independent| {
            vec![
                Entry {
                    key: "a",
                    definition: &bar,
                    output: a,
                    hovered: false,
                    fading: false,
                },
                Entry {
                    key: "b",
                    definition: if independent { &local } else { &bar },
                    output: &b,
                    hovered: false,
                    fading: false,
                },
            ]
        };
        let entries = make(false);
        let center = Some((a.logical.x + 10., a.logical.y + 200.));
        let edge = Some((b.logical.x + 10., b.logical.y + 1.));
        let mut s = update(&BTreeMap::new(), &entries, center, 0);
        assert!(s.values().all(|s| s.concealed));
        s = update(&s, &entries, edge, 1);
        s = update(&s, &entries, edge, 101);
        assert!(s.values().all(|s| !s.concealed));
        s = update(
            &s,
            &entries,
            Some((b.logical.x + 10., b.logical.y + 32.)),
            102,
        );
        assert!(!s["a"].concealed);
        s = update(
            &s,
            &entries,
            Some((b.logical.x + 10., b.logical.y + 33.)),
            103,
        );
        assert!(s["a"].concealed);
        s = update(&s, &make(true), edge, 200);
        s = update(&s, &make(true), edge, 300);
        assert!(s["a"].concealed);
        assert!(!s["b"].concealed);
        assert_eq!(update(&s, &entries[..1], center, 301).len(), 1);
    }
}
