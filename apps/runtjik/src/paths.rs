use std::path::PathBuf;
pub fn root() -> PathBuf {
    std::env::var_os("RUNTJIK_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let exe = std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(PathBuf::from));
            exe.filter(|p| p.join("assets").is_dir())
                .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")))
        })
}

pub fn command_icon(name: &str) -> String {
    let path = root()
        .join("assets/icons/commands")
        .join(format!("runtjik-{name}.svg"));
    if path.is_file() {
        path.to_string_lossy().into_owned()
    } else {
        name.to_owned()
    }
}
