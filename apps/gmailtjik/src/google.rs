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
            let value = match read(
                &self.client,
                &token,
                &format!("{}history", self.api),
                &[
                    ("startHistoryId", cursor.as_str()),
                    ("pageToken", &page_token),
                ],
            ) {
                Ok(value) => value,
                Err(error)
                    if error
                        .downcast_ref::<GoogleReadError>()
                        .is_some_and(|error| error.status == reqwest::StatusCode::NOT_FOUND) =>
                {
                    store.invalidate_all()?;
                    let profile = self.get("profile", &[])?;
                    break profile["historyId"]
                        .as_str()
                        .context("Missing history cursor")?
                        .to_string();
                }
                Err(error) => return Err(error),
            };
            if let Some(records) = value["history"].as_array() {
                // New messages may not yet be members of any cached view.
                if !records.is_empty() {
                    store
                        .db
                        .execute("UPDATE mail_views SET refreshed_at=0", [])?;
                }
                for record in records {
                    if let Some(messages) = record["messages"].as_array() {
                        for message in messages {
                            if let Some(id) = message["id"].as_str() {
                                store.invalidate(id)?;
                                if let Some(thread) = message["threadId"].as_str() {
                                    store.db.execute("UPDATE mail_thread_freshness SET refreshed_at=0 WHERE id=?", [thread])?;
                                }
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
        let started = chrono::Utc::now().timestamp_millis().max(0) as u64;
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
            let mut results = results;
            store.observe_messages(&mut results, started)?;
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
    #[cfg(test)]
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

    /// Load the first screen without fetching any message bodies or attachments.
    /// Progress snapshots contain complete membership metadata for the threads
    /// loaded so far, in Gmail's list order. Returning false cancels later batches.
    /// These summaries never enter the full-message or full-thread caches.
    pub fn thread_summaries(
        &mut self,
        label: &str,
        query: &str,
        page: &str,
        mut on_progress: impl FnMut(&Page) -> bool,
    ) -> Result<Option<Page>> {
        let query = if label == "INBOX" {
            exclude_snoozed(query)
        } else {
            query.to_owned()
        };
        let mut params = vec![("maxResults", "25"), ("q", query.as_str())];
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
        let mut page = Page {
            messages: Vec::new(),
            next: value["nextPageToken"].as_str().unwrap_or_default().into(),
            threaded: true,
        };
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
                                &[
                                    ("format", "metadata"),
                                    (
                                        "fields",
                                        "messages(id,threadId,labelIds,snippet,internalDate,payload(headers))",
                                    ),
                                ],
                            )?;
                            let mut messages: Vec<Message> =
                                serde_json::from_value(value["messages"].clone())?;
                            for message in &mut messages {
                                message.summary_only = true;
                            }
                            messages.sort_by_key(|m| m.internal_date.parse::<i64>().unwrap_or(0));
                            Ok(messages)
                        })
                    })
                    .collect();
                jobs.into_iter()
                    .map(|job| {
                        job.join().unwrap_or_else(|_| {
                            Err(anyhow::anyhow!("Gmail summary fetch worker stopped"))
                        })
                    })
                    .collect::<Vec<_>>()
            });
            let previous_len = page.messages.len();
            let mut batch_error = None;
            for thread in threads {
                match thread {
                    Ok(messages) => page.messages.extend(messages),
                    Err(error) => {
                        // Keep a throttling failure observable even if another
                        // member of the batch fails for a different reason.
                        if batch_error.as_ref().is_none_or(|previous| {
                            rate_limit_until(&error) > rate_limit_until(previous)
                        }) {
                            batch_error = Some(error);
                        }
                    }
                }
            }
            // Publish successes in Gmail order before reporting a failed peer.
            // Incomplete pages are not returned as completed pagination results.
            let keep_loading = page.messages.len() == previous_len || on_progress(&page);
            if let Some(error) = batch_error {
                return Err(error);
            }
            if !keep_loading {
                return Ok(None);
            }
        }
        Ok(Some(page))
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
        if trash {
            // Google's empty-body POST endpoints require an explicit length.
            request = request.header(reqwest::header::CONTENT_LENGTH, "0");
        } else {
            request = request.json(&json!({"addLabelIds":add,"removeLabelIds":remove}));
        }
        let response = request.send()?;
        if !response.status().is_success() {
            return Err(GoogleReadError::from_response(response).into());
        }
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
        let started = chrono::Utc::now().timestamp_millis().max(0) as u64;
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
        }
        store.observe_messages(&mut messages, started)?;
        for message in &messages {
            store.save_message(message)?;
        }
        messages.sort_by_key(|message| message.internal_date.parse::<i64>().unwrap_or(0));
        store.reconcile_thread(id, &messages)?;
        store.mark_thread_fresh(id)?;
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
            return Err(GoogleReadError::from_response(response).into());
        }
        Ok(())
    }
    #[cfg(test)]
    pub fn mark_all_read(&mut self, store: &Store, label: &str, query: &str) -> Result<usize> {
        self.mark_all_read_ids(label, query, |ids| {
            for id in ids {
                store.apply_labels(id, &[], &["UNREAD".into()])?;
            }
            Ok(())
        })
        .map(|ids| ids.len())
    }

    #[cfg(test)]
    pub fn mark_all_read_ids(
        &mut self,
        label: &str,
        query: &str,
        mut confirmed: impl FnMut(&[String]) -> Result<()>,
    ) -> Result<Vec<String>> {
        let ids = self.unread_ids(label, query)?;
        self.mark_messages_read(&ids, &mut confirmed)?;
        Ok(ids)
    }

    pub fn unread_ids(&mut self, label: &str, query: &str) -> Result<Vec<String>> {
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
        Ok(ids.into_iter().collect())
    }

    pub fn mark_messages_read(
        &mut self,
        ids: &[String],
        mut confirmed: impl FnMut(&[String]) -> Result<()>,
    ) -> Result<()> {
        let mut completed = 0;
        for chunk in ids.chunks(1000) {
            let token = self.auth.token()?;
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
                return Err(
                    anyhow::Error::from(GoogleReadError::from_response(response)).context(format!(
                        "Mark all read interrupted; {completed} messages already updated"
                    )),
                );
            }
            completed += chunk.len();
            confirmed(chunk)?;
        }
        Ok(())
    }
    pub fn empty_folder(&mut self, store: &Store, label: &str) -> Result<usize> {
        let name = match label {
            "SPAM" => "Spam",
            "TRASH" => "Trash",
            _ => bail!("Only Spam and Trash can be emptied"),
        };
        anyhow::ensure!(
            self.auth.can_delete_permanently()?,
            "Empty {name} needs permission to permanently delete mail. Reconnect Gmail in Settings → Sync and grant the requested access, then try again. No messages were deleted."
        );
        let mut completed = 0;
        let mut write_started = false;
        let result = (|| -> Result<usize> {
            // Snapshot every message ID before deleting. Mutating between list
            // pages can skip messages, and a thread may contain mail elsewhere.
            let mut page = String::new();
            let mut ids = std::collections::BTreeSet::new();
            let mut seen_pages = HashSet::new();
            loop {
                let mut params = vec![
                    ("maxResults", "500"),
                    ("labelIds", label),
                    ("includeSpamTrash", "true"),
                ];
                if !page.is_empty() {
                    params.push(("pageToken", page.as_str()));
                }
                let value = self.get("messages", &params)?;
                if let Some(messages) = value.get("messages") {
                    for message in messages.as_array().context("Invalid Gmail message list")? {
                        let id = message["id"]
                            .as_str()
                            .filter(|id| !id.is_empty())
                            .context("Missing message ID")?;
                        ids.insert(id.to_owned());
                    }
                }
                page = match value.get("nextPageToken") {
                    Some(value) => value.as_str().context("Invalid Gmail page token")?.into(),
                    None => String::new(),
                };
                if page.is_empty() {
                    break;
                }
                anyhow::ensure!(
                    seen_pages.insert(page.clone()),
                    "Gmail repeated a results page; no mail was changed"
                );
            }
            let ids: Vec<_> = ids.into_iter().collect();
            for chunk in ids.chunks(1000) {
                let token = self.auth.token()?;
                // Never retry permanent writes: a lost response may still mean
                // Gmail applied the batch. Only confirmed IDs leave the cache.
                write_started = true;
                let response = self
                    .client
                    .post(format!("{}messages/batchDelete", self.api))
                    .bearer_auth(token)
                    .json(&json!({"ids": chunk}))
                    .send()
                    .context(
                        "Deletion outcome is uncertain; refresh this folder before retrying",
                    )?;
                if !response.status().is_success() {
                    return Err(GoogleReadError::from_response(response).into());
                }
                completed += chunk.len();
                store.remove_messages(chunk)?;
            }
            Ok(completed)
        })();
        result.map_err(|error| {
            let error = if write_started { error } else { error.context(BeforeWrite) };
            let detail = format!(
                "Empty {name} interrupted; {completed} messages confirmed permanently deleted. {error}"
            );
            error.context(detail)
        })
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
            return Err(GoogleReadError::from_response(response).into());
        }
        Ok(())
    }
    pub fn save_draft(&mut self, draft: &Draft, from: &str) -> Result<String> {
        let token = self.auth.token().context(BeforeWrite)?;
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
            return Err(GoogleReadError::from_response(response).into());
        }
        Ok(response.json::<Value>()?["id"]
            .as_str()
            .context("Missing draft ID")?
            .into())
    }
    pub fn send(&mut self, draft: &Draft, from: &str) -> Result<()> {
        let token = self.auth.token().context(BeforeWrite)?;
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
            return Err(
                anyhow::Error::from(GoogleReadError::from_response(response))
                    .context("Gmail did not confirm sending. Check Sent before retrying"),
            );
        }
        Ok(())
    }
    pub fn filters(&mut self) -> Result<Vec<crate::filters::Filter>> {
        let value = self.get("settings/filters", &[])?;
        Ok(serde_json::from_value(
            value.get("filter").cloned().unwrap_or_else(|| json!([])),
        )?)
    }
    pub fn create_filter(
        &mut self,
        filter: &crate::filters::Filter,
    ) -> Result<crate::filters::Filter> {
        filter.validate(false)?;
        let token = self.auth.token()?;
        let response = self
            .client
            .post(format!("{}settings/filters", self.api))
            .bearer_auth(token)
            .json(&json!({"criteria":filter.criteria,"action":filter.action}))
            .send()?;
        if !response.status().is_success() {
            return Err(GoogleReadError::from_response(response).into());
        }
        Ok(response.json()?)
    }
    pub fn delete_filter(&mut self, id: &str) -> Result<()> {
        anyhow::ensure!(!id.is_empty(), "Missing filter ID");
        let token = self.auth.token()?;
        let response = self
            .client
            .delete(format!("{}settings/filters/{}", self.api, segment(id)))
            .bearer_auth(token)
            .send()?;
        if !response.status().is_success() {
            return Err(GoogleReadError::from_response(response).into());
        }
        Ok(())
    }
    pub fn apply_filter(
        &mut self,
        store: &Store,
        filter: &crate::filters::Filter,
    ) -> Result<usize> {
        filter.validate(true)?;
        let query = filter.query()?;
        let mut ids = std::collections::BTreeSet::new();
        let mut page = String::new();
        let mut seen = HashSet::new();
        loop {
            let value = self.get(
                "messages",
                &[
                    ("q", &query),
                    ("maxResults", "500"),
                    ("includeSpamTrash", "true"),
                    ("pageToken", &page),
                ],
            )?;
            for row in value["messages"].as_array().into_iter().flatten() {
                ids.insert(
                    row["id"]
                        .as_str()
                        .context("Missing matching message ID")?
                        .to_string(),
                );
            }
            page = value["nextPageToken"].as_str().unwrap_or_default().into();
            if page.is_empty() {
                break;
            }
            anyhow::ensure!(
                seen.insert(page.clone()),
                "Gmail repeated a results page; no mail was changed"
            );
        }
        let ids: Vec<_> = ids.into_iter().collect();
        let add = filter.label_ids("addLabelIds");
        let remove = filter.label_ids("removeLabelIds");
        let mut completed = 0;
        for chunk in ids.chunks(1000) {
            let result = (|| -> Result<()> {
                let token = self.auth.token()?;
                let response = self
                    .client
                    .post(format!("{}messages/batchModify", self.api))
                    .bearer_auth(token)
                    .json(&json!({"ids":chunk,"addLabelIds":add,"removeLabelIds":remove}))
                    .send()?;
                if !response.status().is_success() {
                    return Err(GoogleReadError::from_response(response).into());
                }
                completed += chunk.len();
                store.invalidate_page_freshness()?;
                for id in chunk {
                    store.apply_labels(id, &add, &remove)?;
                }
                Ok(())
            })();
            result.with_context(|| format!("Apply interrupted after {completed} confirmed messages; retry Apply existing mail"))?;
        }
        store.invalidate_page_freshness()?;
        Ok(completed)
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GoogleFailure {
    RateLimit,
    Permissions,
    DisabledApi,
    Policy,
    Quota,
    Unknown,
}

#[derive(Debug)]
struct GoogleReadError {
    status: reqwest::StatusCode,
    kind: GoogleFailure,
    retry_after_until: u64,
}

impl GoogleReadError {
    fn from_response(response: reqwest::blocking::Response) -> Self {
        let status = response.status();
        let retry_after_until = retry_after_until(
            response
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|value| value.to_str().ok()),
            crate::auth::now(),
        );
        let mut kind = GoogleFailure::Unknown;
        if status == reqwest::StatusCode::FORBIDDEN {
            // Inspect only bounded structured reasons. Google's free-form messages
            // can contain account or project details and must never reach the UI/log.
            let value: Value = serde_json::from_reader(std::io::Read::take(response, 65_536))
                .unwrap_or(Value::Null);
            let error = &value["error"];
            let reasons = error["errors"]
                .as_array()
                .into_iter()
                .flatten()
                .chain(error["details"].as_array().into_iter().flatten())
                .filter_map(|entry| entry["reason"].as_str());
            for reason in reasons {
                let recognized = match reason {
                    "rateLimitExceeded" | "userRateLimitExceeded" | "RATE_LIMIT_EXCEEDED" => {
                        GoogleFailure::RateLimit
                    }
                    "insufficientPermissions" | "ACCESS_TOKEN_SCOPE_INSUFFICIENT" => {
                        GoogleFailure::Permissions
                    }
                    "accessNotConfigured" | "SERVICE_DISABLED" | "API_DISABLED" => {
                        GoogleFailure::DisabledApi
                    }
                    "domainPolicy" | "ORG_RESTRICTION_VIOLATION" => GoogleFailure::Policy,
                    "dailyLimitExceeded" | "quotaExceeded" | "QUOTA_EXCEEDED" => {
                        GoogleFailure::Quota
                    }
                    _ => GoogleFailure::Unknown,
                };
                if recognized != GoogleFailure::Unknown {
                    kind = recognized;
                    // A permanent denial takes precedence over any rate-limit detail.
                    if kind != GoogleFailure::RateLimit {
                        break;
                    }
                }
            }
        }
        Self {
            status,
            kind,
            retry_after_until,
        }
    }

    fn retryable(&self) -> bool {
        self.status.is_server_error()
    }
}

