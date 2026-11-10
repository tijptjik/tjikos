//! Host-owned preview location; transport is shared with standalone Tasktjik.
use std::{path::PathBuf, rc::Rc};
use tasktjik_gtk::previews::PreviewProvider;
pub fn provider(demo: bool) -> Option<Rc<dyn PreviewProvider>> {
    if demo {
        return None;
    }
    let home = std::env::var_os("RUNTJIK_HOME")?;
    Some(tasktjik_gtk::preview_helper::provider(
        PathBuf::from(home).join("helpers/page_preview.py"),
    ))
}
