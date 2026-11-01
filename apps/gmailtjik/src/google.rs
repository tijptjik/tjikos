use crate::{
    auth::{Auth, http},
    model::{Contact, Draft, Label, Message, Page},
    store::Store,
};
use anyhow::{Context, Result, bail};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use reqwest::blocking::Client;
use serde_json::{Value, json};
use std::{collections::HashSet, path::Path, time::Duration};

const API: &str = "https://gmail.googleapis.com/gmail/v1/users/me/";
pub struct Google {
    auth: Auth,
    client: Client,
    api: String,
    people_api: String,
}
impl Google {
    pub fn open(dir: &Path) -> Result<Self> {
        Ok(Self {
            auth: Auth::open(dir)?,
            client: http()?,
            api: API.into(),
            people_api: "https://people.googleapis.com/v1/".into(),
        })
    }
    fn get(&mut self, route: &str, params: &[(&str, &str)]) -> Result<Value> {
        let token = self.auth.token()?;
        read(
            &self.client,
            &token,
            &format!("{}{route}", self.api),
            params,
        )
    }
    pub fn labels(&mut self) -> Result<Vec<Label>> {
        let mut labels: Vec<Label> =
            serde_json::from_value(self.get("labels", &[])?["labels"].clone())?;
        let token = self.auth.token()?;
        for chunk in labels.chunks_mut(6) {
            let values = std::thread::scope(|scope| {
                let jobs: Vec<_> = chunk
                    .iter()
                    .map(|label| {
                        let client = &self.client;
                        let token = &token;
                        let url = format!("{}labels/{}", self.api, label.id);
                        scope.spawn(move || {
                            read(client, token, &url, &[("fields", "messagesUnread")])
                        })
                    })
                    .collect();
                jobs.into_iter()
                    .map(|job| job.join().expect("label worker"))
                    .collect::<Result<Vec<_>>>()
            })?;
            for (label, value) in chunk.iter_mut().zip(values) {
                label.messages_unread = value["messagesUnread"].as_u64().unwrap_or(0);
            }
        }
        Ok(labels)
    }