fn retry_after_until(value: Option<&str>, now: u64) -> u64 {
    value
        .and_then(|value| {
            let value = value.trim();
            value
                .parse::<u64>()
                .ok()
                .map(|seconds| now.saturating_add(seconds))
                .or_else(|| {
                    chrono::DateTime::parse_from_rfc2822(value)
                        .ok()
                        .and_then(|date| u64::try_from(date.timestamp()).ok())
                        .map(|timestamp| timestamp.max(now))
                })
        })
        .unwrap_or_else(|| now.saturating_add(60))
}

/// Account workers persist this deadline to share Google's cooldown across
/// folder reads, background sync and separate GUI/service processes.
pub fn rate_limit_until(error: &anyhow::Error) -> Option<u64> {
    let error = error.downcast_ref::<GoogleReadError>()?;
    (error.status == reqwest::StatusCode::TOO_MANY_REQUESTS
        || error.kind == GoogleFailure::RateLimit)
        .then_some(error.retry_after_until)
}

/// Only delivery of idempotent mailbox actions may use this retry policy.
/// Sending, creating drafts and permanent deletion never use automatic replay.
pub fn transient_failure(error: &anyhow::Error) -> bool {
    if let Some(error) = error.downcast_ref::<GoogleReadError>() {
        return error.retryable()
            || error.status == reqwest::StatusCode::TOO_MANY_REQUESTS
            || error.kind == GoogleFailure::RateLimit;
    }
    error
        .downcast_ref::<reqwest::Error>()
        .is_some_and(|error| error.is_timeout() || error.is_connect() || error.is_request())
}

