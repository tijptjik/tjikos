use crate::model::Contact;
use std::collections::{BTreeMap, HashSet};

#[derive(Clone)]
pub struct Person {
    pub key: String,
    pub name: String,
    pub addresses: Vec<Contact>,
    pub photo: String,
}

pub fn people(contacts: &[Contact]) -> Vec<Person> {
    let mut groups = BTreeMap::<String, Person>::new();
    for contact in contacts {
        let name = contact
            .name
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        let key = if contact.resource_name.is_empty() {
            format!("email:{}", contact.email.to_lowercase())
        } else {
            format!("google:{}", contact.resource_name)
        };
        let person = groups.entry(key.clone()).or_insert_with(|| Person {
            key,
            name: if name.is_empty() {
                contact.email.clone()
            } else {
                name
            },
            addresses: vec![],
            photo: String::new(),
        });
        if person.photo.is_empty() {
            person.photo = contact.photo.clone();
        }
        if !person
            .addresses
            .iter()
            .any(|address| address.email.eq_ignore_ascii_case(&contact.email))
        {
            person.addresses.push(contact.clone());
        }
    }
    groups.into_values().collect()
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct DefaultAddress {
    pub name: String,
    pub email: String,
    pub aliases: Vec<String>,
}
pub type Defaults = BTreeMap<String, DefaultAddress>;

/// Carry legacy preferences over only when their selected email has one Google owner.
/// Names are never used to infer identity. Existing ID-based preferences win.
pub fn migrate_defaults(defaults: &mut Defaults, contacts: &[Contact]) -> bool {
    let people = people(contacts);
    let migrations: Vec<_> = defaults
        .iter()
        .filter(|(key, _)| key.starts_with("name:") || key.starts_with("email:"))
        .filter_map(|(key, preferred)| {
            let mut matches = people.iter().filter(|person| {
                person
                    .addresses
                    .iter()
                    .any(|address| address.email.eq_ignore_ascii_case(&preferred.email))
            });
            let person = matches.next()?;
            if matches.next().is_some() || !person.key.starts_with("google:") {
                return None;
            }
            Some((
                key.clone(),
                person.key.clone(),
                DefaultAddress {
                    name: person.name.clone(),
                    email: preferred.email.clone(),
                    aliases: person
                        .addresses
                        .iter()
                        .map(|address| address.email.clone())
                        .collect(),
                },
            ))
        })
        .collect();
    let changed = !migrations.is_empty();
    for (old, key, preferred) in migrations {
        defaults.remove(&old);
        defaults.entry(key).or_insert(preferred);
    }
    changed
}

pub fn ordered_people(contacts: &[Contact], defaults: &Defaults) -> Vec<Person> {
    let mut people = people(contacts);
    for person in &mut people {
        if let Some(preferred) = defaults.get(&person.key) {
            person
                .addresses
                .sort_by_key(|address| !address.email.eq_ignore_ascii_case(&preferred.email));
        }
    }
    people
}

pub fn toggle_default(defaults: &mut Defaults, person: &Person, address: &Contact) {
    if defaults
        .get(&person.key)
        .is_some_and(|preferred| preferred.email.eq_ignore_ascii_case(&address.email))
    {
        defaults.remove(&person.key);
    } else {
        defaults.insert(
            person.key.clone(),
            DefaultAddress {
                name: person.name.clone(),
                email: address.email.clone(),
                aliases: person
                    .addresses
                    .iter()
                    .map(|address| address.email.clone())
                    .collect(),
            },
        );
    }
}

pub fn warnings(to: &str, contacts: &[Contact], defaults: &Defaults) -> Vec<String> {
    let Ok(recipients) = crate::model::addresses(to) else {
        return vec![];
    };
    let recipients: HashSet<_> = recipients
        .into_iter()
        .map(|(_, email)| email.to_lowercase())
        .collect();
    let people = people(contacts);
    defaults
        .iter()
        .filter_map(|(key, preferred)| {
            if recipients.contains(&preferred.email.to_lowercase()) {
                return None;
            }
            let aliases = people
                .iter()
                .find(|person| &person.key == key)
                .map(|person| {
                    person
                        .addresses
                        .iter()
                        .map(|address| address.email.clone())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_else(|| preferred.aliases.clone());
            let selected: Vec<_> = aliases
                .into_iter()
                .filter(|email| recipients.contains(&email.to_lowercase()))
                .collect();
            if selected.is_empty() {
                None
            } else {
                Some(format!(
                    "{}: To uses {}. Default: {}",
                    preferred.name,
                    selected.join(", "),
                    preferred.email
                ))
            }
        })
        .collect()
}

pub fn suggestions(contacts: &[Contact], query: &str) -> Vec<Contact> {
    let query = query.trim().to_lowercase();
    people(contacts)
        .into_iter()
        .filter(|person| {
            person.name.to_lowercase().contains(&query)
                || person.addresses.iter().any(|contact| {
                    contact.email.to_lowercase().contains(&query)
                        || contact.name.to_lowercase().contains(&query)
                })
        })
        .take(12)
        .flat_map(|person| person.addresses)
        .collect()
}

pub fn initials(name: &str) -> String {
    name.split_whitespace()
        .take(2)
        .filter_map(|word| word.chars().next())
        .flat_map(char::to_uppercase)
        .collect()
}

/// Independent of the mail worker: a slow photo host must not delay sending mail.
pub fn photo_worker(
    events: std::sync::mpsc::Sender<crate::worker::Event>,
) -> std::sync::mpsc::Sender<Vec<String>> {
    let (tx, rx) = std::sync::mpsc::channel::<Vec<String>>();
    std::thread::spawn(move || {
        let mut cache = std::collections::HashMap::<String, Vec<u8>>::new();
        let mut failed = std::collections::HashMap::<String, std::time::Instant>::new();
        while let Ok(mut urls) = rx.recv() {
            for latest in rx.try_iter() {
                urls = latest;
            }
            let mut seen = HashSet::new();
            urls.retain(|url| !url.is_empty() && seen.insert(url.clone()));
            for url in urls.into_iter().take(12) {
                if let Some(bytes) = cache.get(&url) {
                    let _ = events.send(crate::worker::Event::ContactPhoto {
                        url,
                        bytes: bytes.clone(),
                    });
                    continue;
                }
                if failed
                    .get(&url)
                    .is_some_and(|last| last.elapsed() < std::time::Duration::from_secs(60))
                {
                    continue;
                }
                match crate::google::contact_photo(&url) {
                    Ok(bytes) => {
                        if cache.len() >= 128 {
                            cache.clear();
                        }
                        cache.insert(url.clone(), bytes.clone());
                        let _ = events.send(crate::worker::Event::ContactPhoto { url, bytes });
                    }
                    Err(_) => {
                        if failed.len() >= 128 {
                            failed.clear();
                        }
                        failed.insert(url, std::time::Instant::now());
                    }
                }
            }
        }
    });
    tx
}

#[cfg(test)]
mod tests {
    use super::{people, suggestions};
    use crate::model::Contact;
    fn contact(name: &str, email: &str) -> Contact {
        Contact {
            resource_name: if name.trim().is_empty() {
                String::new()
            } else {
                format!(
                    "people/{}",
                    name.split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ")
                        .to_lowercase()
                )
            },
            name: name.into(),
            email: email.into(),
            ..Default::default()
        }
    }
    #[test]
    fn groups_ids_and_preserves_all_addresses_without_merging_unnamed_people() {
        let contacts = vec![
            contact("Mira Chen", "mira@example.test"),
            contact(" mira  CHEN ", "work@example.test"),
            contact("Mira Chen", "MIRA@example.test"),
            contact("", "one@example.test"),
            contact("", "two@example.test"),
            contact("Jun Park", "jun@example.test"),
        ];
        let groups = people(&contacts);
        assert_eq!(groups.len(), 4);
        assert_eq!(
            groups
                .iter()
                .find(|person| person.key == "google:people/mira chen")
                .unwrap()
                .addresses
                .len(),
            2
        );
        assert_eq!(suggestions(&contacts, "work@").len(), 2);
        assert_eq!(suggestions(&contacts, "Jun").len(), 1);
    }
    #[test]
    fn identity_survives_renames_and_never_merges_same_name_people() {
        let mut first = contact("Mira Chen", "personal@example.test");
        first.resource_name = "people/one".into();
        let mut renamed = contact("Mira Z", "work@example.test");
        renamed.resource_name = first.resource_name.clone();
        let mut other = contact("Mira Chen", "other@example.test");
        other.resource_name = "people/two".into();
        let rows = vec![first.clone(), renamed.clone(), other];
        let groups = people(&rows);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].key, "google:people/one");
        assert_eq!(groups[0].addresses.len(), 2);
        assert_eq!(suggestions(&rows, "Mira Z").len(), 2);

        let mut defaults = super::Defaults::new();
        super::toggle_default(&mut defaults, &groups[0], &renamed);
        first.name = "New name".into();
        let renamed_rows = vec![first, renamed];
        assert_eq!(
            super::ordered_people(&renamed_rows, &defaults)[0].addresses[0].email,
            "work@example.test"
        );
        assert_eq!(
            super::warnings("personal@example.test", &renamed_rows, &defaults).len(),
            1
        );
    }

    #[test]
    fn old_cache_without_identity_loads_and_groups_only_by_email() {
        let rows: Vec<Contact> = serde_json::from_value(serde_json::json!([
            {"name": "Mira Chen", "email": "one@example.test"},
            {"name": "Mira Chen", "email": "two@example.test"},
            {"name": "Another name", "email": "ONE@example.test"}
        ]))
        .unwrap();
        assert!(rows.iter().all(|row| row.resource_name.is_empty()));
        let groups = people(&rows);
        assert_eq!(groups.len(), 2);
        assert!(groups.iter().all(|person| person.addresses.len() == 1));
        let store = crate::store::Store::open(None).unwrap();
        let mut row = rows[0].clone();
        row.resource_name = "people/persisted".into();
        store.put("contacts", &vec![row]).unwrap();
        let restored = store.get::<Vec<Contact>>("contacts").unwrap().unwrap();
        assert_eq!(people(&restored)[0].key, "google:people/persisted");
    }

    #[test]
    fn legacy_defaults_migrate_by_unique_email_without_overwriting_new_preferences() {
        let rows = vec![
            contact("Mira Chen", "personal@example.test"),
            contact("Mira Chen", "work@example.test"),
        ];
        let person = people(&rows).remove(0);
        let mut defaults = super::Defaults::new();
        let legacy = super::DefaultAddress {
            name: "Old name".into(),
            email: "WORK@example.test".into(),
            aliases: vec!["stale@example.test".into()],
        };
        defaults.insert("name:old name".into(), legacy.clone());
        assert!(super::migrate_defaults(&mut defaults, &rows));
        assert_eq!(defaults.len(), 1);
        assert_eq!(defaults[&person.key].aliases.len(), 2);
        assert_eq!(
            super::ordered_people(&rows, &defaults)[0].addresses[0].email,
            "work@example.test"
        );
        assert!(!super::migrate_defaults(&mut defaults, &rows));
        super::toggle_default(&mut defaults, &person, &rows[0]);
        defaults.insert("name:old name".into(), legacy.clone());
        assert!(super::migrate_defaults(&mut defaults, &rows));
        assert_eq!(defaults[&person.key].email, "personal@example.test");
        defaults.clear();
        defaults.insert("name:old name".into(), legacy);
        let mut ambiguous = rows.clone();
        ambiguous.push(contact("Someone else", "work@example.test"));
        assert!(!super::migrate_defaults(&mut defaults, &ambiguous));
        assert!(defaults.contains_key("name:old name"));
        assert!(!super::migrate_defaults(&mut defaults, &[]));
    }

    #[test]
    fn limit_is_people_not_addresses() {
        let contacts: Vec<_> = (0..15)
            .map(|index| contact("Mira Chen", &format!("mira{index}@example.test")))
            .collect();
        assert_eq!(suggestions(&contacts, "Mira").len(), 15);
    }
    #[test]
    fn default_sorts_first_toggles_off_and_warns_for_alternate_to() {
        let contacts = vec![
            contact("Mira Chen", "personal@example.test"),
            contact("Mira Chen", "work@example.test"),
        ];
        let person = people(&contacts).remove(0);
        let mut defaults = super::Defaults::new();
        super::toggle_default(&mut defaults, &person, &person.addresses[1]);
        assert_eq!(
            super::ordered_people(&contacts, &defaults)[0].addresses[0].email,
            "work@example.test"
        );
        assert_eq!(
            super::warnings(
                "\"Chen, Mira\" <PERSONAL@example.test>",
                &contacts,
                &defaults
            )
            .len(),
            1
        );
        assert!(super::warnings("work@example.test", &contacts, &defaults).is_empty());
        assert!(
            super::warnings(
                "personal@example.test, work@example.test",
                &contacts,
                &defaults
            )
            .is_empty()
        );
        assert!(super::warnings("unknown@example.test", &contacts, &defaults).is_empty());
        let store = crate::store::Store::open(None).unwrap();
        store.put("contact_defaults", &defaults).unwrap();
        let restored = store
            .get::<super::Defaults>("contact_defaults")
            .unwrap()
            .unwrap();
        assert_eq!(
            super::warnings("personal@example.test", &[], &restored).len(),
            1
        );
        super::toggle_default(&mut defaults, &person, &person.addresses[1]);
        assert!(defaults.is_empty());
        assert!(super::warnings("personal@example.test", &contacts, &defaults).is_empty());
    }
}
