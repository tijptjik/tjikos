//! Browser capture stays on the client; the HTTP authority owns saved reading items.
use crate::{browser, discovery::Entry, todo::Todo, util::*};
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};

#[derive(Default)]
pub struct Reading {
    draft: Option<Draft>,
}
struct Draft {
    id: String,
    entry: Entry,
    submitted: Option<Value>,
}
impl Reading {
    pub fn begin(&mut self, entry: &Entry, settings: &Value, demo: bool) -> Result<Value> {
        if !demo {
            let options = tasktjik_api::client::options(settings)?;
            ensure!(
                !s(&options["url"]).is_empty() || !s(&options["socket"]).is_empty(),
                "Configure Tasktjik's HTTP authority before saving reading items"
            );
        }
        let mut entry = entry.clone();
        if !demo {
            let details = browser::request(
                &browser::session_path(s(&entry.data["session"]))?,
                &json!({"op":"details","window":entry.data["window"],"tab":entry.data["tab"]}),
            )?;
            ensure!(
                details["tab"]["id"] == entry.data["tab"],
                "Browser tab changed; refresh the list"
            );
            entry.name = s(&details["tab"]["title"]).into();
            entry.data["url"] = details["tab"]["url"].clone();
        }
        let url = url::Url::parse(s(&entry.data["url"]))?;
        ensure!(
            ["http", "https"].contains(&url.scheme()) && url.host_str().is_some(),
            "Reading items need an HTTP or HTTPS URL; tab left open"
        );
        // Reopening the same capture after a failed save retains its request ID.
        if !self
            .draft
            .as_ref()
            .is_some_and(|d| d.entry.id == entry.id && d.entry.data["url"] == entry.data["url"])
        {
            self.draft = Some(Draft {
                id: uuid::Uuid::new_v4().to_string(),
                entry,
                submitted: None,
            });
        }
        let draft = self.draft.as_ref().unwrap();
        let fields = draft.submitted.as_ref().map(|r| r["fields"].clone()).unwrap_or_else(||
            json!({"kind":"reading","title":draft.entry.name,"reference":draft.entry.data["url"],"group":"All","tags":[],"due":"","repeat":{}}));
        Ok(json!({"collection":"reading","draft_id":draft.id,"draft":fields}))
    }

    pub fn action(
        &mut self,
        request: Value,
        todo: &mut Todo,
        settings: &Value,
        demo: bool,
    ) -> Result<Value> {
        let draft = self
            .draft
            .as_mut()
            .context("Reading draft expired; select the tab again")?;
        ensure!(
            request["draft_id"] == draft.id,
            "Reading draft changed; select the tab again"
        );
        if request["action"] == "reading-cancel" {
            self.draft = None;
            return Ok(
                json!({"draft_id":"","mutation_accepted":true,"message":"Capture cancelled; tab left open"}),
            );
        }
        let mut fields = request["fields"].clone();
        ensure!(fields.is_object(), "Missing reading fields");
        fields["kind"] = json!("reading");
        let create = json!({"action":"create","request_id":draft.id,"fields":fields});
        // Validate before freezing a submission so a validation error is editable.
        tasktjik_api::model::mutate(&[], &create)?;
        if let Some(previous) = &draft.submitted {
            ensure!(
                previous == &create,
                "Retry the original reading draft to confirm its save before editing it in Reading"
            );
        }
        draft.submitted = Some(create.clone());
        let mut state = todo.state(create, settings, demo)?;
        ensure!(
            state["mutation_accepted"] == true,
            "Reading save was not acknowledged; tab left open"
        );
        // A close failure must never turn a successful save into another create.
        let closed = if demo {
            Ok(json!({}))
        } else {
            browser::session_path(s(&draft.entry.data["session"])).and_then(|path| {
                browser::request(
                    &path,
                    &json!({"op":"close","window":draft.entry.data["window"],
                    "tab":draft.entry.data["tab"],"url":draft.entry.data["url"]}),
                )
            })
        };
        state["message"] = json!(match closed {
            Ok(_) => String::new(),
            Err(e) => format!("Saved to Reading; tab could not be closed: {e}"),
        });
        state["collection"] = json!("reading");
        state["draft_id"] = json!("");
        self.draft = None;
        Ok(state)
    }
}