#[derive(Debug)]
struct BeforeWrite;
impl std::fmt::Display for BeforeWrite {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("No mailbox write was started")
    }
}

pub fn before_write(error: &anyhow::Error) -> bool {
    error.downcast_ref::<BeforeWrite>().is_some()
}

impl std::fmt::Display for GoogleReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let detail = if self.status == reqwest::StatusCode::UNAUTHORIZED {
            "Google authorization is no longer valid. Reconnect Gmail."
        } else if self.status == reqwest::StatusCode::TOO_MANY_REQUESTS
            || self.kind == GoogleFailure::RateLimit
        {
            "Google is temporarily limiting requests. Wait a moment and try again."
        } else if self.status.is_server_error() {
            "Google is temporarily unavailable. Try again shortly."
        } else {
            match self.kind {
                GoogleFailure::Permissions => {
                    "Required Google permissions are missing. Reconnect Gmail and grant the requested access."
                }
                GoogleFailure::DisabledApi => {
                    "The requested Google API is disabled. Enable it in the OAuth client's Google Cloud project."
                }
                GoogleFailure::Policy => {
                    "Your Google Workspace policy blocks this request. Contact your administrator."
                }
                GoogleFailure::Quota => {
                    "The Google API quota is exhausted. Check the project's quota or wait for it to reset."
                }
                _ if self.status == reqwest::StatusCode::FORBIDDEN => {
                    "Google denied access. Check account permissions and Google API settings."
                }
                _ => "Check account permissions and connectivity.",
            }
        };
        write!(f, "Google request failed ({}). {detail}", self.status)
    }
}

impl std::error::Error for GoogleReadError {}

