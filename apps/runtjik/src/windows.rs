//! Presentation serialization of the shared desktop model; no compositor schema.
use anyhow::Result;
use serde_json::Value;
use tjik_desktop::{Desktop, WindowId};
pub fn clients_from(desktop: &dyn Desktop) -> Result<Vec<Value>> {
    Ok(desktop
        .snapshot()?
        .windows
        .into_iter()
        .map(|w| serde_json::to_value(w).expect("window serialization"))
        .collect())
}
pub fn clients() -> Result<Vec<Value>> {
    clients_from(tjik_desktop::selected().as_ref())
}
pub fn outputs() -> Result<Vec<Value>> {
    Ok(tjik_desktop::selected()
        .snapshot()?
        .outputs
        .into_iter()
        .map(|o| serde_json::to_value(o).expect("output serialization"))
        .collect())
}
pub fn focus(client: &Value) -> Result<()> {
    Ok(tjik_desktop::selected().activate(&id(client)?)?)
}
pub fn close(client: &Value) -> Result<()> {
    Ok(tjik_desktop::selected().close(&id(client)?)?)
}
pub fn id(client: &Value) -> Result<WindowId> {
    Ok(serde_json::from_value(client["id"].clone())?)
}
#[cfg(test)]
mod tests {
    use super::*;
    use tjik_desktop::WindowActions;
    #[test]
    fn fake_windows_use_the_application_path() {
        let fake = tjik_desktop::fake::Fake::default();
        let clients = clients_from(&fake).unwrap();
        assert_eq!(clients[0]["app_id"], "editor");
        let id = id(&clients[0]).unwrap();
        fake.activate(&id).unwrap();
        fake.close(&id).unwrap();
        assert_eq!(fake.commands.lock().unwrap().len(), 2);
    }
}