    pub fn email(&mut self) -> Result<String> {
        Ok(self.get("profile", &[])?["emailAddress"]
            .as_str()
            .context("Missing account email")?
            .into())
    }
    pub fn stop_watch(&mut self) -> Result<()> {
        let token = self.auth.token()?;
        let response = self
            .client
            .post(format!("{}stop", self.api))
            .bearer_auth(token)
            .send()?;
        anyhow::ensure!(
            response.status().is_success(),
            "Stopping Gmail watch failed (HTTP {})",
            response.status().as_u16()
        );
        Ok(())
    }
    pub fn watch(&mut self, topic: &str) -> Result<u64> {
        let token = self.auth.token()?;
        let response = self
            .client
            .post(format!("{}watch", self.api))
            .bearer_auth(token)
            .json(&json!({"topicName":topic}))
            .send()?;
        if !response.status().is_success() {
            bail!("Gmail watch failed (HTTP {})", response.status().as_u16());
        }
        let value: Value = response.json()?;
        let expiration = value["expiration"]
            .as_str()
            .context("Missing watch expiration")?
            .parse::<u64>()?
            / 1000;
        anyhow::ensure!(
            expiration > crate::auth::now(),
            "Gmail returned an expired watch"
        );
        Ok(expiration)
    }
    pub fn changes(&mut self, store: &Store) -> Result<()> {
        let Some(cursor): Option<String> = store.get("history")? else {
            let profile = self.get("profile", &[])?;
            return store.put(
                "history",
                &profile["historyId"]
                    .as_str()
                    .context("Missing history cursor")?,
            );
        };
        let token = self.auth.token()?;
        let mut page_token = String::new();
        let final_cursor = loop {
            let response = self
                .client
                .get(format!("{}history", self.api))
                .bearer_auth(&token)
                .query(&[
                    ("startHistoryId", cursor.as_str()),
                    ("pageToken", &page_token),
                ])
                .send()?;
            if response.status() == reqwest::StatusCode::NOT_FOUND {
                store.invalidate_all()?;
                let profile = self.get("profile", &[])?;
                break profile["historyId"]
                    .as_str()
                    .context("Missing history cursor")?
                    .to_string();
            }
            if !response.status().is_success() {
                bail!("Gmail sync failed ({})", response.status());
            }
            let value: Value = response.json()?;
            if let Some(records) = value["history"].as_array() {
                for record in records {
                    if let Some(messages) = record["messages"].as_array() {
                        for message in messages {
                            if let Some(id) = message["id"].as_str() {
                                store.invalidate(id)?;
                            }
                        }
                    }
                }
            }
            page_token = value["nextPageToken"].as_str().unwrap_or_default().into();
            if page_token.is_empty() {
                break value["historyId"]
                    .as_str()
                    .context("Missing history cursor")?
                    .into();
            }
        };
        store.put("history", &final_cursor)
    }
    pub fn page(&mut self, store: &Store, label: &str, query: &str, page: &str) -> Result<Page> {
        let mut params = vec![("maxResults", "40"), ("q", query)];
        if !label.is_empty() {
            params.push(("labelIds", label));
        }
        if !page.is_empty() {
            params.push(("pageToken", page));
        }
        let value = self.get("messages", &params)?;
        let ids: Vec<String> = value["messages"]
            .as_array()
            .map(|rows| {
                rows.iter()
                    .filter_map(|v| v["id"].as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        let mut messages = Vec::new();
        let mut missing = Vec::new();
        for id in &ids {
            if let Some(message) = store.message(id)? {
                messages.push(message);
            } else {
                missing.push(id.clone());
            }
        }
        let token = self.auth.token()?;
        // Bound concurrent requests to avoid flooding the user's Gmail quota.
        for chunk in missing.chunks(6) {
            let results = std::thread::scope(|scope| {
                let jobs: Vec<_> = chunk
                    .iter()
                    .map(|id| {
                        let client = &self.client;
                        let token = &token;
                        let api = &self.api;
                        scope.spawn(move || -> Result<Message> {
                            let mut message: Message = serde_json::from_value(read(
                                client,
                                token,
                                &format!("{api}messages/{}", segment(id)),
                                &[("format", "full")],
                            )?)?;
                            fetch_text_parts(
                                client,
                                token,
                                api,
                                &message.id,
                                &mut message.payload,
                            )?;
                            Ok(message)
                        })
                    })
                    .collect();
                jobs.into_iter()
                    .map(|job| {
                        job.join()
                            .map_err(|_| anyhow::anyhow!("Gmail fetch worker stopped"))?
                    })
                    .collect::<Result<Vec<_>>>()
            })?;
            for message in results {
                store.save_message(&message)?;
                messages.push(message);
            }
        }
        messages.sort_by_key(|m| ids.iter().position(|id| id == &m.id).unwrap_or(usize::MAX));
        Ok(Page {
            messages,
            next: value["nextPageToken"].as_str().unwrap_or_default().into(),
        })
    }
    pub fn thread(&mut self, store: &Store, id: &str) -> Result<Vec<Message>> {
        let value = self.get(&format!("threads/{}", segment(id)), &[("format", "full")])?;
        let mut messages: Vec<Message> = serde_json::from_value(value["messages"].clone())?;
        let token = self.auth.token()?;
        for message in &mut messages {
            fetch_text_parts(
                &self.client,
                &token,
                &self.api,
                &message.id,
                &mut message.payload,
            )?;
            store.save_message(message)?;
        }
        messages.sort_by_key(|message| message.internal_date.parse::<i64>().unwrap_or(0));
        store.put(&format!("thread:{id}"), &messages)?;
        Ok(messages)
    }
    pub fn modify(&mut self, id: &str, add: &[String], remove: &[String]) -> Result<()> {
        let token = self.auth.token()?;
        let response = self
            .client
            .post(format!("{}messages/{}/modify", self.api, segment(id)))
            .bearer_auth(token)
            .json(&json!({"addLabelIds":add,"removeLabelIds":remove}))
            .send()?;
        if !response.status().is_success() {
            bail!("Gmail did not confirm the change ({})", response.status());
        }
        Ok(())
    }
    pub fn save_draft(&mut self, draft: &Draft, from: &str) -> Result<String> {
        let token = self.auth.token()?;
        let message = message_body(draft, from)?;
        let request = if draft.gmail_id.is_empty() {
            self.client.post(format!("{}drafts", self.api))
        } else {
            self.client
                .put(format!("{}drafts/{}", self.api, segment(&draft.gmail_id)))
        };
        let response = request
            .bearer_auth(token)
            .json(&json!({"message":message}))
            .send()?;
        if !response.status().is_success() {
            bail!("Gmail did not save the draft ({})", response.status());
        }
        Ok(response.json::<Value>()?["id"]
            .as_str()
            .context("Missing draft ID")?
            .into())
    }
    pub fn send(&mut self, draft: &Draft, from: &str) -> Result<()> {
        let token = self.auth.token()?;
        let message = message_body(draft, from)?;
        let (route, body) = if draft.gmail_id.is_empty() {
            ("messages/send", message)
        } else {
            (
                "drafts/send",
                json!({"id":draft.gmail_id,"message":message}),
            )
        };
        // Sending is deliberately never automatically retried: a timeout may mean Gmail accepted it.
        let response = self
            .client
            .post(format!("{}{route}", self.api))
            .bearer_auth(token)
            .json(&body)
            .send()
            .context("Send outcome is unknown. Check Gmail Sent before trying again")?;
        if !response.status().is_success() {
            bail!(
                "Gmail did not confirm sending ({}). Check Sent before retrying",
                response.status()
            );
        }
        Ok(())
    }
    pub fn all_contacts(&mut self) -> Result<Vec<Contact>> {
        if !self.auth.contacts() {
            return Ok(vec![]);
        }
        let token = self.auth.token()?;
        let mut contacts = Vec::new();
        let mut seen = HashSet::new();
        for (route, field, mask) in [
            ("people/me/connections", "connections", "personFields"),
            ("otherContacts", "otherContacts", "readMask"),
        ] {
            let mut page = String::new();
            loop {
                let result = read(
                    &self.client,
                    &token,
                    &format!("{}{route}", self.people_api),
                    &[
                        (mask, "names,emailAddresses,photos"),
                        ("sources", "READ_SOURCE_TYPE_CONTACT"),
                        ("sources", "READ_SOURCE_TYPE_PROFILE"),
                        ("pageSize", "1000"),
                        ("pageToken", &page),
                    ],
                )?;
                if let Some(rows) = result[field].as_array() {
                    for person in rows {
                        let resource_name = person["resourceName"].as_str().unwrap_or_default();
                        let name = person["names"][0]["displayName"]
                            .as_str()
                            .unwrap_or_default();
                        if let Some(emails) = person["emailAddresses"].as_array() {
                            for email in emails {
                                if let Some(email) = email["value"].as_str() {
                                    if seen.insert((resource_name.to_owned(), email.to_lowercase()))
                                    {
                                        contacts.push(Contact {
                                            resource_name: resource_name.into(),
                                            name: name.into(),
                                            email: email.into(),
                                            photo: person_photo(person),
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
                page = result["nextPageToken"].as_str().unwrap_or_default().into();
                if page.is_empty() {
                    break;
                }
            }
        }
        Ok(contacts)
    }
    pub fn contacts(&mut self, query: &str) -> Result<Vec<Contact>> {
        if !self.auth.contacts() {
            bail!("Enable People API and reconnect with --contacts for address suggestions");
        }
        let token = self.auth.token()?;
        let mut contacts = Vec::new();
        let mut seen = HashSet::new();
        for route in ["people:searchContacts", "otherContacts:search"] {
            let url = format!("{}{route}", self.people_api);
            let mask = if route == "people:searchContacts" {
                "names,emailAddresses,photos"
            } else {
                "names,emailAddresses"
            };
            read(
                &self.client,
                &token,
                &url,
                &[("query", ""), ("readMask", mask)],
            )?;
            let result = read(
                &self.client,
                &token,
                &url,
                &[("query", query), ("readMask", mask), ("pageSize", "12")],
            )?;
            if let Some(rows) = result["results"].as_array() {
                for row in rows {
                    let resource_name = row["person"]["resourceName"].as_str().unwrap_or_default();
                    let name = row["person"]["names"][0]["displayName"]
                        .as_str()
                        .unwrap_or_default();
                    if let Some(emails) = row["person"]["emailAddresses"].as_array() {
                        for email in emails {
                            if let Some(email) = email["value"].as_str() {
                                if seen.insert((resource_name.to_owned(), email.to_lowercase())) {
                                    contacts.push(Contact {
                                        resource_name: resource_name.into(),
                                        name: name.into(),
                                        email: email.into(),
                                        photo: person_photo(&row["person"]),
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok(contacts)
    }
    pub fn images(&mut self, message: &Message, remote: bool) -> Result<Vec<(String, Vec<u8>)>> {
        let mut images = Vec::new();
        for part in message.payload.walk() {
            if images.len() >= 20
                || images
                    .iter()
                    .map(|(_, b): &(String, Vec<u8>)| b.len())
                    .sum::<usize>()
                    >= 40_000_000
            {
                break;
            }
            if !matches!(
                part.mime_type.as_str(),
                "image/png" | "image/jpeg" | "image/gif" | "image/webp"
            ) || part.body.size > 12_000_000
            {
                continue;
            }
            let bytes = if part.body.attachment_id.is_empty() {
                part.decoded()
            } else {
                let result = self.get(
                    &format!(
                        "messages/{}/attachments/{}",
                        segment(&message.id),
                        segment(&part.body.attachment_id)
                    ),
                    &[],
                )?;
                URL_SAFE_NO_PAD.decode(
                    result["data"]
                        .as_str()
                        .unwrap_or_default()
                        .trim_end_matches('='),
                )?
            };
            if !bytes.is_empty() && bytes.len() <= 12_000_000 {
                let cid = part.header("Content-ID");
                let source = if cid.is_empty() {
                    part.filename.clone()
                } else {
                    format!("cid:{}", cid.trim().trim_matches(['<', '>']))
                };
                images.push((source, bytes));
            }
        }
        if remote {
            // No OAuth headers, cookies or redirects on requests to email image hosts.
            for (url, _alt) in message.remote_images() {
                if images.len() >= 20
                    || images.iter().map(|(_, b)| b.len()).sum::<usize>() >= 40_000_000
                {
                    break;
                }
                let parsed = url::Url::parse(&url)?;
                if !parsed.username().is_empty()
                    || parsed.password().is_some()
                    || parsed.port_or_known_default() != Some(443)
                {
                    continue;
                }
                let host = parsed.host_str().context("Image has no host")?;
                use std::net::ToSocketAddrs;
                let resolved: Vec<_> = (host, 443).to_socket_addrs()?.collect();
                if resolved.is_empty() || resolved.iter().any(|a| !public_ip(a.ip())) {
                    continue;
                }
                let client = Client::builder()
                    .timeout(Duration::from_secs(15))
                    .redirect(reqwest::redirect::Policy::none())
                    .resolve_to_addrs(host, &resolved)
                    .no_proxy()
                    .build()?;
                let response = client.get(&url).send()?;
                if !response.status().is_success() {
                    continue;
                }
                let mime = response
                    .headers()
                    .get("content-type")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or_default()
                    .split(';')
                    .next()
                    .unwrap_or_default();
                if !matches!(
                    mime,
                    "image/png" | "image/jpeg" | "image/gif" | "image/webp"
                ) {
                    continue;
                }
                use std::io::Read;
                let mut bytes = Vec::new();
                response.take(12_000_001).read_to_end(&mut bytes)?;
                if bytes.len() <= 12_000_000 {
                    images.push((url, bytes));
                }
            }
        }
        Ok(images)
    }
}

fn public_ip(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v) => {
            !(v.is_private()
                || v.is_loopback()
                || v.is_link_local()
                || v.is_unspecified()
                || v.is_multicast()
                || v.is_broadcast()
                || v.octets()[0] == 0)
        }
        std::net::IpAddr::V6(v) => v
            .to_ipv4_mapped()
            .map(|v| public_ip(v.into()))
            .unwrap_or_else(|| {
                !(v.is_loopback()
                    || v.is_unspecified()
                    || v.is_unique_local()
                    || v.is_unicast_link_local()
                    || v.is_multicast())
            }),
    }
}

fn fetch_text_parts(
    client: &Client,
    token: &str,
    api: &str,
    id: &str,
    part: &mut crate::model::Part,
) -> Result<()> {
    if matches!(part.mime_type.as_str(), "text/plain" | "text/html")
        && part.filename.is_empty()
        && part.body.data.is_empty()
        && !part.body.attachment_id.is_empty()
    {
        if part.body.size > 8_000_000 {
            bail!("Email text exceeds the 8 MB reader limit");
        }
        let value = read(
            client,
            token,
            &format!(
                "{api}messages/{}/attachments/{}",
                segment(id),
                segment(&part.body.attachment_id)
            ),
            &[],
        )?;
        part.body.data = value["data"]
            .as_str()
            .context("Gmail omitted text attachment data")?
            .into();
    }
    for part in &mut part.parts {
        fetch_text_parts(client, token, api, id, part)?;
    }
    Ok(())
}
fn message_body(draft: &Draft, from: &str) -> Result<Value> {
    let mut value = json!({"raw":draft.raw(from)?});
    if !draft.thread_id.is_empty() {
        value["threadId"] = json!(draft.thread_id);
    }
    Ok(value)
}
pub fn segment(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}
fn read(client: &Client, token: &str, url: &str, params: &[(&str, &str)]) -> Result<Value> {
    for attempt in 0..4 {
        let response = client.get(url).bearer_auth(token).query(params).send()?;
        let status = response.status();
        if status.is_success() {
            return Ok(response.json()?);
        }
        if (status.as_u16() == 429 || status.is_server_error()) && attempt < 3 {
            std::thread::sleep(Duration::from_millis(250 * 2_u64.pow(attempt)));
            continue;
        }
        bail!("Google request failed ({status}). Check account permissions and connectivity");
    }
    unreachable!()
}

fn person_photo(person: &Value) -> String {
    person["photos"]
        .as_array()
        .and_then(|photos| {
            photos.iter().find(|photo| {
                !photo["default"].as_bool().unwrap_or(false) && photo["url"].as_str().is_some()
            })
        })
        .and_then(|photo| photo["url"].as_str())
        .unwrap_or_default()
        .into()
}

pub fn contact_photo(url: &str) -> Result<Vec<u8>> {
    let parsed = url::Url::parse(url)?;
    let host = parsed.host_str().context("Photo has no host")?;
    if parsed.scheme() != "https"
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.port_or_known_default() != Some(443)
        || !["googleusercontent.com", "ggpht.com"]
            .iter()
            .any(|domain| host == *domain || host.ends_with(&format!(".{domain}")))
    {
        bail!("Unsupported contact photo URL");
    }
    use std::{io::Read, net::ToSocketAddrs};
    let resolved: Vec<_> = (host, 443).to_socket_addrs()?.collect();
    if resolved.is_empty() || resolved.iter().any(|address| !public_ip(address.ip())) {
        bail!("Non-public photo host");
    }
    let response = Client::builder()
        .timeout(Duration::from_secs(5))
        .redirect(reqwest::redirect::Policy::none())
        .resolve_to_addrs(host, &resolved)
        .no_proxy()
        .build()?
        .get(url)
        .send()?
        .error_for_status()?;
    let mut bytes = Vec::new();
    response.take(2_000_001).read_to_end(&mut bytes)?;
    if bytes.len() > 2_000_000 {
        bail!("Photo is too large");
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::mpsc,
    };

    fn server(
        responses: Vec<(u16, Value)>,
    ) -> (String, mpsc::Receiver<String>, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let (tx, rx) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            for (status, body) in responses {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut bytes = Vec::new();
                let mut byte = [0; 1];
                while !bytes.ends_with(b"\r\n\r\n") {
                    stream.read_exact(&mut byte).unwrap();
                    bytes.push(byte[0]);
                }
                let headers = String::from_utf8(bytes.clone()).unwrap();
                let length = headers
                    .lines()
                    .find_map(|line| {
                        line.to_lowercase()
                            .strip_prefix("content-length: ")
                            .and_then(|n| n.parse::<usize>().ok())
                    })
                    .unwrap_or(0);
                let mut body_bytes = vec![0; length];
                stream.read_exact(&mut body_bytes).unwrap();
                bytes.extend(body_bytes);
                tx.send(String::from_utf8(bytes).unwrap()).unwrap();
                let body = body.to_string();
                write!(stream,"HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
            }
        });
        (url, rx, thread)
    }
    fn client(dir: &Path, api: String) -> Google {
        crate::store::write_private(&dir.join("account.json"),&serde_json::to_vec(&json!({"client_id":"synthetic","client_secret":"synthetic","refresh_token":"synthetic","access_token":"synthetic","expires_at":u64::MAX,"scope":"https://www.googleapis.com/auth/contacts.readonly https://www.googleapis.com/auth/contacts.other.readonly"})).unwrap()).unwrap();
        Google {
            auth: Auth::open(dir).unwrap(),
            client: http().unwrap(),
            people_api: api.clone(),
            api,
        }
    }

    #[test]
    fn thread_fetch_includes_sent_replies_and_orders_messages() {
        let messages = crate::model::demo_thread(&crate::model::demo().1.messages[0]);
        let (api, requests, thread) =
            server(vec![(200, json!({"messages":[messages[1], messages[0]]}))]);
        let dir = tempfile::tempdir_in("/tmp").unwrap();
        let mut google = client(dir.path(), api);
        let store = Store::open(None).unwrap();
        let fetched = google.thread(&store, "thread-0").unwrap();
        thread.join().unwrap();
        assert_eq!(fetched[0].id, "demo-0");
        assert_eq!(fetched[1].id, "demo-reply-0");
        assert!(fetched[1].label_ids.contains(&"SENT".to_string()));
        assert!(
            requests
                .recv()
                .unwrap()
                .starts_with("GET /threads/thread-0?format=full")
        );
        assert_eq!(
            store
                .get::<Vec<Message>>("thread:thread-0")
                .unwrap()
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn contact_search_requests_photos_only_from_supported_endpoint() {
        let person = json!({"resourceName":"people/mira","names":[{"displayName":"Mira"}],"emailAddresses":[{"value":"mira@example.test"},{"value":"work@example.test"}],
            "photos":[{"url":"https://lh3.googleusercontent.com/photo"}]});
        let (api, requests, thread) = server(vec![
            (200, json!({})),
            (200, json!({"results":[{"person":person}]})),
            (200, json!({})),
            (
                200,
                json!({"results":[{"person":{"resourceName":"otherContacts/other","names":[{"displayName":"Mira"}],"emailAddresses":[{"value":"mira@example.test"}]}}]}),
            ),
        ]);
        let dir = tempfile::tempdir_in("/tmp").unwrap();
        let mut google = client(dir.path(), api);
        let contacts = google.contacts("Mira").unwrap();
        thread.join().unwrap();
        assert_eq!(contacts[0].photo, "https://lh3.googleusercontent.com/photo");
        assert_eq!(contacts.len(), 3);
        assert_eq!(contacts[0].resource_name, "people/mira");
        assert_eq!(contacts[1].resource_name, "people/mira");
        assert_eq!(contacts[2].resource_name, "otherContacts/other");
        assert_eq!(crate::contacts::people(&contacts).len(), 2);
        let requests: Vec<_> = requests.try_iter().collect();
        assert!(requests[0].contains("photos"));
        assert!(requests[1].contains("photos"));
        assert!(!requests[2].contains("photos"));
        assert!(!requests[3].contains("photos"));
    }
    #[test]
    fn contact_import_paginates_and_deduplicates_both_sources() {
        let person = json!({"resourceName":"people/mira","names":[{"displayName":"Mira"}],"emailAddresses":[{"value":"mira@example.test"},{"value":"work@example.test"}],"photos":[{"url":"https://lh3.googleusercontent.com/synthetic","default":false}]});
        let (api, requests, thread) = server(vec![
            (
                200,
                json!({"connections":[person.clone()],"nextPageToken":"next-contacts"}),
            ),
            (200, json!({"connections":[person.clone()]})),
            (
                200,
                json!({"otherContacts":[person, {"resourceName":"otherContacts/jun","emailAddresses":[{"value":"jun@example.test"}]}]}),
            ),
        ]);
        let dir = tempfile::tempdir_in("/tmp").unwrap();
        let mut google = client(dir.path(), api);
        let rows = google.all_contacts().unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].resource_name, "people/mira");
        assert_eq!(rows[1].resource_name, "people/mira");
        assert_eq!(rows[2].resource_name, "otherContacts/jun");
        assert_eq!(crate::contacts::people(&rows).len(), 2);
        assert_eq!(rows[0].name, "Mira");
        assert_eq!(rows[0].photo, "https://lh3.googleusercontent.com/synthetic");
        thread.join().unwrap();
        let requests: Vec<_> = requests.try_iter().collect();
        assert!(requests[0].starts_with("GET /people/me/connections?"));
        assert!(requests[0].contains("photos"));
        assert!(requests[1].contains("pageToken=next-contacts"));
        assert!(requests[2].starts_with("GET /otherContacts?"));
    }

    #[test]
    fn search_uses_server_query_and_reuses_cached_messages() {
        let message = crate::model::demo().1.messages.remove(0);
        let listing = json!({"messages":[{"id":message.id}],"nextPageToken":"older"});
        let (api, requests, thread) = server(vec![
            (200, listing.clone()),
            (200, serde_json::to_value(&message).unwrap()),
            (200, listing),
        ]);
        let dir = tempfile::tempdir_in("/tmp").unwrap();
        let mut google = client(dir.path(), api);
        let store = Store::open(None).unwrap();
        let first = google
            .page(&store, "", "from:mira@example.test before:2024/01/01", "")
            .unwrap();
        assert_eq!(first.next, "older");
        assert_eq!(first.messages[0].subject(), message.subject());
        google
            .page(
                &store,
                "",
                "from:mira@example.test before:2024/01/01",
                "older",
            )
            .unwrap();
        thread.join().unwrap();
        let requests: Vec<_> = requests.try_iter().collect();
        assert_eq!(requests.len(), 3);
        assert!(requests[0].starts_with("GET /messages?"));
        assert!(requests[0].contains("q=from%3Amira%40example.test+before%3A2024%2F01%2F01"));
        assert!(!requests[0].contains("labelIds"));
        assert!(requests[2].contains("pageToken=older"));
    }
    #[test]
    fn expired_history_invalidates_cache_before_rebasing_cursor() {
        let (api, _requests, thread) =
            server(vec![(404, json!({})), (200, json!({"historyId":"new"}))]);
        let dir = tempfile::tempdir_in("/tmp").unwrap();
        let mut google = client(dir.path(), api);
        let store = Store::open(None).unwrap();
        store.put("history", &"old").unwrap();
        store
            .save_message(&crate::model::demo().1.messages[0])
            .unwrap();
        google.changes(&store).unwrap();
        thread.join().unwrap();
        assert!(store.message("demo-0").unwrap().is_none());
        assert_eq!(store.get::<String>("history").unwrap().unwrap(), "new");
    }
    #[test]
    fn send_is_single_attempt_and_preserves_thread_headers() {
        let (api, requests, thread) = server(vec![(503, json!({"error":"synthetic failure"}))]);
        let dir = tempfile::tempdir_in("/tmp").unwrap();
        let mut google = client(dir.path(), api);
        let mut draft = Draft::reply(&crate::model::demo().1.messages[0]);
        draft.body = "Synthetic reply".into();
        assert!(google.send(&draft, "you@example.test").is_err());
        thread.join().unwrap();
        let requests: Vec<_> = requests.try_iter().collect();
        assert_eq!(requests.len(), 1);
        let body: Value =
            serde_json::from_str(requests[0].split("\r\n\r\n").nth(1).unwrap()).unwrap();
        assert_eq!(body["threadId"], "thread-0");
        let bytes = URL_SAFE_NO_PAD
            .decode(body["raw"].as_str().unwrap())
            .unwrap();
        let parsed = mailparse::parse_mail(&bytes).unwrap();
        use mailparse::MailHeaderMap;
        assert_eq!(
            parsed.headers.get_first_value("In-Reply-To").unwrap(),
            "<synthetic@example.test>"
        );
    }
    #[test]
    fn fetches_text_stored_as_a_gmail_attachment() {
        let message = crate::model::demo().1.messages.remove(0);
        let expected = message.text();
        let data = message.payload.body.data.clone();
        let mut message = message;
        message.payload.body.data.clear();
        message.payload.body.attachment_id = "text-body".into();
        let (api, requests, thread) = server(vec![
            (200, json!({"messages":[{"id":message.id}]})),
            (200, serde_json::to_value(&message).unwrap()),
            (200, json!({"data":data})),
        ]);
        let dir = tempfile::tempdir_in("/tmp").unwrap();
        let mut google = client(dir.path(), api);
        let store = Store::open(None).unwrap();
        let page = google.page(&store, "INBOX", "", "").unwrap();
        thread.join().unwrap();
        assert_eq!(page.messages[0].text(), expected);
        assert!(
            requests
                .try_iter()
                .last()
                .unwrap()
                .starts_with("GET /messages/demo-0/attachments/text-body")
        );
    }
    #[test]
    fn image_hosts_reject_local_addresses() {
        for ip in [
            "127.0.0.1",
            "10.0.0.1",
            "169.254.169.254",
            "::1",
            "::ffff:127.0.0.1",
            "fc00::1",
        ] {
            assert!(!public_ip(ip.parse().unwrap()));
        }
        assert!(public_ip("8.8.8.8".parse().unwrap()));
    }
}
