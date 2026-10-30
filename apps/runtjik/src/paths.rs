use std::path::PathBuf;
pub fn root() -> PathBuf {
    std::env::var_os("RUNTJIK_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let exe = std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(PathBuf::from));
            exe.filter(|p| p.join("qml").is_dir())
                .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")))
        })
}