fn read(client: &Client, token: &str, url: &str, params: &[(&str, &str)]) -> Result<Value> {
    for attempt in 0..4 {
        let response = client.get(url).bearer_auth(token).query(params).send()?;
        let status = response.status();
        if status.is_success() {
            return Ok(response.json()?);
        }
        let error = GoogleReadError::from_response(response);
        if error.retryable() && attempt < 3 {
            std::thread::sleep(Duration::from_secs(2_u64.pow(attempt)));
            continue;
        }
        return Err(error.into());
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
        || !(host == "www.gravatar.com"
            || ["googleusercontent.com", "ggpht.com"]
                .iter()
                .any(|domain| host == *domain || host.ends_with(&format!(".{domain}"))))
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
        server_with_headers(
            responses
                .into_iter()
                .map(|(status, body)| (status, body, String::new()))
                .collect(),
        )
    }

    fn server_with_headers(
        responses: Vec<(u16, Value, String)>,
    ) -> (String, mpsc::Receiver<String>, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let (tx, rx) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            for (status, body, extra_headers) in responses {
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
                write!(stream,"HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{extra_headers}Connection: close\r\n\r\n{body}",body.len()).unwrap();
            }
        });
        (url, rx, thread)
    }
    fn client(dir: &Path, api: String) -> Google {
        scoped_client(
            dir,
            api,
            "https://www.googleapis.com/auth/contacts.readonly https://www.googleapis.com/auth/contacts.other.readonly",
        )
    }
    fn scoped_client(dir: &Path, api: String, scope: &str) -> Google {
        crate::store::write_private(&dir.join("account.json"),&serde_json::to_vec(&json!({"client_id":"synthetic","client_secret":"synthetic","refresh_token":"synthetic","access_token":"synthetic","expires_at":u64::MAX,"scope":scope})).unwrap()).unwrap();
        Google {
            auth: Auth::open(dir).unwrap(),
            client: http().unwrap(),
            people_api: api.clone(),
            api,
        }
    }

    #[test]
    fn filters_use_server_endpoints_and_freeze_matches_before_batching() {
        let dir = tempfile::tempdir_in("/tmp").unwrap();
        let filter = crate::filters::Filter {
            id: "old".into(),
            criteria: json!({"from":"sender@example.test"}),
            action: json!({"removeLabelIds":["UNREAD"]}),
        };
        let messages: Vec<_> = (0..1001).map(|i| json!({"id":format!("m{i}")})).collect();
        let (api, requests, thread) = server(vec![
            (200, json!({"filter":[]})),
            (200, serde_json::to_value(&filter).unwrap()),
            (200, json!({})),
            (
                200,
                json!({"messages":messages[..500],"nextPageToken":"next"}),
            ),
            (200, json!({"messages":messages[500..]})),
            (200, json!({})),
            (200, json!({})),
        ]);
        let mut google = client(dir.path(), api);
        let store = Store::open(None).unwrap();
        assert!(google.filters().unwrap().is_empty());
        google.create_filter(&filter).unwrap();
        google.delete_filter("old").unwrap();
        assert_eq!(google.apply_filter(&store, &filter).unwrap(), 1001);
        thread.join().unwrap();
        let requests: Vec<_> = requests.try_iter().collect();
        assert!(requests[0].starts_with("GET /settings/filters"));
        let body: Value =
            serde_json::from_str(requests[1].split("\r\n\r\n").nth(1).unwrap()).unwrap();
        assert!(body.get("id").is_none());
        assert!(requests[2].starts_with("DELETE /settings/filters/old"));
        assert!(requests[4].contains("pageToken=next"));
        for (request, count) in requests[5..].iter().zip([1000, 1]) {
            assert!(request.starts_with("POST /messages/batchModify"));
            let body: Value =
                serde_json::from_str(request.split("\r\n\r\n").nth(1).unwrap()).unwrap();
            assert_eq!(body["ids"].as_array().unwrap().len(), count);
            assert_eq!(body["removeLabelIds"], json!(["UNREAD"]));
        }
    }

    #[test]
    fn repeated_filter_page_aborts_before_any_mutation() {
        let dir = tempfile::tempdir_in("/tmp").unwrap();
        let (api, requests, thread) = server(vec![
            (
                200,
                json!({"messages":[{"id":"m1"}],"nextPageToken":"same"}),
            ),
            (
                200,
                json!({"messages":[{"id":"m2"}],"nextPageToken":"same"}),
            ),
        ]);
        let mut google = client(dir.path(), api);
        let filter = crate::filters::Filter {
            criteria: json!({"from":"sender@example.test"}),
            action: json!({"removeLabelIds":["UNREAD"]}),
            ..Default::default()
        };
        assert!(
            google
                .apply_filter(&Store::open(None).unwrap(), &filter)
                .unwrap_err()
                .to_string()
                .contains("no mail was changed")
        );
        thread.join().unwrap();
        assert!(requests.try_iter().all(|r| r.starts_with("GET /messages?")));
    }

    fn summary_server(
        thread_count: usize,
        fetched_threads: usize,
    ) -> (String, mpsc::Receiver<String>, std::thread::JoinHandle<()>) {
        summary_failure_server(thread_count, fetched_threads, Default::default())
    }

    fn summary_failure_server(
        thread_count: usize,
        fetched_threads: usize,
        failures: std::collections::HashMap<String, (u16, Value, String)>,
    ) -> (String, mpsc::Receiver<String>, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let (tx, rx) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            for _ in 0..=fetched_threads {
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
                let request = String::from_utf8(bytes).unwrap();
                let target = request.split_whitespace().nth(1).unwrap();
                let route = url::Url::parse(&format!("http://fixture{target}")).unwrap();
                let body = if route.path() == "/threads" {
                    json!({
                        "threads": (0..thread_count).map(|i| json!({"id":format!("t{i}")})).collect::<Vec<_>>(),
                        "nextPageToken":"next-25",
                    })
                } else {
                    let id = route
                        .path()
                        .strip_prefix("/threads/")
                        .expect("metadata must not fetch attachments");
                    assert!(!id.contains('/'));
                    let mut received = crate::model::demo().1.messages.remove(0);
                    received.id = format!("{id}-received");
                    received.thread_id = id.to_string();
                    received.internal_date = "100".into();
                    received.label_ids = vec!["INBOX".into(), "UNREAD".into()];
                    // A body reference, even if returned by an over-broad server,
                    // must not make the list fetch its attachment.
                    received.payload.body.data.clear();
                    received.payload.body.attachment_id = "must-not-fetch".into();
                    let mut sent = received.clone();
                    sent.id = format!("{id}-sent");
                    sent.internal_date = "200".into();
                    sent.label_ids = vec!["SENT".into()];
                    sent.payload.headers = vec![
                        crate::model::Header {
                            name: "From".into(),
                            value: "You <you@example.test>".into(),
                        },
                        crate::model::Header {
                            name: "Subject".into(),
                            value: "Reply to the sample".into(),
                        },
                    ];
                    // Deliberately reverse chronology within each conversation.
                    json!({"messages":[sent,received]})
                };
                let (status, body, extra_headers) = failures
                    .get(route.path())
                    .cloned()
                    .unwrap_or((200, body, String::new()));
                let body = body.to_string();
                tx.send(request).unwrap();
                write!(stream,"HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{extra_headers}Connection: close\r\n\r\n{body}",body.len()).unwrap();
            }
        });
        (url, rx, thread)
    }

    #[test]
    fn summaries_render_25_threads_progressively_without_fetching_bodies() {
        let (api, requests, server) = summary_server(25, 25);
        let dir = tempfile::tempdir_in("/tmp").unwrap();
        let mut google = client(dir.path(), api);
        let mut counts = Vec::new();
        let mut observed = Vec::new();
        let page = google
            .thread_summaries("INBOX", "", "", |page| {
                observed.extend(requests.try_iter());
                let count = page.conversations().len();
                counts.push(count);
                // No later batch has been requested before these rows are exposed.
                assert_eq!(observed.len(), count + 1);
                assert_eq!(page.messages.len(), count * 2);
                true
            })
            .unwrap()
            .unwrap();
        server.join().unwrap();
        assert_eq!(counts, [6, 12, 18, 24, 25]);
        assert_eq!(page.next, "next-25");
        assert!(page.threaded);
        assert!(page.messages.iter().all(|message| message.summary_only));
        for (index, row) in page.conversations().iter().enumerate() {
            assert_eq!(row.message().thread_id, format!("t{index}"));
            assert_eq!(row.messages.len(), 2);
            assert!(row.unread());
            assert_eq!(row.latest().id, format!("t{index}-sent"));
            assert_eq!(row.participants("you@example.test"), "Mira Chen, me");
        }
        let first = &observed[0];
        assert!(first.starts_with("GET /threads?"));
        assert!(first.contains("maxResults=25"));
        assert!(first.contains("labelIds=INBOX"));
        assert!(first.contains("q=-in%3Asnoozed"));
        for request in &observed[1..] {
            assert!(request.contains("format=metadata"));
            let target = request.split_whitespace().nth(1).unwrap();
            let url = url::Url::parse(&format!("http://fixture{target}")).unwrap();
            let fields = url
                .query_pairs()
                .find(|(key, _)| key == "fields")
                .unwrap()
                .1;
            assert_eq!(
                fields,
                "messages(id,threadId,labelIds,snippet,internalDate,payload(headers))"
            );
        }
    }

    #[test]
    fn obsolete_summary_load_stops_before_the_next_batch() {
        let (api, requests, server) = summary_server(25, 6);
        let dir = tempfile::tempdir_in("/tmp").unwrap();
        let mut google = client(dir.path(), api);
        let page = google
            .thread_summaries("TRASH", "from:sample", "older", |page| {
                assert_eq!(page.conversations().len(), 6);
                false
            })
            .unwrap();
        assert!(page.is_none());
        server.join().unwrap();
        let requests: Vec<_> = requests.try_iter().collect();
        assert_eq!(requests.len(), 7);
        assert!(requests[0].contains("maxResults=25"));
        assert!(requests[0].contains("labelIds=TRASH"));
        assert!(requests[0].contains("includeSpamTrash=true"));
        assert!(requests[0].contains("q=from%3Asample"));
        assert!(requests[0].contains("pageToken=older"));
    }

    #[test]
    fn summary_batch_keeps_successes_and_reports_throttling_without_retry_fanout() {
        let failures = [
            (
                "/threads/t1".into(),
                (
                    403,
                    json!({"error":{"errors":[{"reason":"rateLimitExceeded"}]}}),
                    String::new(),
                ),
            ),
            (
                "/threads/t3".into(),
                (
                    403,
                    json!({"error":{"errors":[{"reason":"insufficientPermissions"}]}}),
                    String::new(),
                ),
            ),
            (
                "/threads/t5".into(),
                (429, json!({}), "Retry-After: 180\r\n".into()),
            ),
        ]
        .into_iter()
        .collect();
        let (api, requests, server) = summary_failure_server(25, 6, failures);
        let dir = tempfile::tempdir_in("/tmp").unwrap();
        let mut google = client(dir.path(), api);
        let before = crate::auth::now();
        let mut progress = Vec::new();
        let error = google
            .thread_summaries("INBOX", "", "", |page| {
                progress.push(page.clone());
                // Cancellation does not swallow the shared throttling deadline.
                false
            })
            .unwrap_err();
        server.join().unwrap();
        assert_eq!(requests.try_iter().count(), 7);
        assert_eq!(progress.len(), 1);
        let ids: Vec<_> = progress[0]
            .conversations()
            .iter()
            .map(|row| row.message().thread_id.clone())
            .collect();
        assert_eq!(ids, ["t0", "t2", "t4"]);
        assert!(
            progress[0]
                .messages
                .iter()
                .all(|message| message.summary_only)
        );
        let deadline =
            rate_limit_until(&error).expect("a permanent peer error must not hide throttling");
        assert!((before + 180..=crate::auth::now() + 180).contains(&deadline));
    }

    #[test]
    fn empty_summary_page_preserves_cursor_without_fetching_threads() {
        let (api, requests, server) = summary_server(0, 0);
        let dir = tempfile::tempdir_in("/tmp").unwrap();
        let mut google = client(dir.path(), api);
        let page = google
            .thread_summaries("SPAM", "", "", |_| {
                panic!("An empty page has no progress batch")
            })
            .unwrap()
            .unwrap();
        assert!(page.messages.is_empty() && page.threaded);
        assert_eq!(page.next, "next-25");
        server.join().unwrap();
        let requests: Vec<_> = requests.try_iter().collect();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].contains("includeSpamTrash=true"));
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
    fn all_mail_lists_threads_without_a_label_filter() {
        let (api, requests, thread) = server(vec![(200, json!({"threads":[]}))]);
        let dir = tempfile::tempdir_in("/tmp").unwrap();
        let mut google = client(dir.path(), api);
        google.thread_summaries("", "", "", |_| true).unwrap();
        thread.join().unwrap();
        let request = requests.recv().unwrap();
        assert!(request.starts_with("GET /threads?"));
        assert!(!request.contains("labelIds"));
        assert!(!request.contains("includeSpamTrash"));
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
        let trash = requests.recv().unwrap();
        assert!(trash.starts_with("POST /threads/conversation/trash "));
        assert!(
            trash
                .to_ascii_lowercase()
                .contains("\r\ncontent-length: 0\r\n")
        );
        assert!(trash.ends_with("\r\n\r\n"));
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
    fn queued_mark_all_read_retries_only_the_frozen_unconfirmed_ids() {
        use crate::actions::Action;
        let ids: Vec<_> = (0..1001)
            .map(|n| json!({"id":format!("frozen-{n:04}")}))
            .collect();
        let (api, requests, server) = server(vec![
            (200, json!({"messages":ids})),
            (200, json!({})),
            (503, json!({})),
            (200, json!({})),
        ]);
        let dir = tempfile::tempdir_in("/tmp").unwrap();
        let mut google = client(dir.path(), api);
        let store = Store::open(None).unwrap();
        let id = store
            .enqueue_action(
                Action::MarkAllRead {
                    label: "INBOX".into(),
                    query: String::new(),
                },
                &[],
            )
            .unwrap();
        let first = store.claim_action(1).unwrap().unwrap();
        assert!(
            crate::worker::execute_action(&first, &mut google, "you@example.test", &store).is_err()
        );
        let partial = store.action(id).unwrap().unwrap();
        assert_eq!(partial.target_ids.as_ref().unwrap().len(), 1001);
        assert_eq!(partial.confirmed_ids.len(), 1000);
        store
            .defer_action(id, 2, "Synthetic interrupted batch")
            .unwrap();
        let second = store.claim_action(2).unwrap().unwrap();
        let (confirmed, result) =
            crate::worker::execute_action(&second, &mut google, "you@example.test", &store)
                .unwrap();
        assert_eq!(confirmed.len(), 1001);
        assert_eq!(result.count, 1001);
        server.join().unwrap();
        assert!(requests.recv().unwrap().starts_with("GET /messages?"));
        let writes: Vec<_> = requests.try_iter().collect();
        assert_eq!(writes.len(), 3);
        for (request, size) in writes.iter().zip([1000, 1, 1]) {
            assert!(request.starts_with("POST /messages/batchModify "));
            let body: Value =
                serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap();
            assert_eq!(body["ids"].as_array().unwrap().len(), size);
        }
    }

    #[test]
    fn empty_folder_requires_exact_permission_and_a_disposable_folder() {
        let dir = tempfile::tempdir_in("/tmp").unwrap();
        let store = Store::open(None).unwrap();
        for scope in [
            "",
            "https://www.googleapis.com/auth/gmail.modify",
            "https://mail.google.com/other",
        ] {
            let mut google = scoped_client(dir.path(), "http://127.0.0.1:9/".into(), scope);
            let error = google.empty_folder(&store, "SPAM").unwrap_err();
            assert!(error.to_string().contains("Reconnect Gmail"));
            assert!(error.to_string().contains("No messages were deleted"));
        }
        let mut google = scoped_client(
            dir.path(),
            "http://127.0.0.1:9/".into(),
            "https://mail.google.com/",
        );
        for label in ["", "INBOX", "Label_Spam", "spam"] {
            assert_eq!(
                google.empty_folder(&store, label).unwrap_err().to_string(),
                "Only Spam and Trash can be emptied"
            );
        }
    }

    #[test]
    fn empty_folder_listing_failure_is_safe_to_defer_before_any_write() {
        let (api, requests, thread) = server(vec![(429, json!({}))]);
        let dir = tempfile::tempdir_in("/tmp").unwrap();
        let error = scoped_client(dir.path(), api, "https://mail.google.com/")
            .empty_folder(&Store::open(None).unwrap(), "TRASH")
            .unwrap_err();
        assert!(before_write(&error));
        assert!(transient_failure(&error));
        assert!(rate_limit_until(&error).is_some());
        assert!(requests.recv().unwrap().starts_with("GET /messages?"));
        thread.join().unwrap();
        assert!(requests.try_recv().is_err());
    }

    #[test]
    fn empty_folder_collects_all_pages_then_deletes_bounded_deduplicated_messages() {
        let entries = |range: std::ops::Range<usize>| {
            range
                .map(|i| json!({"id":format!("id-{i:04}")}))
                .collect::<Vec<_>>()
        };
        let (api, requests, thread) = server(vec![
            (
                200,
                json!({"messages":entries(0..500), "nextPageToken":"second"}),
            ),
            (
                200,
                json!({"messages":entries(499..999), "nextPageToken":"third"}),
            ),
            (200, json!({"messages":entries(999..1001)})),
            (204, json!({})),
            (204, json!({})),
        ]);
        let dir = tempfile::tempdir_in("/tmp").unwrap();
        let mut google = scoped_client(dir.path(), api, "https://mail.google.com/");
        let store = Store::open(None).unwrap();
        let mut deleted = crate::model::demo().1.messages[0].clone();
        deleted.id = "id-0000".into();
        store.save_message(&deleted).unwrap();
        let mut retained = deleted.clone();
        retained.id = "outside-folder".into();
        store.save_message(&retained).unwrap();
        assert_eq!(google.empty_folder(&store, "TRASH").unwrap(), 1001);
        thread.join().unwrap();
        let requests: Vec<_> = requests.try_iter().collect();
        assert_eq!(requests.len(), 5);
        for (index, request) in requests[..3].iter().enumerate() {
            let target = request.split_whitespace().nth(1).unwrap();
            let url = url::Url::parse(&format!("http://test{target}")).unwrap();
            let params: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
            assert!(request.starts_with("GET /messages?"));
            assert_eq!(params["labelIds"], "TRASH");
            assert_eq!(params["includeSpamTrash"], "true");
            assert_eq!(params["maxResults"], "500");
            assert!(!params.contains_key("q"));
            assert_eq!(
                params.get("pageToken").map(String::as_str),
                [None, Some("second"), Some("third")][index]
            );
        }
        let mut deleted_ids = HashSet::new();
        for (request, count) in requests[3..].iter().zip([1000, 1]) {
            assert!(request.starts_with("POST /messages/batchDelete "));
            let body: Value =
                serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap();
            assert_eq!(body["ids"].as_array().unwrap().len(), count);
            for id in body["ids"].as_array().unwrap() {
                assert!(deleted_ids.insert(id.as_str().unwrap().to_owned()));
            }
        }
        assert_eq!(deleted_ids.len(), 1001);
        assert!(store.message("id-0000").unwrap().is_none());
        assert!(store.message("outside-folder").unwrap().is_some());
    }

    #[test]
    fn empty_folder_keeps_confirmed_progress_and_typed_cooldown_on_partial_failure() {
        let entries = |range: std::ops::Range<usize>| {
            range
                .map(|i| json!({"id":format!("id-{i:04}")}))
                .collect::<Vec<_>>()
        };
        let (api, requests, thread) = server_with_headers(vec![
            (
                200,
                json!({"messages":entries(0..500), "nextPageToken":"second"}),
                String::new(),
            ),
            (
                200,
                json!({"messages":entries(500..1000), "nextPageToken":"third"}),
                String::new(),
            ),
            (200, json!({"messages":entries(1000..1001)}), String::new()),
            (204, json!({}), String::new()),
            (
                429,
                json!({"error": {"errors":[{"reason":"rateLimitExceeded"}]}}),
                "Retry-After: 120\r\n".into(),
            ),
        ]);
        let dir = tempfile::tempdir_in("/tmp").unwrap();
        let mut google = scoped_client(dir.path(), api, "https://mail.google.com/");
        let store = Store::open(None).unwrap();
        for id in ["id-0000", "id-1000"] {
            let mut message = crate::model::demo().1.messages[0].clone();
            message.id = id.into();
            store.save_message(&message).unwrap();
        }
        let before = crate::auth::now();
        let error = google.empty_folder(&store, "SPAM").unwrap_err();
        assert!(
            error
                .to_string()
                .contains("1000 messages confirmed permanently deleted")
        );
        assert!(error.to_string().contains("limiting requests"));
        assert!(rate_limit_until(&error).unwrap() >= before + 120);
        assert!(store.message("id-0000").unwrap().is_none());
        assert!(store.message("id-1000").unwrap().is_some());
        thread.join().unwrap();
        assert_eq!(requests.try_iter().count(), 5);
    }

    #[test]
    fn empty_folder_does_not_write_when_listing_is_empty_or_invalid() {
        let dir = tempfile::tempdir_in("/tmp").unwrap();
        let store = Store::open(None).unwrap();
        let (api, requests, thread) = server(vec![(200, json!({}))]);
        // Upgrade persisted permissions without replacing the open Google instance.
        let mut google = client(dir.path(), api.clone());
        let _upgraded = scoped_client(dir.path(), api, "https://mail.google.com/");
        assert_eq!(google.empty_folder(&store, "SPAM").unwrap(), 0);
        thread.join().unwrap();
        assert_eq!(requests.try_iter().count(), 1);
        for replies in [
            vec![
                (
                    200,
                    json!({"messages":[{"id":"one"}], "nextPageToken":"repeated"})
                );
                2
            ],
            vec![(200, json!({"messages":[{}]}))],
            vec![(
                403,
                json!({"error":{"errors":[{"reason":"insufficientPermissions"}]}}),
            )],
        ] {
            let expected = replies.len();
            let (api, requests, thread) = server(replies);
            let error = scoped_client(dir.path(), api, "https://mail.google.com/")
                .empty_folder(&store, "TRASH")
                .unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("0 messages confirmed permanently deleted")
            );
            thread.join().unwrap();
            let requests: Vec<_> = requests.try_iter().collect();
            assert_eq!(requests.len(), expected);
            assert!(requests.iter().all(|request| request.starts_with("GET ")));
        }
    }

    #[test]
    fn empty_folder_does_not_retry_server_failure_or_remove_unconfirmed_cache() {
        let (api, requests, thread) = server(vec![
            (200, json!({"messages":[{"id":"unconfirmed"}]})),
            (503, json!({})),
        ]);
        let dir = tempfile::tempdir_in("/tmp").unwrap();
        let store = Store::open(None).unwrap();
        let mut message = crate::model::demo().1.messages[0].clone();
        message.id = "unconfirmed".into();
        store.save_message(&message).unwrap();
        let error = scoped_client(dir.path(), api, "https://mail.google.com/")
            .empty_folder(&store, "SPAM")
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("0 messages confirmed permanently deleted")
        );
        assert_eq!(
            error.downcast_ref::<GoogleReadError>().unwrap().status,
            reqwest::StatusCode::SERVICE_UNAVAILABLE
        );
        assert!(store.message("unconfirmed").unwrap().is_some());
        thread.join().unwrap();
        assert_eq!(requests.try_iter().count(), 2);
    }

    #[test]
    fn empty_folder_transport_failure_reports_uncertain_outcome() {
        // The mock closes its listener after listing; the mutation cannot
        // receive a confirmation. The UI must not claim that nothing changed.
        let (api, requests, thread) =
            server(vec![(200, json!({"messages":[{"id":"unconfirmed"}]}))]);
        let dir = tempfile::tempdir_in("/tmp").unwrap();
        let store = Store::open(None).unwrap();
        let mut message = crate::model::demo().1.messages[0].clone();
        message.id = "unconfirmed".into();
        store.save_message(&message).unwrap();
        let error = scoped_client(dir.path(), api, "https://mail.google.com/")
            .empty_folder(&store, "TRASH")
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("0 messages confirmed permanently deleted")
        );
        assert!(
            error
                .to_string()
                .contains("Deletion outcome is uncertain; refresh this folder before retrying")
        );
        assert!(store.message("unconfirmed").unwrap().is_some());
        thread.join().unwrap();
        assert_eq!(requests.try_iter().count(), 1);
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
    fn new_history_message_invalidates_existing_thread_and_folder_freshness() {
        let (api, _requests, server) = server(vec![(
            200,
            json!({
                "history":[{"messages":[{"id":"new-message","threadId":"existing-thread"}]}],
                "historyId":"new"
            }),
        )]);
        let dir = tempfile::tempdir_in("/tmp").unwrap();
        let mut google = client(dir.path(), api);
        let store = Store::open(None).unwrap();
        store.put("history", &"old").unwrap();
        store.mark_thread_fresh("existing-thread").unwrap();
        store
            .save_page(
                &crate::worker::key("INBOX", "", ""),
                &crate::model::demo().1,
            )
            .unwrap();
        google.changes(&store).unwrap();
        server.join().unwrap();
        assert!(!store.thread_is_fresh("existing-thread", 900).unwrap());
        assert!(!store.page_is_fresh("INBOX", "", "", 300).unwrap());
    }

    #[test]
    fn history_retries_server_failures_before_advancing_cursor() {
        let (api, requests, thread) = server(vec![
            (503, json!({})),
            (
                200,
                json!({"history":[{"messages":[{"id":"demo-0"}]}],"historyId":"new"}),
            ),
        ]);
        let dir = tempfile::tempdir_in("/tmp").unwrap();
        let mut google = client(dir.path(), api);
        let store = Store::open(None).unwrap();
        store.put("history", &"old").unwrap();
        store
            .save_message(&crate::model::demo().1.messages[0])
            .unwrap();
        google.changes(&store).unwrap();
        thread.join().unwrap();
        let requests: Vec<_> = requests.try_iter().collect();
        assert_eq!(requests.len(), 2);
        assert!(requests.iter().all(|r| r.starts_with("GET /history?")));
        assert!(requests.iter().all(|r| r.contains("startHistoryId=old")));
        assert!(store.message("demo-0").unwrap().is_none());
        assert_eq!(store.get::<String>("history").unwrap().unwrap(), "new");
    }

    #[test]
    fn history_permission_denial_preserves_cache_without_retrying() {
        let (api, requests, thread) = server(vec![(
            403,
            json!({"error":{"errors":[{"reason":"insufficientPermissions"}]}}),
        )]);
        let dir = tempfile::tempdir_in("/tmp").unwrap();
        let mut google = client(dir.path(), api);
        let store = Store::open(None).unwrap();
        store.put("history", &"old").unwrap();
        store
            .save_message(&crate::model::demo().1.messages[0])
            .unwrap();
        let error = google.changes(&store).unwrap_err().to_string();
        thread.join().unwrap();
        assert_eq!(requests.try_iter().count(), 1);
        assert!(error.contains("Required Google permissions are missing"));
        assert!(error.contains("Reconnect Gmail"));
        assert!(store.message("demo-0").unwrap().is_some());
        assert_eq!(store.get::<String>("history").unwrap().unwrap(), "old");
    }

    #[test]
    fn forbidden_errors_use_safe_actionable_messages_without_retries() {
        for (reason, message) in [
            (
                "ACCESS_TOKEN_SCOPE_INSUFFICIENT",
                "Required Google permissions",
            ),
            ("SERVICE_DISABLED", "Google API is disabled"),
            ("domainPolicy", "Google Workspace policy"),
            ("dailyLimitExceeded", "Google API quota is exhausted"),
            ("private-unknown-reason", "Google denied access"),
        ] {
            let (api, requests, thread) = server(vec![(
                403,
                json!({"error":{
                    "message":"private-account@example.test secret-project",
                    "details":[{"reason":reason,"metadata":{"token":"private-token"}}]
                }}),
            )]);
            let error = read(&http().unwrap(), "synthetic", &api, &[])
                .unwrap_err()
                .to_string();
            thread.join().unwrap();
            assert_eq!(requests.try_iter().count(), 1);
            assert!(error.contains("403 Forbidden"));
            assert!(error.contains(message), "{error}");
            assert!(!error.contains("private-"));
            assert!(!error.contains("secret-project"));
            if reason != "ACCESS_TOKEN_SCOPE_INSUFFICIENT" {
                assert!(!error.contains("Reconnect"));
            }
        }
    }

    #[test]
    fn history_rate_limit_stops_immediately_and_preserves_cursor() {
        let (api, requests, thread) = server(vec![(
            403,
            json!({"error":{"errors":[{"reason":"rateLimitExceeded"}]}}),
        )]);
        let dir = tempfile::tempdir_in("/tmp").unwrap();
        let mut google = client(dir.path(), api);
        let store = Store::open(None).unwrap();
        store.put("history", &"old").unwrap();
        let before = crate::auth::now();
        let error = google.changes(&store).unwrap_err();
        thread.join().unwrap();
        assert_eq!(requests.try_iter().count(), 1);
        assert!(error.to_string().contains("temporarily limiting requests"));
        assert!(!error.to_string().contains("Reconnect"));
        assert!(
            (before + 60..=crate::auth::now() + 60).contains(&rate_limit_until(&error).unwrap())
        );
        assert_eq!(store.get::<String>("history").unwrap().unwrap(), "old");
    }

    #[test]
    fn throttling_honors_retry_after_seconds_and_http_dates() {
        let date_deadline = crate::auth::now() + 3600;
        let date = chrono::DateTime::from_timestamp(date_deadline as i64, 0)
            .unwrap()
            .format("%a, %d %b %Y %H:%M:%S GMT")
            .to_string();
        for (status, header) in [(403, "120".to_string()), (429, date)] {
            let (api, requests, thread) = server_with_headers(vec![(
                status,
                json!({"error":{"errors":[{"reason":"userRateLimitExceeded"}]}}),
                format!("Retry-After: {header}\r\n"),
            )]);
            let before = crate::auth::now();
            let error = read(&http().unwrap(), "synthetic", &api, &[]).unwrap_err();
            thread.join().unwrap();
            assert_eq!(requests.try_iter().count(), 1);
            let deadline = rate_limit_until(&error.context("Synthetic request context")).unwrap();
            if status == 403 {
                assert!((before + 120..=crate::auth::now() + 120).contains(&deadline));
            } else {
                assert_eq!(deadline, date_deadline);
            }
        }
        assert_eq!(retry_after_until(None, 100), 160);
        assert_eq!(retry_after_until(Some("bad-date"), 100), 160);
        assert_eq!(retry_after_until(Some("0"), 100), 100);
        assert_eq!(
            retry_after_until(Some("Wed, 21 Oct 2015 07:28:00 GMT"), 2_000_000_000),
            2_000_000_000
        );
        assert!(rate_limit_until(&anyhow::anyhow!("unrelated error")).is_none());
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
