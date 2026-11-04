use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use std::{collections::HashSet, io::Read};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Contact {
    pub to: String,
    pub first_name: String,
    pub last_name: String,
}

pub fn read(input: impl Read) -> Result<Vec<Contact>> {
    let mut bytes = Vec::new();
    input.take(131_073).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 131_072, "contact input exceeds 128 KiB");
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).context("expected contact JSON")?;
    let mut contacts: Vec<Contact> = if value.is_array() {
        serde_json::from_value(value)?
    } else {
        vec![serde_json::from_value(value)?]
    };
    ensure!(
        (1..=100).contains(&contacts.len()),
        "provide 1–100 contacts"
    );
    let mut seen = HashSet::new();
    for contact in &mut contacts {
        contact.to = whatsover_engine::normalize_recipient(&contact.to)?;
        ensure!(
            contact.to.ends_with("@s.whatsapp.net"),
            "contacts require a phone-number JID"
        );
        ensure!(
            seen.insert(contact.to.clone()),
            "duplicate contact phone number"
        );
        for name in [&contact.first_name, &contact.last_name] {
            ensure!(
                name.len() <= 200 && !name.chars().any(char::is_control),
                "names must be at most 200 bytes without control characters"
            );
        }
        contact.first_name = contact.first_name.trim().to_owned();
        contact.last_name = contact.last_name.trim().to_owned();
        ensure!(
            !contact.first_name.is_empty(),
            "firstName must not be empty"
        );
    }
    Ok(contacts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_is_validated_before_any_network_write() {
        let valid = r#"{"to":"+15555550100","firstName":" Test ","lastName":"Person"}"#;
        let contacts = read(valid.as_bytes()).unwrap();
        assert_eq!(contacts[0].first_name, "Test");
        assert_eq!(contacts[0].to, "15555550100@s.whatsapp.net");
        assert!(read(format!("[{valid},{valid}]").as_bytes()).is_err());
        assert!(
            read(
                valid
                    .replace("+15555550100", "120363000000000000@g.us")
                    .as_bytes()
            )
            .is_err()
        );
        assert!(read(valid.replace(" Test ", "\\n").as_bytes()).is_err());
        assert!(read(valid.replace("lastName", "description").as_bytes()).is_err());
    }
}
