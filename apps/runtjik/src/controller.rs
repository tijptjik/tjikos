//! Delegate launcher lifecycle to the native frontend's locked control socket.
use anyhow::Result;
use std::os::unix::process::CommandExt;
pub fn control(action: &str) -> Result<()> {
    Err(
        std::process::Command::new(crate::paths::root().join("runtjik-gtk"))
            .arg(action)
            .exec()
            .into(),
    )
}
