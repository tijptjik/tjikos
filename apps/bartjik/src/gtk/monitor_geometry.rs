//! Logical display geometry for draft editing; no compositor transport.
use crate::util::{arr, num, s};
use serde_json::{json, Value};
#[derive(Clone, Copy, Debug)]
pub struct Box2 {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}
pub fn size(m: &Value) -> (f64, f64) {
    let v = &m["settings"];
    let mode = s(&v["mode"]).split('@').next().unwrap_or("");
    let Some((w, h)) = mode.split_once('x') else {
        return (0., 0.);
    };
    let (mut w, mut h) = (
        w.parse::<f64>().unwrap_or(0.),
        h.parse::<f64>().unwrap_or(0.),
    );
    if v["transform"].as_i64().unwrap_or(0) % 2 != 0 {
        std::mem::swap(&mut w, &mut h);
    }
    let scale = num(&v["scale"]).max(0.25);
    (w / scale, h / scale)
}
pub fn mirrored(m: &Value, all: &[Value]) -> bool {
    m["connected"] == true
        && m["settings"]["enabled"] == true
        && !s(&m["settings"]["mirror"]).is_empty()
        && all.iter().any(|p| {
            p["name"] == m["settings"]["mirror"]
                && p["connected"] == true
                && p["settings"]["enabled"] == true
                && s(&p["settings"]["mirror"]).is_empty()
        })
}
pub fn bounds(all: &[Value]) -> Box2 {
    let rects: Vec<_> = all
        .iter()
        .filter(|m| !mirrored(m, all))
        .map(|m| {
            let (w, h) = size(m);
            Box2 {
                x: num(&m["settings"]["x"]),
                y: num(&m["settings"]["y"]),
                w,
                h,
            }
        })
        .filter(|r| r.w > 0. && r.h > 0.)
        .collect();
    if rects.is_empty() {
        return Box2 {
            x: 0.,
            y: 0.,
            w: 1.,
            h: 1.,
        };
    }
    let x = rects.iter().map(|r| r.x).fold(f64::INFINITY, f64::min);
    let y = rects.iter().map(|r| r.y).fold(f64::INFINITY, f64::min);
    Box2 {
        x,
        y,
        w: rects.iter().map(|r| r.x + r.w).fold(x, f64::max) - x,
        h: rects.iter().map(|r| r.y + r.h).fold(y, f64::max) - y,
    }
}
pub fn fit(b: Box2, w: f64, h: f64) -> (f64, f64, f64) {
    let scale = ((w - 28.) / b.w.max(1.))
        .min((h - 28.) / b.h.max(1.))
        .max(0.001);
    (
        scale,
        (w - b.w * scale) / 2. - b.x * scale,
        (h - b.h * scale) / 2. - b.y * scale,
    )
}
pub fn snap(m: &Value, all: &[Value], x: f64, y: f64, threshold: f64) -> (f64, f64) {
    let (w, h) = size(m);
    let (mut bx, mut by, mut dx, mut dy) = (threshold, threshold, 0., 0.);
    for p in all
        .iter()
        .filter(|p| p["name"] != m["name"] && !mirrored(p, all))
    {
        let (pw, ph) = size(p);
        let (px, py) = (num(&p["settings"]["x"]), num(&p["settings"]["y"]));
        for e in [px, px + pw] {
            for v in [x, x + w] {
                let d = e - v;
                if d.abs() < bx {
                    bx = d.abs();
                    dx = d;
                }
            }
        }
        for e in [py, py + ph] {
            for v in [y, y + h] {
                let d = e - v;
                if d.abs() < by {
                    by = d.abs();
                    dy = d;
                }
            }
        }
    }
    (
        (x + dx).round().clamp(-32768., 32768.),
        (y + dy).round().clamp(-32768., 32768.),
    )
}
pub fn changes(before: &Value, after: &Value) -> Value {
    json!(arr(after)
        .iter()
        .filter(|m| m["connected"] == true)
        .filter_map(|m| {
            let old = arr(before).iter().find(|b| b["name"] == m["name"])?;
            let mut edits = json!({});
            for k in [
                "enabled",
                "mode",
                "x",
                "y",
                "scale",
                "transform",
                "vrr",
                "cm",
                "mirror",
            ] {
                if old["settings"][k] != m["settings"][k] {
                    edits[k] = m["settings"][k].clone();
                }
            }
            if edits.as_object().unwrap().is_empty() {
                None
            } else {
                Some(json!({"name":m["name"],"settings":edits}))
            }
        })
        .collect::<Vec<_>>())
}
pub fn scales(m: &Value) -> Vec<f64> {
    let mut out = vec![
        0.5,
        0.75,
        1.,
        1.25,
        1.5,
        1.75,
        2.,
        2.5,
        3.,
        4.,
        num(&m["settings"]["scale"]),
    ];
    out.sort_by(f64::total_cmp);
    out.dedup();
    let mut unscaled = m.clone();
    unscaled["settings"]["scale"] = json!(1.);
    let (w, h) = size(&unscaled);
    out.retain(|s| {
        *s > 0.
            && w > 0.
            && h > 0.
            && [w, h]
                .iter()
                .all(|d| (d / s - (d / s).round()).abs() < 0.01)
    });
    out
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rotation_scaling_snap_and_diffs() {
        let a = json!({"name":"a","connected":true,"settings":{"mode":"1920x1080@60","scale":1.5,"transform":1,"x":-720,"y":0}});
        assert_eq!(size(&a), (720., 1280.));
        let mut b = a.clone();
        b["name"] = json!("b");
        assert_eq!(snap(&b, &[a.clone()], 3., 5., 10.), (0., 0.));
        let mut next = a.clone();
        next["settings"]["x"] = json!(0);
        assert_eq!(
            changes(&json!([a]), &json!([next]))[0]["settings"],
            json!({"x":0})
        );
    }
}
