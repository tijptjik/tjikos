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

/// Match by address and disambiguate shared mailboxes by name; never merge people.
pub fn sender_details(from: &str, contacts: &[Contact]) -> Person {
    let (name, email) = crate::model::addresses(from)
        .ok()
        .and_then(|rows| rows.into_iter().next())
        .unwrap_or_else(|| (from.into(), String::new()));
    // Resolve only matching People records, retaining all of their aliases.
    // Grouping the entire address book for each visible sender stalls rendering.
    let ids: HashSet<_> = contacts
        .iter()
        .filter(|contact| contact.email.eq_ignore_ascii_case(&email))
        .filter(|contact| !contact.resource_name.is_empty())
        .map(|contact| contact.resource_name.as_str())
        .collect();
    let relevant: Vec<_> = contacts
        .iter()
        .filter(|contact| {
            if contact.resource_name.is_empty() {
                contact.email.eq_ignore_ascii_case(&email)
            } else {
                ids.contains(contact.resource_name.as_str())
            }
        })
        .cloned()
        .collect();
    let candidates: Vec<_> = people(&relevant)
        .into_iter()
        .filter(|person| {
            person
                .addresses
                .iter()
                .any(|a| a.email.eq_ignore_ascii_case(&email))
        })
        .collect();
    if candidates.len() == 1 {
        return candidates[0].clone();
    }
    let named: Vec<_> = candidates
        .iter()
        .filter(|person| person.name.eq_ignore_ascii_case(&name))
        .collect();
    if named.len() == 1 {
        return named[0].clone();
    }
    Person {
        key: format!("sender:{email}"),
        name: if name.is_empty() {
            email.clone()
        } else {
            name.clone()
        },
        addresses: if email.is_empty() {
            vec![]
        } else {
            vec![Contact {
                resource_name: String::new(),
                name,
                email,
                photo: String::new(),
            }]
        },
        photo: String::new(),
    }
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

#[derive(Clone, Debug)]
pub struct PhotoRequest {
    pub key: String,
    pub urls: Vec<String>,
}

/// Sender identity resolution is stable until the contact book changes.
/// Keep it out of repeated hover redraws, including already-loaded avatars.
#[derive(Default)]
pub struct PhotoCache(std::cell::RefCell<std::collections::HashMap<String, PhotoRequest>>);
impl PhotoCache {
    pub fn clear(&self) {
        self.0.borrow_mut().clear();
    }
    pub fn get(&self, from: &str, contacts: &[Contact]) -> PhotoRequest {
        if let Some(request) = self.0.borrow().get(from) {
            return request.clone();
        }
        let request = sender_photo(from, contacts);
        let mut entries = self.0.borrow_mut();
        if entries.len() >= 4096 {
            entries.clear();
        }
        entries.insert(from.to_owned(), request.clone());
        request
    }
}

pub fn mailbox(name: &str, email: &str) -> String {
    if name.is_empty() {
        return email.to_owned();
    }
    format!(
        "\"{}\" <{email}>",
        name.replace('\\', "\\\\").replace('\"', "\\\"")
    )
}

pub fn sender_photo(from: &str, contacts: &[Contact]) -> PhotoRequest {
    use sha2::{Digest, Sha256};
    let (_, email) = crate::model::addresses(from)
        .ok()
        .and_then(|addresses| addresses.first().cloned())
        .unwrap_or_default();
    let email = email.trim().to_lowercase();
    let matching: Vec<_> = contacts
        .iter()
        .filter(|contact| contact.email.eq_ignore_ascii_case(&email) && !contact.photo.is_empty())
        .collect();
    let photos: HashSet<_> = matching
        .iter()
        .map(|contact| contact.photo.as_str())
        .collect();
    // A shared address does not establish a person's identity. Never select an
    // arbitrary contact's portrait when distinct people share that address.
    let google = if photos.len() == 1 {
        photos.iter().next().map(|photo| (*photo).to_owned())
    } else {
        None
    };
    let person = sender_details(&email, contacts);
    let known_person = person.key.starts_with("google:");
    let google = if known_person && !person.photo.is_empty() {
        Some(person.photo.clone())
    } else {
        google
    };
    // Aliases on the same People record share both their portrait and fallback.
    let identity = if known_person {
        person.key.as_str()
    } else {
        email.as_str()
    };
    let identity_hash = format!("{:x}", Sha256::digest(identity.as_bytes()));
    let avatar_email = if known_person {
        person
            .addresses
            .iter()
            .map(|a| a.email.to_lowercase())
            .min()
            .unwrap_or_else(|| email.clone())
    } else {
        email.clone()
    };
    let hash = format!("{:x}", Sha256::digest(avatar_email.as_bytes()));
    let mut urls = Vec::new();
    if let Some(google) = &google {
        urls.push(google.clone());
    }
    if !email.is_empty() {
        urls.push(format!("https://www.gravatar.com/avatar/{hash}?s=64&d=404"));
    }
    PhotoRequest {
        key: format!("sender:{identity_hash}:{}", google.unwrap_or_default()),
        urls,
    }
}

pub fn identicon(key: &str) -> Vec<u8> {
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(
        key.strip_prefix("sender:")
            .unwrap_or(key)
            .split(':')
            .next()
            .unwrap_or(key)
            .as_bytes(),
    );
    let color = [
        100 + hash[0] % 140,
        100 + hash[1] % 140,
        100 + hash[2] % 140,
        255,
    ];
    let image = image::RgbaImage::from_fn(64, 64, |x, y| {
        if !(7..57).contains(&x) || !(7..57).contains(&y) {
            return image::Rgba([46, 42, 64, 255]);
        }
        let col = (x - 7) / 10;
        let col = col.min(4 - col);
        let row = (y - 7) / 10;
        image::Rgba(if hash[(row * 3 + col + 3) as usize] & 1 == 1 {
            color
        } else {
            [46, 42, 64, 255]
        })
    });
    let mut bytes = std::io::Cursor::new(Vec::new());
    image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
    bytes.into_inner()
}

/// Independent of the mail worker: slow avatar hosts never delay sending mail.
pub fn photo_worker(
    events: std::sync::mpsc::Sender<crate::worker::Event>,
) -> std::sync::mpsc::Sender<Vec<PhotoRequest>> {
    let (tx, rx) = std::sync::mpsc::channel::<Vec<PhotoRequest>>();
    std::thread::spawn(move || {
        let mut cache = std::collections::HashMap::<String, Vec<u8>>::new();
        while let Ok(requests) = rx.recv() {
            for request in requests {
                if request.key.starts_with("favicon:") {
                    let bytes = request
                        .urls
                        .first()
                        .map(|url| crate::favicons::cached(url))
                        .unwrap_or_default();
                    if !bytes.is_empty() {
                        let _ = events.send(crate::worker::Event::ContactPhoto {
                            url: request.key,
                            bytes,
                        });
                    }
                    continue;
                }
                let bytes = cache.get(&request.key).cloned().unwrap_or_else(|| {
                    request
                        .urls
                        .iter()
                        .find_map(|url| {
                            let bytes = crate::google::contact_photo(url).ok()?;
                            // Invalid or oversized decoded images also fall through to the next provider.
                            let mut reader = image::ImageReader::new(std::io::Cursor::new(&bytes))
                                .with_guessed_format()
                                .ok()?;
                            let mut limits = image::Limits::default();
                            limits.max_image_width = Some(2048);
                            limits.max_image_height = Some(2048);
                            reader.limits(limits);
                            let image = reader.decode().ok()?.thumbnail(64, 64);
                            let mut png = std::io::Cursor::new(Vec::new());
                            image.write_to(&mut png, image::ImageFormat::Png).ok()?;
                            Some(png.into_inner())
                        })
                        .unwrap_or_else(|| identicon(&request.key))
                });
                if cache.len() >= 512 {
                    cache.clear();
                }
                cache.insert(request.key.clone(), bytes.clone());
                let _ = events.send(crate::worker::Event::ContactPhoto {
                    url: request.key,
                    bytes,
                });
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
    fn sender_photo_cache_refreshes_identity_after_contact_book_changes() {
        let cache = super::PhotoCache::default();
        let mut contacts = vec![contact("Alice", "alice@example.test")];
        let before = cache.get("alice@example.test", &contacts);
        assert_eq!(
            before.key,
            super::sender_photo("alice@example.test", &contacts).key
        );
        contacts[0].photo = "https://example.test/alice.png".into();
        cache.clear();
        let after = cache.get("alice@example.test", &contacts);
        assert_ne!(before.key, after.key);
        assert_eq!(
            after.key,
            super::sender_photo("alice@example.test", &contacts).key
        );
        assert_eq!(after.urls, cache.get("alice@example.test", &contacts).urls);
    }

    #[test]
    #[ignore = "synthetic timing probe; run explicitly in release mode"]
    fn sender_photo_render_cost() {
        let contacts: Vec<_> = (0..5000)
            .map(|i| Contact {
                resource_name: format!("people/{i}"),
                name: format!("Person {i}"),
                email: format!("person{i}@example.test"),
                photo: String::new(),
            })
            .collect();
        let started = std::time::Instant::now();
        for _ in 0..5 {
            for i in 0..100 {
                std::hint::black_box(super::sender_photo(
                    &format!("person{i}@example.test"),
                    &contacts,
                ));
            }
        }
        eprintln!(
            "100 uncached sender lookups / 5000 contacts: {:.2} ms per redraw",
            started.elapsed().as_secs_f64() * 1000. / 5.
        );
        let cache = super::PhotoCache::default();
        for i in 0..100 {
            cache.get(&format!("person{i}@example.test"), &contacts);
        }
        let started = std::time::Instant::now();
        for _ in 0..1000 {
            for i in 0..100 {
                std::hint::black_box(cache.get(&format!("person{i}@example.test"), &contacts));
            }
        }
        eprintln!(
            "100 cached sender lookups / 5000 contacts: {:.3} ms per redraw",
            started.elapsed().as_secs_f64()
        );
    }

    #[test]
    fn sender_details_preserve_people_and_fall_back_for_unknown_senders() {
        let contacts = vec![
            contact("Mira", "shared@example.test"),
            contact("Mira", "mira@example.test"),
            contact("Jun", "shared@example.test"),
        ];
        let mira = super::sender_details("Mira <SHARED@example.test>", &contacts);
        assert_eq!(mira.name, "Mira");
        assert_eq!(mira.addresses.len(), 2);
        let shared = super::sender_details("shared@example.test", &contacts);
        assert!(shared.key.starts_with("sender:"));
        assert_eq!(shared.addresses.len(), 1);
        let unknown = super::sender_details("New Person <new@example.test>", &contacts);
        assert_eq!(unknown.name, "New Person");
        assert_eq!(unknown.addresses[0].email, "new@example.test");
    }
    #[test]
    fn aliases_of_one_saved_person_share_avatar_and_fallback() {
        let mut first = contact("Mira", "mira@example.test");
        first.photo = "https://lh3.googleusercontent.com/mira".into();
        let second = contact("Mira", "work@example.test");
        let contacts = vec![first, second];
        let a = super::sender_photo("Mira <mira@example.test>", &contacts);
        let b = super::sender_photo("work@example.test", &contacts);
        assert_eq!(a.key, b.key);
        assert_eq!(a.urls, b.urls);
        assert_eq!(super::identicon(&a.key), super::identicon(&b.key));
        assert_ne!(
            a.key,
            super::sender_photo("Mira <unrelated@example.test>", &contacts).key
        );
    }
    #[test]
    fn avatar_provider_order_and_normalized_identity() {
        let contacts = vec![crate::model::Contact {
            email: "mira@example.test".into(),
            photo: "https://lh3.googleusercontent.com/photo".into(),
            ..Default::default()
        }];
        let request = super::sender_photo("Mira <MIRA@example.test>", &contacts);
        assert_eq!(request.urls[0], contacts[0].photo);
        assert!(request.urls[1].starts_with("https://www.gravatar.com/avatar/"));
        assert!(request.urls[1].ends_with("?s=64&d=404"));
        assert_eq!(
            request.key,
            super::sender_photo("mira@example.test", &contacts).key
        );
        assert_eq!(super::sender_photo("mira@example.test", &[]).urls.len(), 1);
        assert!(super::sender_photo("", &[]).urls.is_empty());
    }
    #[test]
    fn avatar_mailbox_preserves_names_with_commas_and_quotes() {
        let name = "Example, Morgan \"M\"";
        assert_eq!(
            crate::model::addresses(&super::mailbox(name, "morgan@example.test")).unwrap(),
            vec![(name.into(), "morgan@example.test".into())]
        );
    }
    #[test]
    fn shared_address_avatar_is_independent_of_display_name_and_contact_order() {
        let first = crate::model::Contact {
            name: "Rhea Example".into(),
            email: "shared@example.test".into(),
            photo: "https://lh3.googleusercontent.com/rhea".into(),
            ..Default::default()
        };
        let second = crate::model::Contact {
            name: "Morgan Example".into(),
            email: "shared@example.test".into(),
            photo: "https://lh3.googleusercontent.com/morgan".into(),
            ..Default::default()
        };
        for contacts in [
            vec![first.clone(), second.clone()],
            vec![second.clone(), first.clone()],
        ] {
            let plain = super::sender_photo("shared@example.test", &contacts);
            for name in ["Morgan Example", "Rhea Example", "Someone Else"] {
                let named =
                    super::sender_photo(&super::mailbox(name, "shared@example.test"), &contacts);
                assert_eq!(named.key, plain.key);
                assert_eq!(named.urls, plain.urls);
            }
            assert!(
                super::sender_photo("shared@example.test", &contacts).urls[0]
                    .contains("gravatar.com")
            );
            assert!(
                super::sender_photo("Someone Else <shared@example.test>", &contacts).urls[0]
                    .contains("gravatar.com")
            );
        }
    }
    #[test]
    fn identicons_are_deterministic_distinct_and_mirrored() {
        let bytes = super::identicon("mira@example.test");
        assert_eq!(bytes, super::identicon("mira@example.test"));
        assert_ne!(bytes, super::identicon("jun@example.test"));
        let image = image::load_from_memory(&bytes).unwrap().to_rgba8();
        for y in 0..64 {
            for x in 0..64 {
                assert_eq!(image.get_pixel(x, y), image.get_pixel(63 - x, y));
            }
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
