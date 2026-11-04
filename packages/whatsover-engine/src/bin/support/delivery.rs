//! Desktop send evidence: SDK completion is distinct from server acceptance.
use anyhow::Result;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use whatsapp_rust::prelude::{Event, EventHandler};
use whatsover_engine::{Outbound, native::SqliteJournal};

#[derive(Clone, Default)]
pub struct DeliveryEvents(Arc<Mutex<Vec<Value>>>);

impl EventHandler for DeliveryEvents {
    fn handle_event(&self, event: Arc<Event>) {
        let value = match &*event {
            Event::ServerAck(ack) if ack.class.as_deref() == Some("message") => {
                json!({"kind": "server_ack", "data": ack})
            }
            Event::Receipt(receipt) => json!({"kind": "receipt", "data": receipt}),
            _ => return,
        };
        self.0.lock().unwrap().push(value);
    }
}

impl DeliveryEvents {
    pub async fn flush(&self, journal: &SqliteJournal) -> Result<usize> {
        let batch = std::mem::take(&mut *self.0.lock().unwrap());
        if batch.is_empty() {
            return Ok(0);
        }
        if let Err(error) = journal.record_delivery_events(&batch).await {
            self.0.lock().unwrap().extend(batch);
            return Err(error);
        }
        Ok(batch.len())
    }
}

fn canonical_jid(value: &Value) -> Option<String> {
    let jid = if let Some(text) = value.as_str() {
        text.parse::<whatsapp_rust::prelude::Jid>().ok()?
    } else {
        serde_json::from_value::<whatsapp_rust::prelude::Jid>(value.clone()).ok()?
    };
    whatsover_engine::normalize_recipient(&jid.to_non_ad_string()).ok()
}

pub fn evidence(result: &Outbound, events: Vec<Value>) -> Value {
    let ack = events.iter().rev().find(|e| {
        e["kind"] == "server_ack"
            && e["data"]["class"] == "message"
            && e["data"]["id"] == result.message_id
    });
    let server = match ack {
        Some(e) if e["data"]["error"].is_null() => "accepted",
        Some(_) => "rejected",
        None => "unconfirmed",
    };
    // The matching successful server ACK can name the recipient by LID even
    // when our request used a phone number. Retain that authoritative address.
    let server_recipient = ack
        .filter(|e| e["data"]["error"].is_null())
        .and_then(|e| canonical_jid(&e["data"]["from"]));
    // Own-device echoes/read-self receipts do not prove recipient delivery.
    let delivered = events.iter().any(|e| {
        let d = &e["data"];
        e["kind"] == "receipt"
            && d["message_ids"]
                .as_array()
                .is_some_and(|ids| ids.iter().any(|id| id == &result.message_id))
            && d["source"]["is_from_me"] == false
            && d["source"]["is_group"] == false
            && ["Delivered", "Read", "Played"]
                .iter()
                .any(|t| d["type"] == *t)
            && ["sender", "sender_alt"].iter().any(|key| {
                canonical_jid(&d["source"][key]).is_some_and(|jid| {
                    jid == result.request.to || server_recipient.as_ref() == Some(&jid)
                })
            })
    });
    json!({"server": server, "recipientDeliveryConfirmed": delivered, "events": events})
}

pub async fn result_json(journal: &SqliteJournal, result: &Outbound) -> Result<Value> {
    let mut value = serde_json::to_value(result)?;
    value["transport"] = evidence(result, journal.delivery_events(&result.message_id).await?);
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use whatsover_engine::{SendState, TextRequest};

    fn result() -> Outbound {
        Outbound {
            request: TextRequest {
                request_id: "synthetic".into(),
                to: "15555550100@s.whatsapp.net".into(),
                text: "test".into(),
                reply_to: None,
                reaction: None,
                attachment_id: None,
            },
            message_id: "synthetic-message".into(),
            state: SendState::Submitted,
        }
    }

    #[test]
    fn sdk_completion_does_not_prove_server_acceptance_or_delivery() {
        let r = result();
        assert_eq!(evidence(&r, vec![])["server"], "unconfirmed");
        let ack =
            json!({"kind":"server_ack","data":{"id":r.message_id,"class":"message","error":null}});
        let e = evidence(&r, vec![ack.clone()]);
        assert_eq!(e["server"], "accepted");
        assert_eq!(e["recipientDeliveryConfirmed"], false);
        let mut nack = ack;
        nack["data"]["error"] = json!("479");
        assert_eq!(evidence(&r, vec![nack])["server"], "rejected");
    }

    #[test]
    fn delivery_requires_matching_recipient_message_and_receipt_type() {
        let r = result();
        let receipt = json!({"kind":"receipt","data":{"message_ids":[r.message_id],"type":"Delivered","source":{"sender":r.request.to,"is_from_me":false,"is_group":false}}});
        assert_eq!(
            evidence(&r, vec![receipt.clone()])["recipientDeliveryConfirmed"],
            true
        );
        for (key, value) in [
            ("type", json!("Sender")),
            ("type", json!("ReadSelf")),
            ("type", json!("Sent")),
            ("message_ids", json!(["other"])),
        ] {
            let mut e = receipt.clone();
            e["data"][key] = value;
            assert_eq!(evidence(&r, vec![e])["recipientDeliveryConfirmed"], false);
        }
        let mut own = receipt.clone();
        own["data"]["source"]["is_from_me"] = json!(true);
        assert_eq!(evidence(&r, vec![own])["recipientDeliveryConfirmed"], false);
        let mut other = receipt.clone();
        other["data"]["source"]["sender"] = json!("15555550101@s.whatsapp.net");
        assert_eq!(
            evidence(&r, vec![other.clone()])["recipientDeliveryConfirmed"],
            false
        );
        other["data"]["source"]["sender_alt"] = json!(r.request.to);
        assert_eq!(
            evidence(&r, vec![other])["recipientDeliveryConfirmed"],
            true
        );
    }

    #[test]
    fn sdk_structured_lid_receipts_match_only_the_acked_recipient() {
        let r = result();
        let recipient: whatsapp_rust::prelude::Jid = "123456789012345@lid".parse().unwrap();
        let own_device: whatsapp_rust::prelude::Jid = "999999999999999:79@lid".parse().unwrap();
        let ack = json!({"kind":"server_ack","data":{"id":r.message_id,"class":"message","error":null,"from":recipient}});
        let receipt = json!({"kind":"receipt","data":{"message_ids":[r.message_id],"type":"Delivered","source":{"sender":recipient,"is_from_me":false,"is_group":false}}});
        assert_eq!(
            evidence(&r, vec![ack.clone(), receipt.clone()])["recipientDeliveryConfirmed"],
            true
        );
        assert_eq!(
            evidence(&r, vec![receipt.clone()])["recipientDeliveryConfirmed"],
            false
        );
        let mut own = receipt.clone();
        own["data"]["source"]["sender"] = json!(own_device);
        assert_eq!(
            evidence(&r, vec![ack.clone(), own])["recipientDeliveryConfirmed"],
            false
        );
        let mut unrelated = ack.clone();
        unrelated["data"]["id"] = json!("other-message");
        assert_eq!(
            evidence(&r, vec![unrelated, receipt.clone()])["recipientDeliveryConfirmed"],
            false
        );
        let mut rejected = ack;
        rejected["data"]["error"] = json!("479");
        assert_eq!(
            evidence(&r, vec![rejected, receipt])["recipientDeliveryConfirmed"],
            false
        );
    }
}
