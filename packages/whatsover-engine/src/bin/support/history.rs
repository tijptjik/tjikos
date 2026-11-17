//! Bounded phone history requests and durable capture, independent of live inbox sync.
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::sync::{Arc, Mutex};
use whatsapp_rust::prelude::{Event, EventHandler};
use whatsapp_rust::{HistorySyncDurabilityHook, HistorySyncKey, HistorySyncMetadata};
use whatsover_engine::{InboxMessage, Journal, native::SqliteJournal};

#[derive(Clone)]
pub struct History {
    pub journal: Arc<SqliteJournal>,
    events: Arc<Mutex<Vec<Arc<Event>>>>,
}
impl History {
    pub fn new(journal: Arc<SqliteJournal>) -> Self {
        Self {
            journal,
            events: Default::default(),
        }
    }
    pub async fn flush(&self) -> Result<Vec<Value>> {
        let batch = std::mem::take(&mut *self.events.lock().unwrap());
        let mut reports = Vec::new();
        for event in batch {
            if let Event::HistorySync(sync) = &*event {
                ensure!(
                    sync.decompressed_size() <= 32 * 1024 * 1024,
                    "History chunk exceeds 32 MiB decode limit; raw chunk retained"
                );
                let value = serde_json::to_value(
                    sync.get()
                        .context("History decode failed; raw chunk retained")?,
                )?;
                let messages = decode_messages(&value)?;
                let report = json!({"requestId":sync.peer_data_request_session_id(),"chunkOrder":sync.chunk_order(),"progress":sync.progress(),"messages":messages.len(),"historyComplete":false});
                self.journal
                    .history_record(
                        &format!(
                            "decoded:{}",
                            hex::encode(Sha256::digest(sync.compressed_bytes()))
                        ),
                        &json!({"report":report,"history":value}),
                    )
                    .await?;
                self.journal.receive(messages).await?;
                reports.push(report);
            }
        }
        Ok(reports)
    }
}
impl EventHandler for History {
    fn handle_event(&self, event: Arc<Event>) {
        if matches!(&*event, Event::HistorySync(_)) {
            self.events.lock().unwrap().push(event);
        }
    }
}
#[async_trait::async_trait]
impl HistorySyncDurabilityHook for History {
    async fn on_notification(
        &self,
        key: &HistorySyncKey,
        metadata: &HistorySyncMetadata<'_>,
        bytes: &[u8],
    ) -> Result<()> {
        self.journal.history_record(&format!("notification:{}:{}:{}",key.chat,key.sender,key.id), &json!({"requestId":metadata.peer_data_request_session_id,"chat":key.chat,"sender":key.sender,"messageId":key.id,"notificationHex":hex::encode(bytes)})).await
    }
    async fn on_compressed_chunk(
        &self,
        key: &HistorySyncKey,
        metadata: &HistorySyncMetadata<'_>,
        bytes: &[u8],
    ) -> Result<()> {
        self.journal.history_record(&format!("chunk:{}:{}:{}",key.chat,key.sender,key.id), &json!({"requestId":metadata.peer_data_request_session_id,"compressedHex":hex::encode(bytes)})).await
    }
}
fn number(v: &Value) -> Option<i64> {
    v.as_i64().or_else(|| v.as_str()?.parse().ok())
}
fn decode_messages(history: &Value) -> Result<Vec<InboxMessage>> {
    let mut result = Vec::new();
    for conversation in history["conversations"].as_array().into_iter().flatten() {
        let chat = conversation["id"]
            .as_str()
            .context("History conversation lacks chat ID")?;
        for entry in conversation["messages"].as_array().into_iter().flatten() {
            let web = &entry["message"];
            let key = &web["key"];
            let id = key["id"].as_str().context("History message lacks ID")?;
            let from_me = key["from_me"]
                .as_bool()
                .context("History message lacks direction")?;
            let timestamp = number(&web["message_timestamp"])
                .context("History message lacks timestamp")?
                .checked_mul(1000)
                .context("History timestamp overflow")?;
            let msg = &web["message"];
            let text = msg["conversation"]
                .as_str()
                .or_else(|| msg["extended_text_message"]["text"].as_str())
                .or_else(|| msg["image_message"]["caption"].as_str())
                .map(str::to_owned);
            result.push(InboxMessage {
                chat: chat.to_owned(),
                sender: key["participant"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .unwrap_or(if from_me { "self" } else { chat })
                    .to_owned(),
                id: id.to_owned(),
                timestamp,
                from_me,
                text,
                payload: serde_json::to_string(
                    &json!({"message":msg,"historyWebMessage":web,"source":"phone_history"}),
                )?,
            });
        }
    }
    Ok(result)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn history_preserves_direction_timestamp_and_media_caption() {
        let h = json!({"conversations":[{"id":"15550001111@s.whatsapp.net","messages":[{"message":{"key":{"id":"reply","from_me":false},"message_timestamp":"1700000000","message":{"image_message":{"caption":"Here is our contribution"}}}}]}]});
        let m = decode_messages(&h).unwrap();
        assert_eq!(m[0].timestamp, 1700000000000);
        assert!(!m[0].from_me);
        assert_eq!(m[0].text.as_deref(), Some("Here is our contribution"));
        let bad = json!({"conversations":[{"id":"chat","messages":[{"message":{"key":{"id":"bad"},"message_timestamp":1700000000}}]}]});
        assert!(decode_messages(&bad).is_err());
    }
}
