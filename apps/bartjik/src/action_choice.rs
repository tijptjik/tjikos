use crate::util::s;
use serde_json::Value;
pub fn choose(module: &Value, row: &Value, part: &str, event: &str) -> Value {
    let event = if event == "click" { "leftClick" } else { event };
    let mut result = Value::Null;
    for a in [
        &row["actions"],
        &row[part]["actions"],
        &module["actions"],
        &module[part]["actions"],
        &module["items"][s(&row["id"])]["actions"],
        &module["items"][s(&row["id"])][part]["actions"],
    ] {
        if event == "leftClick" {
            if let Some(v) = a.get("click") {
                result = v.clone();
            }
        }
        if let Some(v) = a.get(event) {
            result = v.clone();
        }
    }
    result
}
