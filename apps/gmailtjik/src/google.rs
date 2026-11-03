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
        let query = if label == "INBOX" {
            exclude_snoozed(query)
        } else {
            query.to_owned()
        };
        let mut params = vec![("maxResults", "40"), ("q", query.as_str())];
        if !label.is_empty() {
            params.push(("labelIds", label));
        }
        if label == "TRASH" || label == "SPAM" {
            params.push(("includeSpamTrash", "true"));
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
            threaded: false,
        })
    }
    pub fn thread_page(
        &mut self,
        store: &Store,
        label: &str,
        query: &str,
        page: &str,
    ) -> Result<Page> {
        let query = if label == "INBOX" {
            exclude_snoozed(query)
        } else {
            query.to_owned()
        };
        let mut params = vec![("maxResults", "40"), ("q", query.as_str())];
        if !label.is_empty() {
            params.push(("labelIds", label));
        }
        if label == "TRASH" || label == "SPAM" {
            params.push(("includeSpamTrash", "true"));
        }
        if !page.is_empty() {
            params.push(("pageToken", page));
        }
        let value = self.get("threads", &params)?;
        let ids: Vec<_> = value["threads"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|row| row["id"].as_str())
            .collect();
        let token = self.auth.token()?;
        let mut messages = Vec::new();
        // Fetch complete conversations, including sent replies, with bounded concurrency.
        // Do not reuse thread caches here: a new reply may change the membership.
        for chunk in ids.chunks(6) {
            let threads = std::thread::scope(|scope| {
                let jobs: Vec<_> = chunk
                    .iter()
                    .map(|id| {
                        let client = &self.client;
                        let token = &token;
                        let api = &self.api;
                        scope.spawn(move || -> Result<Vec<Message>> {
                            let value = read(
                                client,
                                token,
                                &format!("{api}threads/{}", segment(id)),
                                &[("format", "full")],
                            )?;
                            let mut messages: Vec<Message> =
                                serde_json::from_value(value["messages"].clone())?;
                            for message in &mut messages {
                                fetch_text_parts(
                                    client,
                                    token,
                                    api,
                                    &message.id,
                                    &mut message.payload,
                                )?;
                            }
                            messages.sort_by_key(|m| m.internal_date.parse::<i64>().unwrap_or(0));
                            Ok(messages)
                        })
                    })
                    .collect();
                jobs.into_iter()
                    .map(|job| {
                        job.join()
                            .map_err(|_| anyhow::anyhow!("Gmail thread fetch worker stopped"))?
                    })
                    .collect::<Result<Vec<_>>>()
            })?;
            for (id, thread) in chunk.iter().zip(threads) {
                for message in &thread {
                    store.save_message(message)?;
                }
                store.put(&format!("thread:{id}"), &thread)?;
                messages.extend(thread);
            }
        }
        Ok(Page {
            messages,
            next: value["nextPageToken"].as_str().unwrap_or_default().into(),
            threaded: true,
        })
    }

    pub fn modify_thread(
        &mut self,
        id: &str,
        add: &[String],
        remove: &[String],
        trash: bool,
    ) -> Result<Vec<String>> {
        let token = self.auth.token()?;
        let action = if trash { "trash" } else { "modify" };
        let mut request = self
            .client
            .post(format!("{}threads/{}/{action}", self.api, segment(id)))
            .bearer_auth(token);
        if !trash {
            request = request.json(&json!({"addLabelIds":add,"removeLabelIds":remove}));
        }
        let response = request.send()?;
        anyhow::ensure!(
            response.status().is_success(),
            "Gmail did not confirm the conversation change ({})",
            response.status()
        );
        let value: Value = response.json()?;
        let messages = value["messages"]
            .as_array()
            .context("Gmail did not return the changed conversation")?;
        messages
            .iter()
            .map(|m| {
                m["id"]
                    .as_str()
                    .map(str::to_owned)
                    .context("Missing changed message ID")
            })
            .collect()
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
    pub fn mark_all_read(&mut self, store: &Store, label: &str, query: &str) -> Result<usize> {
        // Collect all IDs before mutating: changing unread results while paging can skip mail.
        let query = unread_query(query);
        let query = if label == "INBOX" {
            exclude_snoozed(&query)
        } else {
            query
        };
        let mut page = String::new();
        let mut ids = std::collections::BTreeSet::new();
        let mut seen_pages = std::collections::HashSet::new();
        loop {
            let mut params = vec![("maxResults", "500"), ("q", query.as_str())];
            if !label.is_empty() {
                params.push(("labelIds", label));
            }
            if label == "TRASH" || label == "SPAM" {
                params.push(("includeSpamTrash", "true"));
            }
            if !page.is_empty() {
                params.push(("pageToken", page.as_str()));
            }
            let value = self.get("messages", &params)?;
            if let Some(messages) = value["messages"].as_array() {
                for message in messages {
                    ids.insert(
                        message["id"]
                            .as_str()
                            .context("Missing message ID")?
                            .to_owned(),
                    );
                }
            }
            page = value["nextPageToken"].as_str().unwrap_or_default().into();
            if page.is_empty() {
                break;
            }
            if !seen_pages.insert(page.clone()) {
                bail!("Gmail repeated a results page; no mail was changed");
            }
        }
        let ids: Vec<_> = ids.into_iter().collect();
        let mut completed = 0;
        for chunk in ids.chunks(1000) {
            let token = self.auth.token()?;
            // A lost response may still have applied remotely, so discard stale views before sending.
            store.invalidate_mail_views()?;
            let response = self
                .client
                .post(format!("{}messages/batchModify", self.api))
                .bearer_auth(token)
                .json(&json!({"ids": chunk, "removeLabelIds": ["UNREAD"]}))
                .send()
                .with_context(|| {
                    format!("Mark all read interrupted after {completed} confirmed messages")
                })?;
            if !response.status().is_success() {
                bail!(
                    "Gmail did not confirm marking all read ({}); {completed} messages already updated",
                    response.status()
                );
            }
            completed += chunk.len();
        }
        Ok(completed)
    }
    pub fn trash(&mut self, id: &str) -> Result<()> {
        let token = self.auth.token()?;
        let response = self
            .client
            .post(format!("{}messages/{}/trash", self.api, segment(id)))
            // Gmail requires an explicit length for this empty POST body.
            .header(reqwest::header::CONTENT_LENGTH, "0")
            .bearer_auth(token)
            .send()?;
        if !response.status().is_success() {
            bail!(
                "Gmail did not confirm moving the email to Trash ({})",
                response.status()
            );
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

/// Gmail owns the wake-up schedule; its public API exposes search membership,
/// not the snooze-until timestamp. Keep explicit non-Inbox searches unchanged.
pub fn exclude_snoozed(query: &str) -> String {
    if query.trim().is_empty() {
        "-in:snoozed".into()
    } else {
        format!("({query}) -in:snoozed")
    }
}

pub fn unread_query(query: &str) -> String {
    if query.trim().is_empty() {
        "is:unread".into()
    } else {
        format!("({query}) is:unread")
    }
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
    fn thread_pages_include_all_members_and_use_thread_cursors() {
        let fixture = crate::model::demo_conversations().1;
        let first = &fixture.messages[..3];
        let second = &fixture.messages[3..4];
        let (api, requests, thread) = server(vec![
            (
                200,
                json!({"threads":[{"id":"thread-0"}],"nextPageToken":"thread-next"}),
            ),
            (200, json!({"messages":first})),
            (200, json!({"threads":[{"id":"thread-2"}]})),
            (200, json!({"messages":second})),
        ]);
        let dir = tempfile::tempdir_in("/tmp").unwrap();
        let mut google = client(dir.path(), api);
        let store = Store::open(None).unwrap();
        let page = google.thread_page(&store, "INBOX", "", "").unwrap();
        assert!(page.threaded);
        assert_eq!(page.conversations().len(), 1);
        assert_eq!(page.conversations()[0].messages.len(), 3);
        assert_eq!(
            page.conversations()[0].participants("you@example.test"),
            "Mira Chen, me"
        );
        let next = google.thread_page(&store, "INBOX", "", &page.next).unwrap();
        assert_eq!(next.conversations().len(), 1);
        assert_eq!(next.messages[0].thread_id, "thread-2");
        assert!(next.next.is_empty());
        thread.join().unwrap();
        let requests: Vec<_> = requests.try_iter().collect();
        assert!(requests[0].starts_with("GET /threads?"));
        assert!(requests[0].contains("labelIds=INBOX"));
        assert!(requests[0].contains("q=-in%3Asnoozed"));
        assert!(requests[1].starts_with("GET /threads/thread-0?format=full"));
        assert!(requests[2].contains("pageToken=thread-next"));
        assert!(requests[3].starts_with("GET /threads/thread-2?format=full"));
    }

    #[test]
    fn conversation_actions_use_thread_endpoints_and_report_all_members() {
        let (api, requests, thread) = server(vec![
            (
                200,
                json!({"messages":[{"id":"a"},{"id":"b"},{"id":"sent"}]}),
            ),
            (
                200,
                json!({"messages":[{"id":"a"},{"id":"b"},{"id":"sent"}]}),
            ),
            (403, json!({})),
        ]);
        let dir = tempfile::tempdir_in("/tmp").unwrap();
        let mut google = client(dir.path(), api);
        assert_eq!(
            google
                .modify_thread("conversation", &[], &["INBOX".into()], false)
                .unwrap(),
            ["a", "b", "sent"]
        );
        assert_eq!(
            google
                .modify_thread("conversation", &[], &[], true)
                .unwrap()
                .len(),
            3
        );
        assert!(
            google
                .modify_thread("conversation", &[], &["UNREAD".into()], false)
                .is_err()
        );
        thread.join().unwrap();
        let first = requests.recv().unwrap();
        assert!(first.starts_with("POST /threads/conversation/modify "));
        let body: Value = serde_json::from_str(first.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(body["removeLabelIds"], json!(["INBOX"]));
        assert!(
            requests
                .recv()
                .unwrap()
                .starts_with("POST /threads/conversation/trash ")
        );
    }

    #[test]
    fn inbox_excludes_snoozed_but_explicit_search_can_find_it() {
        let (api, requests, thread) = server(vec![(200, json!({})); 4]);
        let dir = tempfile::tempdir_in("/tmp").unwrap();
        let mut google = client(dir.path(), api);
        let store = Store::open(None).unwrap();
        google
            .page(&store, "INBOX", "from:a OR from:b", "next")
            .unwrap();
        google
            .mark_all_read(&store, "INBOX", "from:a OR from:b")
            .unwrap();
        google.page(&store, "", "in:snoozed", "").unwrap();
        google
            .page(&store, "", &exclude_snoozed("is:unread after:123"), "")
            .unwrap();
        thread.join().unwrap();
        for (index, expected) in [
            "(from:a OR from:b) -in:snoozed",
            "((from:a OR from:b) is:unread) -in:snoozed",
            "in:snoozed",
            "(is:unread after:123) -in:snoozed",
        ]
        .iter()
        .enumerate()
        {
            let request = requests.recv().unwrap();
            let route = request
                .lines()
                .next()
                .unwrap()
                .split_whitespace()
                .nth(1)
                .unwrap();
            let url = url::Url::parse(&format!("http://test{route}")).unwrap();
            let params: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
            assert_eq!(&params["q"], expected);
            if index < 2 {
                assert_eq!(params["labelIds"], "INBOX");
            } else {
                assert!(!params.contains_key("labelIds"));
            }
            if index == 0 {
                assert_eq!(params["pageToken"], "next");
            }
        }
    }

    #[test]
    fn mark_all_read_collects_every_page_before_bounded_writes() {
        let first: Vec<_> = (0..500).map(|i| json!({"id": format!("id-{i}")})).collect();
        let second: Vec<_> = (500..1001)
            .map(|i| json!({"id": format!("id-{i}")}))
            .collect();
        let (api, requests, thread) = server(vec![
            (200, json!({"messages": first, "nextPageToken": "next"})),
            (200, json!({"messages": second})),
            (200, json!({})),
            (200, json!({})),
        ]);
        let dir = tempfile::tempdir_in("/tmp").unwrap();
        let mut google = client(dir.path(), api);
        let store = Store::open(None).unwrap();
        assert_eq!(
            google
                .mark_all_read(&store, "Label_AUTH", "from:a OR from:b")
                .unwrap(),
            1001
        );
        for index in 0..2 {
            let request = requests.recv().unwrap();
            assert!(request.starts_with("GET /messages?"));
            let route = request
                .lines()
                .next()
                .unwrap()
                .split_whitespace()
                .nth(1)
                .unwrap();
            let url = url::Url::parse(&format!("http://test{route}")).unwrap();
            let params: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
            assert_eq!(params["labelIds"], "Label_AUTH");
            assert_eq!(params["q"], "(from:a OR from:b) is:unread");
            if index == 1 {
                assert_eq!(params["pageToken"], "next");
            }
        }
        for size in [1000, 1] {
            let request = requests.recv().unwrap();
            assert!(request.starts_with("POST /messages/batchModify "));
            let body: Value =
                serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap();
            assert_eq!(body["ids"].as_array().unwrap().len(), size);
            assert_eq!(body["removeLabelIds"], json!(["UNREAD"]));
            assert!(body.get("addLabelIds").is_none());
        }
        thread.join().unwrap();
    }

    #[test]
    fn mark_all_read_empty_and_failure_are_not_successful_writes() {
        let (api, requests, thread) = server(vec![(200, json!({}))]);
        let dir = tempfile::tempdir_in("/tmp").unwrap();
        assert_eq!(
            client(dir.path(), api)
                .mark_all_read(&Store::open(None).unwrap(), "INBOX", "")
                .unwrap(),
            0
        );
        assert!(requests.recv().unwrap().starts_with("GET "));
        thread.join().unwrap();
        let (api, requests, thread) = server(vec![
            (200, json!({"messages":[{"id":"one"}]})),
            (403, json!({})),
        ]);
        let error = client(dir.path(), api)
            .mark_all_read(&Store::open(None).unwrap(), "INBOX", "")
            .unwrap_err();
        assert!(error.to_string().contains("0 messages already updated"));
        requests.recv().unwrap();
        requests.recv().unwrap();
        thread.join().unwrap();
    }

    #[test]
    fn trash_uses_recoverable_endpoint_and_reports_failure() {
        let (api, requests, thread) = server(vec![(200, json!({})), (403, json!({}))]);
        let dir = tempfile::tempdir_in("/tmp").unwrap();
        let mut google = client(dir.path(), api);
        google.trash("synthetic-message").unwrap();
        assert!(google.trash("synthetic-message").is_err());
        for _ in 0..2 {
            let request = requests.recv().unwrap();
            assert!(request.starts_with("POST /messages/synthetic-message/trash HTTP/1.1"));
            assert!(
                request
                    .to_ascii_lowercase()
                    .contains("\r\ncontent-length: 0\r\n")
            );
            assert!(request.ends_with("\r\n\r\n"));
        }
        thread.join().unwrap();
    }

    #[test]
    fn trash_folder_requests_trashed_messages() {
        let (api, requests, thread) = server(vec![(200, json!({"messages":[]}))]);
        let dir = tempfile::tempdir_in("/tmp").unwrap();
        let mut google = client(dir.path(), api);
        google
            .page(&Store::open(None).unwrap(), "TRASH", "", "")
            .unwrap();
        let request = requests.recv().unwrap();
        assert!(request.contains("includeSpamTrash=true"));
        assert!(request.contains("labelIds=TRASH"));
        thread.join().unwrap();
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
