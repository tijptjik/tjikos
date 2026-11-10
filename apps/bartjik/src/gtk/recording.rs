//! Derived recording island; never starts another provider or changes user config.
use crate::util::{arr, s};
use serde_json::{json, Value};

pub const PROVIDER: &str = "recording-indicator";

pub fn add_island(bar: &mut Value) {
    let Some((index, name)) = arr(&bar["islands"])
        .iter()
        .enumerate()
        .find_map(|(i, island)| {
            arr(&island["modules"])
                .iter()
                .find(|name| bar["moduleConfigs"][s(name)]["provider"] == "icontray")
                .map(|name| (i, s(name).to_owned()))
        })
    else {
        return;
    };
    // Internal names cannot collide with configured module/island identifiers.
    let id = "@recording";
    let tray = &bar["moduleConfigs"][&name];
    let module = json!({
        "provider": PROVIDER,
        "comparisonItems": [],
        "comparisonActivity": tray["comparisonActivity"],
        "style": tray["style"],
    });
    let mut island = bar["islands"][index].clone();
    island["id"] = json!(id);
    island["modules"] = json!([id]);
    island["style"] = crate::config::merge(&island["style"], &json!({"paddingX":6}));
    bar["moduleConfigs"][id] = module;
    bar["islands"].as_array_mut().unwrap().insert(index, island);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn separate_indicator_precedes_tray_and_has_no_application_icons() {
        for phase in ["recording", "processing", "idle", "unavailable"] {
            let mut bar = json!({"islands":[{"id":"clock","modules":["clock"]},{"id":"apps","modules":["tray"]}],"moduleConfigs":{"tray":{"provider":"icontray","comparisonActivity":{"hyprvoice":phase},"comparisonItems":[{"id":"app"}]}}});
            let original = bar["moduleConfigs"]["tray"].clone();
            add_island(&mut bar);
            assert_eq!(bar["islands"][1]["modules"], json!(["@recording"]));
            assert_eq!(bar["islands"][2]["modules"], json!(["tray"]));
            let indicator = &bar["moduleConfigs"]["@recording"];
            assert_eq!(indicator["provider"], PROVIDER);
            assert_eq!(indicator["comparisonActivity"]["hyprvoice"], phase);
            assert_eq!(indicator["comparisonItems"], json!([]));
            assert_eq!(bar["moduleConfigs"]["tray"], original);
        }
    }

    #[test]
    fn bars_without_a_tray_are_unchanged() {
        let mut bar = json!({"islands":[{"modules":["clock"]}],"moduleConfigs":{}});
        let original = bar.clone();
        add_island(&mut bar);
        assert_eq!(bar, original);
    }
}
