//! Logical geometry matching the legacy renderer's stable-column contract.
use crate::util::{arr, num, s, st};
use serde_json::Value;
use tjik_desktop::{Output, Rect, Snapshot, WorkspaceTarget};
pub fn horizontal(bar: &Value) -> bool {
    ["t", "b"].contains(&s(&bar["side"]))
}
pub fn pixels(value: &Value, extent: f64, scale: f64) -> f64 {
    if let Some(value) = value.as_str() {
        if let Some(percent) = value.strip_suffix('%') {
            return percent.parse::<f64>().unwrap_or(0.) * extent / 100.;
        }
        return value.trim_end_matches("px").parse::<f64>().unwrap_or(0.) / scale;
    }
    num(value) / scale
}
pub fn geometry(bar: &Value, sw: f64, sh: f64, cw: f64, ch: f64, scale: f64) -> Rect {
    let horizontal = horizontal(bar);
    let span = if horizontal { sw } else { sh };
    let centre = if bar["layout"]["centre_width"].is_null() {
        span / 3.
    } else {
        pixels(&bar["layout"]["centre_width"], span, scale)
    };
    let left = (span - centre) / 2.;
    let (column_start, column_size) = match s(&bar["position"]) {
        "l" => (0., left),
        "r" => (left + centre, left),
        _ => (left, centre),
    };
    let padding = num(&bar["padding"]);
    let mut available = (column_size - 2. * padding).max(0.);
    let mut origin = column_start + padding;
    let content = if horizontal { cw } else { ch };
    let gutter = if bar["screenGutter"] == "edgeOffset" {
        pixels(
            &bar[if horizontal { "offsetY" } else { "offsetX" }],
            if horizontal { sh } else { sw },
            scale,
        )
        .abs()
    } else {
        pixels(&bar["screenGutter"], span, scale)
    }
    .min(span / 2.);
    let shift = pixels(
        &bar[if horizontal { "offsetX" } else { "offsetY" }],
        span,
        scale,
    );
    let column_available =
        ((span - gutter).min(origin + available + shift) - gutter.max(origin + shift)).max(0.);
    let expanded =
        bar["overflow"] == "centre" && bar["position"] != "c" && content > column_available;
    if expanded {
        available = (span / 2. - 2. * padding).max(0.);
        if bar["position"] == "r" {
            origin = span / 2. + padding;
        }
    }
    let low = gutter.max((span - gutter).min(origin + shift));
    let high = low.max((span - gutter).min(origin + available + shift));
    available = high - low;
    let extent = content.min(available);
    let alignment = if expanded {
        if bar["position"] == "r" {
            "r"
        } else {
            "l"
        }
    } else {
        s(&bar["alignment"])
    };
    let start = low
        + match alignment {
            "l" => 0.,
            "r" => available - extent,
            _ => (available - extent) / 2.,
        };
    let depth = num(&bar["stackOffset"]);
    let mut x = if horizontal {
        start
    } else if bar["side"] == "l" {
        depth
    } else {
        sw - cw - depth
    };
    let mut y = if horizontal {
        if bar["side"] == "t" {
            depth
        } else {
            sh - ch - depth
        }
    } else {
        start
    };
    if horizontal {
        y += pixels(&bar["offsetY"], sh, scale);
    } else {
        x += pixels(&bar["offsetX"], sw, scale);
    }
    Rect {
        x,
        y,
        width: if horizontal { extent } else { cw },
        height: if horizontal { ch } else { extent },
    }
}
pub fn reserve(bar: &Value, rect: Rect, width: f64, height: f64) -> i32 {
    if bar["reserve"] != true || bar["visible"] == false {
        return 0;
    }
    let depth = match s(&bar["side"]) {
        "t" => rect.y + rect.height,
        "b" => height - rect.y,
        "l" => rect.x + rect.width,
        _ => width - rect.x,
    };
    depth.max(0.).ceil() as i32
}
pub struct WorkspaceRow {
    pub label: String,
    pub target: WorkspaceTarget,
    pub active: bool,
}
pub fn workspaces(
    snapshot: &Snapshot,
    output: &Output,
    connector: &str,
    options: &Value,
) -> Vec<WorkspaceRow> {
    let mut rows = vec![];
    for workspace in snapshot
        .workspaces
        .iter()
        .filter(|w| w.output.as_ref() == Some(&output.id))
    {
        if workspace.special && options["special"] == false {
            continue;
        }
        let active = output
            .workspace
            .as_ref()
            .is_some_and(|w| w.id == workspace.workspace.id);
        let occupied = snapshot.windows.iter().any(|w| {
            w.workspace
                .as_ref()
                .is_some_and(|w| w.id == workspace.workspace.id)
        });
        if options["occupiedOnly"] == true && !active && !occupied {
            continue;
        }
        let Some(name) = workspace.workspace.name.as_ref() else {
            continue;
        };
        rows.push(WorkspaceRow {
            label: options["labels"][name]
                .as_str()
                .unwrap_or(name.trim_start_matches("special:"))
                .into(),
            target: WorkspaceTarget {
                generation: snapshot.generation.clone(),
                output: output.id.clone(),
                id: Some(workspace.workspace.id.clone()),
                name: name.clone(),
            },
            active,
        });
    }
    if options["occupiedOnly"] != true {
        for value in arr(&options["persistent"][connector]) {
            let name = st(value);
            if snapshot
                .workspaces
                .iter()
                .any(|w| w.workspace.name.as_ref() == Some(&name))
            {
                continue;
            }
            rows.push(WorkspaceRow {
                label: options["labels"][&name].as_str().unwrap_or(&name).into(),
                target: WorkspaceTarget {
                    generation: snapshot.generation.clone(),
                    output: output.id.clone(),
                    id: None,
                    name,
                },
                active: false,
            });
        }
    }
    rows.sort_by_key(|r| {
        let name = &r.target.name;
        if let Ok(n) = name.parse::<i64>() {
            (0, n, String::new())
        } else {
            (
                if name.starts_with("special:") { 2 } else { 1 },
                0,
                name.to_lowercase(),
            )
        }
    });
    rows
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn legacy_columns_offsets_overflow_and_stacking() {
        let mut b = json!({"side":"b","position":"l","alignment":"r","layout":{},"padding":0,"offsetX":0,"offsetY":0});
        let r = geometry(&b, 1920., 1080., 400., 40., 1.);
        assert_eq!(r.x, 240.);
        assert_eq!(r.x + r.width, 640.);
        b["offsetX"] = json!("10%");
        b["offsetY"] = json!("-40px");
        let r = geometry(&b, 1920., 1080., 400., 40., 2.);
        assert_eq!((r.x, r.y), (432., 1020.));
        b["offsetX"] = json!(0);
        b["offsetY"] = json!(0);
        let r = geometry(&b, 1920., 1080., 900., 40., 1.);
        assert_eq!((r.x, r.width), (0., 640.));
        b["stackOffset"] = json!(48);
        for (side, depth) in [("t", 48.), ("b", 992.), ("l", 48.), ("r", 1832.)] {
            b["side"] = json!(side);
            let r = geometry(&b, 1920., 1080., 40., 40., 1.);
            assert_eq!(if horizontal(&b) { r.y } else { r.x }, depth);
        }
    }
    #[test]
    fn empty_and_configured_workspaces_are_not_window_activation() {
        use tjik_desktop::DesktopSession;
        let snapshot = tjik_desktop::fake::Fake::default().snapshot().unwrap();
        let options = json!({"persistent":{"Synthetic":[7]},"labels":{"Empty":"Vacant"}});
        let rows = workspaces(&snapshot, &snapshot.outputs[0], "Synthetic", &options);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].target.name, "7");
        assert!(rows[0].target.id.is_none());
        assert!(rows
            .iter()
            .any(|r| r.label == "Vacant" && r.target.id.as_deref() == Some("desk-empty")));
    }
}

/// Workspace-scoped previews use the selected adapter's normalized output state.
/// Unknown workspace state fails closed; no host IPC or compositor-name checks here.
pub fn workspace_visible(output: &Output, workspace: Option<&str>) -> bool {
    workspace.is_none_or(|wanted| {
        output
            .workspace
            .as_ref()
            .is_some_and(|w| w.name.as_deref().unwrap_or(&w.id) == wanted)
    })
}
#[cfg(test)]
mod visibility_tests {
    use super::*;
    use tjik_desktop::DesktopSession;
    #[test]
    fn scoped_preview_follows_only_matching_output_workspace() {
        let fake = tjik_desktop::fake::Fake::default();
        let mut output = fake.snapshot().unwrap().outputs[0].clone();
        assert!(workspace_visible(&output, None));
        assert!(!workspace_visible(&output, Some("7")));
        output.workspace.as_mut().unwrap().name = Some("7".into());
        assert!(workspace_visible(&output, Some("7")));
        output.workspace = None;
        assert!(!workspace_visible(&output, Some("7")));
        assert!(workspace_visible(&output, None));
    }
}
