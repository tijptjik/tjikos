//! Host-supplied location for existing QSettings-compatible Tasktjik preferences.
use serde_json::{json, Value};
use std::path::Path;
use tjik_gtk::gtk::glib;
pub fn load(path: &Path) -> Value {
    let file = glib::KeyFile::new();
    let _ = file.load_from_file(path, glib::KeyFileFlags::KEEP_COMMENTS);
    json!({
        "domainSort":file.string("tasks","domainSort").map(|v|v.to_string()).unwrap_or_else(|_|"manual".into()),
        "projectSort":file.string("tasks","projectSort").map(|v|v.to_string()).unwrap_or_else(|_|"first-due".into()),
        "manualDomainOrderJson":file.string("tasks","manualDomainOrderJson").map(|v|v.to_string()).unwrap_or_else(|_|"[]".into()),
        "doubleDInterval":file.integer("tasks","doubleDInterval").unwrap_or(350).clamp(150,1000)
    })
}
pub fn save(path: &Path, value: &Value) -> Result<(), String> {
    let file = glib::KeyFile::new();
    let _ = file.load_from_file(path, glib::KeyFileFlags::KEEP_COMMENTS);
    for key in ["domainSort", "projectSort", "manualDomainOrderJson"] {
        file.set_string("tasks", key, value[key].as_str().unwrap_or(""));
    }
    file.set_integer(
        "tasks",
        "doubleDInterval",
        value["doubleDInterval"].as_i64().unwrap_or(350) as i32,
    );
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    file.save_to_file(path).map_err(|e| e.to_string())
}
