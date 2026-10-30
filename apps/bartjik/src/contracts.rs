#[test]
fn frozen_reference_contracts() {
    use crate::*;
    use serde_json::{json, Value};
    let cases: Value = serde_json::from_str(
        &include_str!("../tests/fixtures/native-contracts.json")
            .replace("@ROOT@", &root().to_string_lossy()),
    )
    .unwrap();
    for case in arr(&cases) {
        let a = &case["args"];
        let actual = match s(&case["op"]) {
            "crypto_items" => json!(providers::crypto_items(&a[0], &a[1]).unwrap()),
            "untrusted" => json!(providers::untrusted(a[0].clone()).unwrap()),
            "nm_fields" => json!(providers::fields(s(&a[0]))),
            "battery_row" => battery::row(num(&a[0]), truth(&a[1])),
            "battery_panel" => battery::panel_data(&a[0], &a[1], &a[2]),
            "relays" => connectivity::parse_relays(s(&a[0])).unwrap(),
            "mullvad_panel" => connectivity::mullvad_panel(s(&a[0]), s(&a[1]), &a[2], &a[3]),
            "tailscale_panel" => connectivity::tailscale_panel(&a[0]),
            "timed_lines" => lyrics::timed_lines(s(&a[0])),
            "lyrics_parse" => lyrics::parse(&a[0]).unwrap(),
            "clean_title" => json!(lyrics::clean_title(s(&a[0]), truth(&a[1]))),
            "artist_matches" => json!(lyrics::artist_matches(s(&a[0]), s(&a[1]))),
            "spotify_uri" => json!(navigation::uri(s(&a[0]), s(&a[1]))),
            "totp" => json!(spotify::totp(&a[0], num(&a[1])).unwrap()),
            "spotify_parse" => json!(spotify::parse(&a[0]).unwrap()),
            "unread" => json!(tray::unread(s(&a[0]), &a[1])),
            "pointer" => workspaces::local_position(&a[0], &a[1]).unwrap(),
            "workspace_view" => json!(workspaces::view(&a[0], s(&a[1]), &a[2])),
            "choose" => actions::choose(&a[0], &a[1], s(&a[2]), s(&a[3])),
            "demo" => json!(demo::items(s(&a[0]), &a[1])),
            other => panic!("unknown contract {other}"),
        };
        assert!(
            equivalent(&actual, &case["result"]),
            "{} {}\nactual: {}\nexpected: {}",
            case["op"],
            a,
            actual,
            case["result"]
        );
    }
}
#[test]
fn hostile_items_and_history() {
    use crate::*;
    use serde_json::json;
    assert!(providers::untrusted(json!([{"id":"x","type":"text","text":{"value":7}}])).is_err());
    assert!(cache::normalize(&json!({"prices":[[1,-1],[2,3]]})).is_err());
    assert!(!cache::raster(b"<svg onload='bad'>"));
    assert!(!spotify::valid_cookie("secret\nheader"));
    assert!(navigation::parse_state(b"1234567890#\x08\x10", "AAAAAAAAAAAAAAAAAAAAAA").is_err());
}

fn equivalent(a: &serde_json::Value, b: &serde_json::Value) -> bool {
    use serde_json::Value;
    match (a, b) {
        (Value::Number(a), Value::Number(b)) => a.as_f64() == b.as_f64(),
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| equivalent(a, b))
        }
        (Value::Object(a), Value::Object(b)) => {
            a.len() == b.len()
                && a.iter()
                    .all(|(k, v)| b.get(k).is_some_and(|b| equivalent(v, b)))
        }
        _ => a == b,
    }
}
