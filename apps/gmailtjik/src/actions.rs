//! Durable mailbox intents. Server snapshots stay separate from optimistic UI state.
use crate::{
    model::{Draft, Label, Message, Page},
    store::Store,
};
use anyhow::{Context, Result, ensure};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub enum Action {
    Labels {
        id: String,
        thread: String,
        add: Vec<String>,
        remove: Vec<String>,
    },
    Trash {
        id: String,
        thread: String,
    },
    MarkAllRead {
        label: String,
        query: String,
    },
    EmptyFolder {
        label: String,
    },
    SaveDraft {
        draft: Draft,
    },
    Send {
        draft: Draft,
    },
}

impl Action {
    pub fn retry_safe(&self) -> bool {
        matches!(
            self,
            Self::Labels { .. } | Self::Trash { .. } | Self::MarkAllRead { .. }
        )
    }

    pub fn description(&self) -> String {
        match self {
            Self::Labels { remove, .. } if remove.iter().any(|l| l == "INBOX") => {
                "Archive conversation".into()
            }
            Self::Labels { add, .. } if add.iter().any(|l| l == "UNREAD") => {
                "Mark conversation unread".into()
            }
            Self::Labels { remove, .. } if remove.iter().any(|l| l == "UNREAD") => {
                "Mark conversation read".into()
            }
            Self::Labels { add, .. } if add.iter().any(|l| l == "STARRED") => {
                "Star conversation".into()
            }
            Self::Labels { remove, .. } if remove.iter().any(|l| l == "STARRED") => {
                "Unstar conversation".into()
            }
            Self::Labels { .. } => "Update conversation".into(),
            Self::Trash { .. } => "Move conversation to Trash".into(),
            Self::MarkAllRead { .. } => "Mark all read".into(),
            Self::EmptyFolder { label } => {
                format!("Empty {}", if label == "SPAM" { "Spam" } else { "Trash" })
            }
            Self::SaveDraft { .. } => "Save draft".into(),
            Self::Send { .. } => "Send email".into(),
        }
    }

    pub fn changes(&self) -> (Vec<String>, Vec<String>) {
        match self {
            Self::Labels { add, remove, .. } => (add.clone(), remove.clone()),
            Self::Trash { .. } => (vec!["TRASH".into()], vec!["INBOX".into()]),
            Self::MarkAllRead { .. } => (vec![], vec!["UNREAD".into()]),
            _ => (vec![], vec![]),
        }
    }

    fn matches(&self, message: &Message, _seeds: &[Message]) -> bool {
        match self {
            Self::Labels { id, thread, .. } | Self::Trash { id, thread } => {
                if thread.is_empty() {
                    message.id == *id
                } else {
                    message.thread_id == *thread
                }
            }
            Self::MarkAllRead { label, query } => {
                (label.is_empty() || message.label_ids.contains(label))
                    && (label == "TRASH" || !message.label_ids.iter().any(|l| l == "TRASH"))
                    && (label == "SPAM" || !message.label_ids.iter().any(|l| l == "SPAM"))
                    && (label != "INBOX" || !message.label_ids.iter().any(|l| l == "SNOOZED"))
                    && matches!(query.trim(), "" | "is:unread")
            }
            Self::EmptyFolder { label } => message.label_ids.contains(label),
            _ => false,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub enum ActionState {
    Queued,
    Running,
    Succeeded,
    Failed,
}

impl ActionState {
    fn parse(value: &str) -> Result<Self> {
        match value {
            "queued" => Ok(Self::Queued),
            "running" => Ok(Self::Running),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            _ => anyhow::bail!("Invalid mailbox action state"),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct ActionResult {
    #[serde(default)]
    pub draft_id: String,
    #[serde(default)]
    pub count: usize,
}

#[derive(Clone, Debug)]
pub struct QueuedAction {
    pub id: i64,
    pub action: Action,
    pub state: ActionState,
    pub attempts: u32,
    pub next_attempt: u64,
    pub error: String,
    pub seeds: Vec<Message>,
    pub confirmed_ids: Vec<String>,
    pub awaiting_ids: Vec<String>,
    pub completed_at: u64,
    pub result: ActionResult,
    pub target_ids: Option<Vec<String>>,
}

fn now_ms() -> u64 {
    chrono::Utc::now().timestamp_millis().max(0) as u64
}

fn decode(row: &rusqlite::Row<'_>) -> rusqlite::Result<QueuedAction> {
    let parse = |index: usize| -> rusqlite::Result<String> { row.get(index) };
    let error =
        |index, e| rusqlite::Error::FromSqlConversionFailure(index, rusqlite::types::Type::Text, e);
    Ok(QueuedAction {
        id: row.get(0)?,
        action: serde_json::from_str(&parse(1)?).map_err(|e| error(1, Box::new(e)))?,
        seeds: serde_json::from_str(&parse(2)?).map_err(|e| error(2, Box::new(e)))?,
        state: ActionState::parse(&parse(3)?).map_err(|e| error(3, e.into()))?,
        attempts: row.get(4)?,
        next_attempt: row.get(5)?,
        error: row.get(6)?,
        confirmed_ids: serde_json::from_str(&parse(7)?).map_err(|e| error(7, Box::new(e)))?,
        awaiting_ids: serde_json::from_str(&parse(8)?).map_err(|e| error(8, Box::new(e)))?,
        completed_at: row.get(9)?,
        result: serde_json::from_str(&parse(10)?).map_err(|e| error(10, Box::new(e)))?,
        target_ids: row
            .get::<_, Option<String>>(11)?
            .map(|value| serde_json::from_str(&value))
            .transpose()
            .map_err(|e| error(11, Box::new(e)))?,
    })
}

const FIELDS: &str = "id,action,seeds,state,attempts,next_attempt,error,confirmed_ids,awaiting_ids,completed_at,result,target_ids";

impl Store {
    pub(crate) fn init_actions(&self) -> Result<()> {
        self.db.execute_batch(
            "CREATE TABLE IF NOT EXISTS mailbox_actions(
             id INTEGER PRIMARY KEY AUTOINCREMENT, action TEXT NOT NULL, seeds TEXT NOT NULL,
             state TEXT NOT NULL DEFAULT 'queued', attempts INTEGER NOT NULL DEFAULT 0,
             next_attempt INTEGER NOT NULL DEFAULT 0, error TEXT NOT NULL DEFAULT '',
             confirmed_ids TEXT NOT NULL DEFAULT '[]', awaiting_ids TEXT NOT NULL DEFAULT '[]',
             completed_at INTEGER NOT NULL DEFAULT 0, dismissed INTEGER NOT NULL DEFAULT 0,
             result TEXT NOT NULL DEFAULT '{}',target_ids TEXT);
             CREATE INDEX IF NOT EXISTS mailbox_actions_state ON mailbox_actions(state,id);
             CREATE INDEX IF NOT EXISTS mailbox_actions_visible ON mailbox_actions(dismissed,state,id);
             CREATE TABLE IF NOT EXISTS message_observations(id TEXT PRIMARY KEY,read_started INTEGER NOT NULL,labels TEXT NOT NULL);",
        )?;
        let mut columns = self.db.prepare("PRAGMA table_info(mailbox_actions)")?;
        let names = columns
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        drop(columns);
        if !names.iter().any(|name| name == "target_ids") {
            self.db
                .execute_batch("ALTER TABLE mailbox_actions ADD COLUMN target_ids TEXT;")?;
        }
        self.db.execute_batch(
            "CREATE INDEX IF NOT EXISTS mailbox_actions_projection ON mailbox_actions(id)
            WHERE state IN ('queued','running') OR awaiting_ids!='[]';",
        )?;
        Ok(())
    }

    /// Persist first, then let the caller update the visible page immediately.
    pub fn enqueue_action(&self, action: Action, seeds: &[Message]) -> Result<i64> {
        let transaction = self.db.unchecked_transaction()?;
        let id = self.enqueue_action_inner(action, seeds)?;
        transaction.commit()?;
        Ok(id)
    }

    fn enqueue_action_inner(&self, action: Action, seeds: &[Message]) -> Result<i64> {
        match &action {
            Action::Labels { id, .. } | Action::Trash { id, .. } => {
                ensure!(!id.is_empty(), "Missing message ID")
            }
            Action::EmptyFolder { label } => ensure!(
                matches!(label.as_str(), "SPAM" | "TRASH"),
                "Only Spam and Trash can be emptied"
            ),
            _ => {}
        }
        let seeds = self.action_seeds(seeds)?;
        self.db.execute(
            "INSERT INTO mailbox_actions(action,seeds) VALUES(?,?)",
            params![
                serde_json::to_string(&action)?,
                serde_json::to_string(&seeds)?
            ],
        )?;
        let id = self.db.last_insert_rowid();
        match &action {
            Action::Send { draft } if self.draft()? == *draft => {
                self.put("composer", &Draft::default())?
            }
            Action::SaveDraft { draft } if self.draft()? == *draft => {
                self.put("pending_draft_save", &id)?
            }
            _ => {}
        }
        Ok(id)
    }

    fn action_seeds(&self, seeds: &[Message]) -> Result<Vec<Message>> {
        // UI seeds may already include an earlier optimistic action. Prefer the
        // unprojected server snapshot, and never write a UI seed into that base.
        let mut statement = self
            .db
            .prepare("SELECT summary FROM mail_summaries WHERE id=?")?;
        let mut previous = None;
        let mut seen = HashSet::new();
        let mut result = Vec::new();
        for seed in seeds {
            if !seen.insert(seed.id.clone()) {
                continue;
            }
            let base: Option<String> = statement
                .query_row([&seed.id], |row| row.get(0))
                .optional()?;
            let mut message = if let Some(base) = base {
                serde_json::from_str(&base)?
            } else {
                if previous.is_none() {
                    previous = Some(self.projecting_actions()?);
                }
                previous
                    .as_ref()
                    .unwrap()
                    .iter()
                    .flat_map(|a| &a.seeds)
                    .find(|m| m.id == seed.id)
                    .cloned()
                    .unwrap_or_else(|| seed.clone())
            };
            message.summary_only = true;
            message.payload.body = Default::default();
            message.payload.parts.clear();
            result.push(message);
        }
        Ok(result)
    }

    pub fn action(&self, id: i64) -> Result<Option<QueuedAction>> {
        Ok(self
            .db
            .query_row(
                &format!("SELECT {FIELDS} FROM mailbox_actions WHERE id=?"),
                [id],
                decode,
            )
            .optional()?)
    }

    /// Pending work, actionable failures, and compose/bulk receipts not yet seen.
    pub fn actions(&self) -> Result<Vec<QueuedAction>> {
        let mut statement = self.db.prepare(&format!("SELECT {FIELDS} FROM mailbox_actions
            WHERE state IN ('queued','running') OR (dismissed=0 AND state IN ('failed','succeeded')) ORDER BY id"))?;
        let rows = statement
            .query_map([], decode)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows
            .into_iter()
            .filter(|a| {
                a.state != ActionState::Succeeded
                    || matches!(
                        a.action,
                        Action::Send { .. }
                            | Action::SaveDraft { .. }
                            | Action::EmptyFolder { .. }
                            | Action::MarkAllRead { .. }
                    )
            })
            .collect())
    }

    /// The delivery process holds an account-specific OS lock while using this.
    pub fn claim_action(&self, now: u64) -> Result<Option<QueuedAction>> {
        let transaction = self.db.unchecked_transaction()?;
        let next = transaction
            .query_row(
                &format!(
                    "SELECT {FIELDS} FROM mailbox_actions
            WHERE state IN ('queued','running') ORDER BY id LIMIT 1"
                ),
                [],
                decode,
            )
            .optional()?;
        let Some(mut next) = next else {
            return Ok(None);
        };
        if next.state == ActionState::Running || next.next_attempt > now {
            return Ok(None);
        }
        transaction.execute("UPDATE mailbox_actions SET state='running',attempts=attempts+1 WHERE id=? AND state='queued'", [next.id])?;
        transaction.commit()?;
        next.state = ActionState::Running;
        next.attempts += 1;
        Ok(Some(next))
    }

    /// Called only after obtaining exclusive delivery ownership. A crashed send
    /// or permanent delete is uncertain and must never be replayed automatically.
    pub fn recover_actions(&self) -> Result<()> {
        let mut statement = self.db.prepare(&format!(
            "SELECT {FIELDS} FROM mailbox_actions WHERE state='running'"
        ))?;
        let rows = statement
            .query_map([], decode)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        drop(statement);
        for row in rows {
            if row.action.retry_safe() {
                self.db.execute(
                    "UPDATE mailbox_actions SET state='queued',next_attempt=0 WHERE id=?",
                    [row.id],
                )?;
            } else {
                self.fail_action(row.id, "The app stopped before Gmail confirmed this action. Its outcome is uncertain; check Gmail before trying again.")?;
            }
        }
        Ok(())
    }

    pub fn defer_action(&self, id: i64, until: u64, error: &str) -> Result<()> {
        self.db.execute("UPDATE mailbox_actions SET state='queued',next_attempt=?,error=? WHERE id=? AND state='running'", params![until,error,id])?;
        Ok(())
    }

    pub fn fail_action(&self, id: i64, error: &str) -> Result<()> {
        self.db.execute("UPDATE mailbox_actions SET state='failed',error=?,dismissed=0 WHERE id=? AND state IN ('queued','running')", params![error,id])?;
        Ok(())
    }

    pub fn record_action_progress(&self, id: i64, ids: &[String]) -> Result<()> {
        let known: HashSet<_> = self
            .action(id)?
            .context("Missing queued action")?
            .confirmed_ids
            .into_iter()
            .collect();
        let new: Vec<_> = ids
            .iter()
            .filter(|id| !known.contains(*id))
            .cloned()
            .collect();
        // Keep write locks short enough that another window can persist its next
        // action while a large Gmail batch is being reconciled. Each receipt
        // chunk is independently durable; replaying label changes is idempotent.
        for chunk in new.chunks(50) {
            let transaction = self.db.unchecked_transaction()?;
            self.record_action_progress_inner(id, chunk)?;
            transaction.commit()?;
        }
        Ok(())
    }

    fn record_action_progress_inner(&self, id: i64, ids: &[String]) -> Result<()> {
        let row = self.action(id)?.context("Missing queued action")?;
        let mut confirmed: HashSet<String> = row.confirmed_ids.into_iter().collect();
        let mut awaiting: HashSet<String> = row.awaiting_ids.into_iter().collect();
        let completed_at = now_ms();
        let (add, remove) = row.action.changes();
        let mut materialized = Vec::new();
        for message_id in ids {
            if !confirmed.insert(message_id.clone()) {
                continue;
            }
            if add.is_empty() && remove.is_empty() {
                continue;
            }
            let summary: Option<String> = self
                .db
                .query_row(
                    "SELECT summary FROM mail_summaries WHERE id=?",
                    [message_id],
                    |row| row.get(0),
                )
                .optional()?;
            if let Some(summary) = summary {
                let mut message: Message = serde_json::from_str(&summary)?;
                self.normalize_observed(std::slice::from_mut(&mut message))?;
                message.label_ids.retain(|label| !remove.contains(label));
                for label in &add {
                    if !message.label_ids.contains(label) {
                        message.label_ids.push(label.clone());
                    }
                }
                self.db.execute("INSERT INTO message_observations(id,read_started,labels) VALUES(?,?,?)
                    ON CONFLICT(id) DO UPDATE SET read_started=excluded.read_started,labels=excluded.labels
                    WHERE excluded.read_started>=message_observations.read_started",
                    params![message_id,completed_at,serde_json::to_string(&message.label_ids)?])?;
                awaiting.remove(message_id);
                materialized.push(message_id.clone());
            } else {
                // No baseline exists from which to preserve unrelated labels.
                // Keep only this bulk receipt until that ID is first observed.
                awaiting.insert(message_id.clone());
            }
        }
        self.apply_receipt_membership(&materialized, &add, &remove)?;
        self.db.execute(
            "UPDATE mailbox_actions SET confirmed_ids=?,awaiting_ids=?,completed_at=? WHERE id=?",
            params![
                serde_json::to_string(&confirmed)?,
                serde_json::to_string(&awaiting)?,
                completed_at,
                id
            ],
        )?;
        Ok(())
    }

    pub fn prepare_action_targets(&self, id: i64, ids: &[String]) -> Result<Vec<String>> {
        self.db.execute(
            "UPDATE mailbox_actions SET target_ids=? WHERE id=? AND target_ids IS NULL",
            params![serde_json::to_string(ids)?, id],
        )?;
        Ok(self
            .action(id)?
            .context("Missing queued action")?
            .target_ids
            .context("Missing action targets")?)
    }

    pub fn complete_action(&self, id: i64, ids: &[String]) -> Result<()> {
        self.complete_action_result(
            id,
            ids,
            &ActionResult {
                count: ids.len(),
                ..Default::default()
            },
        )
    }

    pub fn complete_action_result(
        &self,
        id: i64,
        ids: &[String],
        result: &ActionResult,
    ) -> Result<()> {
        self.record_action_progress(id, ids)?;
        let transaction = self.db.unchecked_transaction()?;
        let row = self.action(id)?.context("Missing queued action")?;
        let awaiting = if matches!(
            row.action,
            Action::EmptyFolder { .. } | Action::Send { .. } | Action::SaveDraft { .. }
        ) {
            "[]".to_string()
        } else {
            serde_json::to_string(&row.awaiting_ids)?
        };
        let dismissed = matches!(row.action, Action::Labels { .. } | Action::Trash { .. });
        self.db.execute("UPDATE mailbox_actions SET state='succeeded',error='',awaiting_ids=?,result=?,dismissed=?,completed_at=MAX(completed_at,?) WHERE id=?",
            params![awaiting,serde_json::to_string(result)?,dismissed,now_ms(),id])?;
        transaction.commit()?;
        Ok(())
    }

    /// A retry is a new user intent after all later actions, never an old command
    /// inserted before them. Uncertain non-idempotent writes require new review.
    pub fn retry_action(&self, id: i64) -> Result<i64> {
        let row = self.action(id)?.context("Missing queued action")?;
        ensure!(
            row.state == ActionState::Failed,
            "Only a failed action can be retried"
        );
        ensure!(
            row.action.retry_safe(),
            "Check Gmail and review this action again before retrying"
        );
        let transaction = self.db.unchecked_transaction()?;
        let new_id = self.enqueue_action_inner(row.action, &row.seeds)?;
        if let Some(targets) = row.target_ids {
            self.prepare_action_targets(new_id, &targets)?;
        }
        self.dismiss_action(id)?;
        transaction.commit()?;
        Ok(new_id)
    }

    pub fn dismiss_action(&self, id: i64) -> Result<()> {
        self.db.execute(
            "UPDATE mailbox_actions SET dismissed=1 WHERE id=? AND state IN ('failed','succeeded')",
            [id],
        )?;
        Ok(())
    }

    pub fn acknowledge_draft_save(&self, id: i64) -> Result<Option<Draft>> {
        let transaction = self.db.unchecked_transaction()?;
        if self.get::<i64>("pending_draft_save")? != Some(id) {
            return Ok(None);
        }
        let Some(receipt) = self.action(id)? else {
            return Ok(None);
        };
        if receipt.state != ActionState::Succeeded
            || receipt.result.draft_id.is_empty()
            || !matches!(receipt.action, Action::SaveDraft { .. })
        {
            return Ok(None);
        }
        let mut draft = self.draft()?;
        draft.gmail_id = receipt.result.draft_id;
        self.put("composer", &draft)?;
        self.db
            .execute("DELETE FROM kv WHERE key='pending_draft_save'", [])?;
        transaction.commit()?;
        Ok(Some(draft))
    }

    fn projecting_actions(&self) -> Result<Vec<QueuedAction>> {
        let mut statement = self.db.prepare(&format!(
            "SELECT {FIELDS} FROM mailbox_actions
            WHERE state IN ('queued','running') OR awaiting_ids!='[]' ORDER BY id"
        ))?;
        Ok(statement
            .query_map([], decode)?
            .collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub(crate) fn optimistic_candidate_limit(&self) -> Result<usize> {
        let count: usize = self.db.query_row(
            "SELECT COALESCE(SUM(json_array_length(seeds)+json_array_length(awaiting_ids)),0)
             FROM mailbox_actions WHERE state IN ('queued','running') OR awaiting_ids!='[]'",
            [],
            |row| row.get(0),
        )?;
        Ok(25usize.saturating_add(count))
    }

    fn project_with(actions: &[QueuedAction], messages: &mut Vec<Message>) {
        for row in actions {
            let pending = matches!(row.state, ActionState::Queued | ActionState::Running);
            let targets = |message: &Message| {
                if pending {
                    if matches!(row.action, Action::MarkAllRead { .. })
                        && let Some(targets) = &row.target_ids
                    {
                        targets.contains(&message.id)
                    } else {
                        row.action.matches(message, &row.seeds)
                            || row.confirmed_ids.contains(&message.id)
                    }
                } else {
                    row.awaiting_ids.contains(&message.id)
                }
            };
            if matches!(row.action, Action::EmptyFolder { .. }) {
                if pending {
                    messages.retain(|message| !targets(message));
                }
                continue;
            }
            let (add, remove) = row.action.changes();
            for message in messages.iter_mut().filter(|message| targets(message)) {
                message.label_ids.retain(|label| !remove.contains(label));
                for label in &add {
                    if !message.label_ids.contains(label) {
                        message.label_ids.push(label.clone());
                    }
                }
            }
        }
    }

    pub fn project_messages(&self, messages: &mut Vec<Message>) -> Result<()> {
        self.filter_deleted(messages)?;
        self.normalize_observed(messages)?;
        Self::project_with(&self.projecting_actions()?, messages);
        Ok(())
    }

    /// Preserve the metadata from the newest started read when older network
    /// responses complete later. Full message bodies remain separately cached.
    pub fn observe_messages(
        &self,
        messages: &mut Vec<Message>,
        read_started_ms: u64,
    ) -> Result<()> {
        for chunk in messages.chunks_mut(50) {
            let transaction = self.db.unchecked_transaction()?;
            for message in chunk {
                self.db.execute("INSERT INTO message_observations(id,read_started,labels) VALUES(?,?,?)
                ON CONFLICT(id) DO UPDATE SET read_started=excluded.read_started,labels=excluded.labels
                WHERE excluded.read_started>=message_observations.read_started",
                params![message.id,read_started_ms,serde_json::to_string(&message.label_ids)?])?;
                let labels: String = self.db.query_row(
                    "SELECT labels FROM message_observations WHERE id=?",
                    [&message.id],
                    |row| row.get(0),
                )?;
                message.label_ids = serde_json::from_str(&labels)?;
            }
            transaction.commit()?;
        }
        self.filter_deleted(messages)?;
        Ok(())
    }

    pub(crate) fn normalize_observed(&self, messages: &mut [Message]) -> Result<()> {
        let mut statement = self
            .db
            .prepare("SELECT labels FROM message_observations WHERE id=?")?;
        for message in messages {
            let labels: Option<String> = statement
                .query_row([&message.id], |row| row.get(0))
                .optional()?;
            if let Some(labels) = labels {
                message.label_ids = serde_json::from_str(&labels)?;
            }
        }
        Ok(())
    }

    fn filter_deleted(&self, messages: &mut Vec<Message>) -> Result<()> {
        let mut query = self
            .db
            .prepare("SELECT 1 FROM deleted_messages WHERE id=?")?;
        let mut deleted = HashSet::new();
        for message in messages.iter() {
            if query
                .query_row([&message.id], |_| Ok(()))
                .optional()?
                .is_some()
            {
                deleted.insert(message.id.clone());
            }
        }
        messages.retain(|m| !deleted.contains(&m.id));
        Ok(())
    }

    /// Apply the known pending delta to an unprojected aggregate-count snapshot.
    /// The UI freezes that snapshot while delivery is pending, then reconciles
    /// successful known deltas before accepting a new server count snapshot.
    pub fn project_labels(&self, labels: &mut [Label]) -> Result<()> {
        let actions = self.projecting_actions()?;
        let seeds: Vec<_> = actions
            .iter()
            .flat_map(|a| a.seeds.iter().cloned())
            .collect();
        let mut baseline = self.action_seeds(&seeds)?;
        self.filter_deleted(&mut baseline)?;
        self.normalize_observed(&mut baseline)?;
        let settled: Vec<_> = actions
            .iter()
            .filter(|a| !matches!(a.state, ActionState::Queued | ActionState::Running))
            .cloned()
            .collect();
        Self::project_with(&settled, &mut baseline);
        let mut projected = baseline.clone();
        let pending: Vec<_> = actions
            .into_iter()
            .filter(|a| matches!(a.state, ActionState::Queued | ActionState::Running))
            .collect();
        Self::project_with(&pending, &mut projected);
        for label in labels {
            let count = |messages: &[Message]| {
                messages
                    .iter()
                    .filter(|m| m.unread() && m.label_ids.contains(&label.id))
                    .count() as u64
            };
            label.messages_unread = label
                .messages_unread
                .saturating_sub(count(&baseline))
                .saturating_add(count(&projected));
        }
        Ok(())
    }

    pub fn project_page(
        &self,
        label: &str,
        query: &str,
        token: &str,
        page: &mut Page,
    ) -> Result<()> {
        let actions = self.projecting_actions()?;
        // Only known first-page folder membership can be expanded locally.
        // Arbitrary Gmail search and pagination remain server-owned.
        if !label.is_empty() && token.is_empty() && matches!(query.trim(), "" | "is:unread") {
            let mut seen: HashSet<_> = page.messages.iter().map(|m| m.id.clone()).collect();
            for action in &actions {
                for seed in &action.seeds {
                    if seen.insert(seed.id.clone()) {
                        page.messages.push(seed.clone());
                    }
                }
            }
        }
        self.filter_deleted(&mut page.messages)?;
        self.normalize_observed(&mut page.messages)?;
        Self::project_with(&actions, &mut page.messages);
        page.retain_conversations(|m| {
            (label.is_empty()
                || (m.label_ids.iter().any(|l| l == label)
                    && (label == "TRASH" || !m.label_ids.iter().any(|l| l == "TRASH"))
                    && (label == "SPAM" || !m.label_ids.iter().any(|l| l == "SPAM"))
                    && (label != "INBOX" || !m.label_ids.iter().any(|l| l == "SNOOZED"))))
                && (query.trim() != "is:unread" || m.unread())
        });
        if !label.is_empty() && token.is_empty() && matches!(query.trim(), "" | "is:unread") {
            let mut conversations = page.conversations();
            conversations.sort_by_key(|row| {
                std::cmp::Reverse(row.latest().internal_date.parse::<i64>().unwrap_or(0))
            });
            page.messages = conversations
                .into_iter()
                .take(25)
                .flat_map(|row| row.messages.into_iter().cloned())
                .collect();
        }
        Ok(())
    }

    pub fn cached_thread(&self, id: &str) -> Result<Option<Vec<Message>>> {
        let Some(mut messages) = self.get::<Vec<Message>>(&format!("thread:{id}"))? else {
            return Ok(None);
        };
        self.project_messages(&mut messages)?;
        Ok(Some(messages))
    }

    /// Only a read that began after a receipt may retire its optimistic fence.
    /// Freeze to confirmed IDs so later incoming messages stay untouched.
    pub fn confirm_actions(&self, observed: &[Message], read_started_ms: u64) -> Result<()> {
        if observed.is_empty() {
            return Ok(());
        }
        let actions = self.projecting_actions()?;
        let mut confirmed = HashSet::new();
        for message in observed {
            // Opposite toggles must retire together. Retiring only the latest
            // would reveal the older optimistic state underneath it.
            let mut has_receipt = false;
            let mut eligible = true;
            for row in &actions {
                if matches!(row.state, ActionState::Queued | ActionState::Running) {
                    if row.action.matches(message, &row.seeds) {
                        eligible = false;
                        break;
                    }
                } else if row.awaiting_ids.contains(&message.id) {
                    if row.completed_at >= read_started_ms {
                        eligible = false;
                        break;
                    }
                    has_receipt = true;
                }
            }
            if eligible && has_receipt {
                confirmed.insert(message.id.clone());
            }
        }
        for row in actions {
            if matches!(row.state, ActionState::Queued | ActionState::Running) {
                continue;
            }
            let keep: Vec<_> = row
                .awaiting_ids
                .iter()
                .filter(|id| !confirmed.contains(*id))
                .cloned()
                .collect();
            if keep.len() != row.awaiting_ids.len() {
                self.db.execute(
                    "UPDATE mailbox_actions SET awaiting_ids=? WHERE id=?",
                    params![serde_json::to_string(&keep)?, row.id],
                )?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(store: &Store) -> Message {
        let message = crate::model::demo().1.messages.remove(0);
        store.save_message(&message).unwrap();
        store
            .put(
                &format!("thread:{}", message.thread_id),
                &vec![message.clone()],
            )
            .unwrap();
        store
            .save_page(
                &crate::worker::key("INBOX", "", ""),
                &Page {
                    messages: vec![message.clone()],
                    threaded: true,
                    ..Default::default()
                },
            )
            .unwrap();
        message
    }

    fn labels(message: &Message, add: &[&str], remove: &[&str]) -> Action {
        Action::Labels {
            id: message.id.clone(),
            thread: message.thread_id.clone(),
            add: add.iter().map(|s| s.to_string()).collect(),
            remove: remove.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn optimistic_failure_replays_later_intent_and_keeps_bodies_and_destinations() {
        let store = Store::open(None).unwrap();
        let message = fixture(&store);
        let archive = store
            .enqueue_action(
                labels(&message, &[], &["INBOX"]),
                std::slice::from_ref(&message),
            )
            .unwrap();
        assert!(
            store
                .cached_page("INBOX", "", "")
                .unwrap()
                .unwrap()
                .messages
                .is_empty()
        );
        let mut projected = vec![message.clone()];
        store.project_messages(&mut projected).unwrap();
        let star = store
            .enqueue_action(labels(&message, &["STARRED"], &[]), &projected)
            .unwrap();
        assert!(
            !store.action(star).unwrap().unwrap().seeds[0]
                .label_ids
                .contains(&"STARRED".to_string())
        );
        assert!(
            store.action(star).unwrap().unwrap().seeds[0]
                .label_ids
                .contains(&"INBOX".to_string())
        );
        assert_eq!(
            store
                .cached_page("STARRED", "", "")
                .unwrap()
                .unwrap()
                .messages
                .len(),
            1
        );
        // A stale remote page cannot resurrect the locally archived row.
        store
            .save_page(
                &crate::worker::key("INBOX", "", ""),
                &Page {
                    messages: vec![message.clone()],
                    threaded: true,
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(
            store
                .cached_page("INBOX", "", "")
                .unwrap()
                .unwrap()
                .messages
                .is_empty()
        );
        store.fail_action(archive, "Synthetic denied").unwrap();
        let inbox = store.cached_page("INBOX", "", "").unwrap().unwrap();
        assert_eq!(inbox.messages.len(), 1);
        assert!(inbox.messages[0].label_ids.contains(&"STARRED".into()));
        let thread = store.cached_thread(&message.thread_id).unwrap().unwrap();
        assert_eq!(thread[0].text(), message.text());
        assert!(thread[0].label_ids.contains(&"STARRED".into()));
        assert_eq!(
            store.message(&message.id).unwrap().unwrap().label_ids,
            message.label_ids
        );
    }

    #[test]
    fn fifo_retry_deadline_and_failure_retry_preserve_order_across_restart() {
        let dir = tempfile::tempdir_in("/tmp").unwrap();
        let store = Store::open(Some(dir.path())).unwrap();
        let message = fixture(&store);
        let first = store
            .enqueue_action(
                labels(&message, &[], &["UNREAD"]),
                std::slice::from_ref(&message),
            )
            .unwrap();
        let second = store
            .enqueue_action(
                labels(&message, &["STARRED"], &[]),
                std::slice::from_ref(&message),
            )
            .unwrap();
        assert_eq!(store.claim_action(10).unwrap().unwrap().id, first);
        store.defer_action(first, 20, "Synthetic offline").unwrap();
        assert!(store.claim_action(19).unwrap().is_none());
        drop(store);
        let store = Store::open(Some(dir.path())).unwrap();
        assert_eq!(store.claim_action(20).unwrap().unwrap().attempts, 2);
        store
            .fail_action(first, "Synthetic permanent failure")
            .unwrap();
        assert_eq!(store.claim_action(20).unwrap().unwrap().id, second);
        let retry = store.retry_action(first).unwrap();
        assert!(retry > second);
        assert!(store.actions().unwrap().iter().all(|a| a.id != first));
        store
            .complete_action(second, std::slice::from_ref(&message.id))
            .unwrap();
        assert_eq!(store.claim_action(20).unwrap().unwrap().id, retry);
    }

    #[test]
    fn recovery_never_replays_an_uncertain_send_or_permanent_delete() {
        let store = Store::open(None).unwrap();
        let draft = Draft {
            to: "friend@example.test".into(),
            subject: "Synthetic durable send".into(),
            body: "Keep this draft".into(),
            ..Default::default()
        };
        let send = store
            .enqueue_action(
                Action::Send {
                    draft: draft.clone(),
                },
                &[],
            )
            .unwrap();
        store.claim_action(0).unwrap().unwrap();
        store.recover_actions().unwrap();
        let failed = store.action(send).unwrap().unwrap();
        assert_eq!(failed.state, ActionState::Failed);
        assert!(failed.error.contains("uncertain"));
        assert_eq!(failed.action, Action::Send { draft });
        assert!(store.retry_action(send).is_err());
        let empty = store
            .enqueue_action(
                Action::EmptyFolder {
                    label: "TRASH".into(),
                },
                &[],
            )
            .unwrap();
        store.claim_action(0).unwrap().unwrap();
        store.recover_actions().unwrap();
        assert_eq!(
            store.action(empty).unwrap().unwrap().state,
            ActionState::Failed
        );
        assert!(store.claim_action(0).unwrap().is_none());
        assert!(
            store
                .enqueue_action(
                    Action::EmptyFolder {
                        label: "INBOX".into()
                    },
                    &[]
                )
                .is_err()
        );
    }

    #[test]
    fn partial_failure_rolls_back_only_unconfirmed_members() {
        let store = Store::open(None).unwrap();
        let first = fixture(&store);
        let mut second = first.clone();
        second.id = "other".into();
        second.thread_id = "other-thread".into();
        store.save_message(&second).unwrap();
        let action = store
            .enqueue_action(
                Action::MarkAllRead {
                    label: "INBOX".into(),
                    query: String::new(),
                },
                &[first.clone(), second.clone()],
            )
            .unwrap();
        store.claim_action(0).unwrap().unwrap();
        store
            .record_action_progress(action, std::slice::from_ref(&first.id))
            .unwrap();
        store.fail_action(action, "Second batch failed").unwrap();
        let mut messages = vec![first, second];
        store.project_messages(&mut messages).unwrap();
        assert!(!messages[0].unread());
        assert!(messages[1].unread());
        store.dismiss_action(action).unwrap();
        store.project_messages(&mut messages).unwrap();
        assert!(!messages[0].unread());
        assert!(store.actions().unwrap().is_empty());
    }

    #[test]
    fn confirmed_opposite_toggles_retire_together_and_never_touch_new_mail() {
        let store = Store::open(None).unwrap();
        let message = fixture(&store);
        let read = store
            .enqueue_action(
                labels(&message, &[], &["UNREAD"]),
                std::slice::from_ref(&message),
            )
            .unwrap();
        store
            .complete_action(read, std::slice::from_ref(&message.id))
            .unwrap();
        let unread = store
            .enqueue_action(
                labels(&message, &["UNREAD"], &[]),
                std::slice::from_ref(&message),
            )
            .unwrap();
        store
            .complete_action(unread, std::slice::from_ref(&message.id))
            .unwrap();
        let completion = store.action(unread).unwrap().unwrap().completed_at;
        let receipt: u64 = store
            .db
            .query_row(
                "SELECT read_started FROM message_observations WHERE id=?",
                [&message.id],
                |row| row.get(0),
            )
            .unwrap();
        store
            .confirm_actions(std::slice::from_ref(&message), completion)
            .unwrap();
        assert!(store.action(read).unwrap().unwrap().awaiting_ids.is_empty());
        assert!(
            store
                .action(unread)
                .unwrap()
                .unwrap()
                .awaiting_ids
                .is_empty()
        );
        let mut late = vec![message.clone()];
        late[0].label_ids.retain(|label| label != "UNREAD");
        store
            .observe_messages(&mut late, receipt.saturating_sub(1))
            .unwrap();
        assert!(late[0].unread());
        store
            .confirm_actions(std::slice::from_ref(&message), completion + 1)
            .unwrap();
        assert!(store.action(read).unwrap().unwrap().awaiting_ids.is_empty());
        assert!(
            store
                .action(unread)
                .unwrap()
                .unwrap()
                .awaiting_ids
                .is_empty()
        );
        let archived = store
            .enqueue_action(
                labels(&message, &[], &["INBOX"]),
                std::slice::from_ref(&message),
            )
            .unwrap();
        store
            .complete_action(archived, std::slice::from_ref(&message.id))
            .unwrap();
        let mut incoming = message.clone();
        incoming.id = "new-reply".into();
        let mut messages = vec![message, incoming];
        store.project_messages(&mut messages).unwrap();
        assert!(!messages[0].label_ids.contains(&"INBOX".into()));
        assert!(messages[1].label_ids.contains(&"INBOX".into()));
    }

    #[test]
    fn send_enqueue_and_matching_composer_clear_are_one_transaction() {
        let store = Store::open(None).unwrap();
        let draft = Draft {
            to: "friend@example.test".into(),
            body: "Queued synthetic body".into(),
            ..Default::default()
        };
        store.put("composer", &draft).unwrap();
        let save = store
            .enqueue_action(
                Action::SaveDraft {
                    draft: draft.clone(),
                },
                &[],
            )
            .unwrap();
        assert_eq!(store.get::<i64>("pending_draft_save").unwrap(), Some(save));
        let send = store
            .enqueue_action(
                Action::Send {
                    draft: draft.clone(),
                },
                &[],
            )
            .unwrap();
        assert_eq!(store.draft().unwrap(), Draft::default());
        assert!(
            matches!(store.action(send).unwrap().unwrap().action,Action::Send { draft: saved } if saved==draft)
        );
        let newer = Draft {
            body: "A newer window's draft".into(),
            ..Default::default()
        };
        store.put("composer", &newer).unwrap();
        store
            .enqueue_action(
                Action::Send {
                    draft: draft.clone(),
                },
                &[],
            )
            .unwrap();
        assert_eq!(store.draft().unwrap(), newer);
        store.put("composer", &draft).unwrap();
        store
            .db
            .execute_batch(
                "CREATE TRIGGER reject_composer_update BEFORE INSERT ON kv WHEN NEW.key='composer'
            BEGIN SELECT RAISE(ABORT,'synthetic disk failure'); END;",
            )
            .unwrap();
        let before = store.actions().unwrap().len();
        assert!(
            store
                .enqueue_action(
                    Action::Send {
                        draft: draft.clone()
                    },
                    &[]
                )
                .is_err()
        );
        assert_eq!(store.actions().unwrap().len(), before);
        assert_eq!(store.draft().unwrap(), draft);
    }

    #[test]
    fn draft_receipt_acknowledges_only_the_current_binding_and_preserves_edits() {
        let store = Store::open(None).unwrap();
        let draft = Draft {
            to: "friend@example.test".into(),
            body: "Original synthetic body".into(),
            ..Default::default()
        };
        store.put("composer", &draft).unwrap();
        let saved = store
            .enqueue_action(
                Action::SaveDraft {
                    draft: draft.clone(),
                },
                &[],
            )
            .unwrap();
        let edited = Draft {
            body: "Edited while Gmail was saving".into(),
            ..draft.clone()
        };
        store.put("composer", &edited).unwrap();
        store
            .complete_action_result(
                saved,
                &[],
                &ActionResult {
                    draft_id: "gmail-123".into(),
                    count: 0,
                },
            )
            .unwrap();
        store.dismiss_action(saved).unwrap(); // Another window saw the receipt first.
        let acknowledged = store.acknowledge_draft_save(saved).unwrap().unwrap();
        assert_eq!(acknowledged.body, edited.body);
        assert_eq!(acknowledged.gmail_id, "gmail-123");
        assert!(store.get::<i64>("pending_draft_save").unwrap().is_none());
        assert!(store.acknowledge_draft_save(saved).unwrap().is_none());
        let second = store
            .enqueue_action(
                Action::SaveDraft {
                    draft: acknowledged.clone(),
                },
                &[],
            )
            .unwrap();
        store
            .complete_action_result(
                second,
                &[],
                &ActionResult {
                    draft_id: "gmail-456".into(),
                    count: 0,
                },
            )
            .unwrap();
        store.put("pending_draft_save", &999_i64).unwrap();
        assert!(store.acknowledge_draft_save(second).unwrap().is_none());
        assert_eq!(store.draft().unwrap(), acknowledged);
    }

    #[test]
    fn fresh_external_change_wins_but_late_reads_and_old_sync_writes_cannot_regress() {
        let store = Store::open(None).unwrap();
        let message = fixture(&store);
        let action = store
            .enqueue_action(
                labels(&message, &[], &["UNREAD"]),
                std::slice::from_ref(&message),
            )
            .unwrap();
        store
            .complete_action(action, std::slice::from_ref(&message.id))
            .unwrap();
        let receipt: u64 = store
            .db
            .query_row(
                "SELECT read_started FROM message_observations WHERE id=?",
                [&message.id],
                |row| row.get(0),
            )
            .unwrap();
        assert!(!store.cached_thread(&message.thread_id).unwrap().unwrap()[0].unread());
        let mut old = vec![message.clone()];
        store
            .observe_messages(&mut old, receipt.saturating_sub(1))
            .unwrap();
        assert!(!old[0].unread());
        // A later change made in Gmail web is authoritative and must be visible.
        let mut external = vec![message.clone()];
        store.observe_messages(&mut external, receipt + 10).unwrap();
        store.confirm_actions(&external, receipt + 10).unwrap();
        store.save_message(&external[0]).unwrap();
        assert!(store.cached_thread(&message.thread_id).unwrap().unwrap()[0].unread());
        let mut late = old;
        store.observe_messages(&mut late, receipt + 5).unwrap();
        assert!(late[0].unread());
        let mut stale = message.clone();
        stale.label_ids.retain(|label| label != "UNREAD");
        store.save_message(&stale).unwrap(); // An old sync binary skips observe_messages.
        store
            .put(&format!("thread:{}", message.thread_id), &vec![stale])
            .unwrap();
        assert!(store.cached_thread(&message.thread_id).unwrap().unwrap()[0].unread());
        assert!(
            store
                .cached_page("INBOX", "is:unread", "")
                .unwrap()
                .unwrap()
                .messages[0]
                .unread()
        );
    }

    #[test]
    fn bulk_targets_are_frozen_and_complex_query_does_not_guess_thread_members() {
        let store = Store::open(None).unwrap();
        let first = fixture(&store);
        let mut other = first.clone();
        other.id = "unmatched-reply".into();
        let action = store
            .enqueue_action(
                Action::MarkAllRead {
                    label: "INBOX".into(),
                    query: "from:friend@example.test".into(),
                },
                &[first.clone(), other.clone()],
            )
            .unwrap();
        let mut messages = vec![first.clone(), other.clone()];
        store.project_messages(&mut messages).unwrap();
        assert!(messages.iter().all(Message::unread));
        store
            .prepare_action_targets(action, std::slice::from_ref(&first.id))
            .unwrap();
        assert_eq!(
            store
                .prepare_action_targets(action, std::slice::from_ref(&other.id))
                .unwrap(),
            vec![first.id.clone()]
        );
        store.project_messages(&mut messages).unwrap();
        assert!(!messages[0].unread());
        assert!(messages[1].unread());
        store.fail_action(action, "Synthetic failure").unwrap();
        let retry = store.retry_action(action).unwrap();
        assert_eq!(
            store.action(retry).unwrap().unwrap().target_ids,
            Some(vec![first.id])
        );
    }

    #[test]
    fn pending_counts_are_deduplicated_and_completion_receipts_persist() {
        let dir = tempfile::tempdir_in("/tmp").unwrap();
        let store = Store::open(Some(dir.path())).unwrap();
        let message = fixture(&store);
        store
            .enqueue_action(
                labels(&message, &[], &["UNREAD"]),
                &[message.clone(), message.clone()],
            )
            .unwrap();
        let mut counts = vec![Label {
            id: "INBOX".into(),
            messages_unread: 12,
            ..Default::default()
        }];
        store.project_labels(&mut counts).unwrap();
        assert_eq!(counts[0].messages_unread, 11);
        let save = store
            .enqueue_action(
                Action::SaveDraft {
                    draft: Draft {
                        subject: "Remember saved draft id".into(),
                        ..Default::default()
                    },
                },
                &[],
            )
            .unwrap();
        store
            .complete_action_result(
                save,
                &[],
                &ActionResult {
                    draft_id: "gmail-draft-id".into(),
                    count: 0,
                },
            )
            .unwrap();
        drop(store);
        let store = Store::open(Some(dir.path())).unwrap();
        assert_eq!(
            store.action(save).unwrap().unwrap().result.draft_id,
            "gmail-draft-id"
        );
        assert!(store.actions().unwrap().iter().any(|a| a.id == save));
        store.dismiss_action(save).unwrap();
        assert!(store.actions().unwrap().iter().all(|a| a.id != save));
    }

    #[test]
    fn queued_trash_survives_reopen_and_fills_a_confirmed_empty_destination() {
        let dir = tempfile::tempdir_in("/tmp").unwrap();
        let store = Store::open(Some(dir.path())).unwrap();
        let message = fixture(&store);
        store
            .save_page(
                &crate::worker::key("TRASH", "", ""),
                &Page {
                    threaded: true,
                    ..Default::default()
                },
            )
            .unwrap();
        let action = store
            .enqueue_action(
                Action::Trash {
                    id: message.id.clone(),
                    thread: message.thread_id.clone(),
                },
                std::slice::from_ref(&message),
            )
            .unwrap();
        drop(store);
        let store = Store::open(Some(dir.path())).unwrap();
        let another = Store::open(Some(dir.path())).unwrap();
        assert!(
            store
                .cached_page("INBOX", "", "")
                .unwrap()
                .unwrap()
                .messages
                .is_empty()
        );
        let trash = store.cached_page("TRASH", "", "").unwrap().unwrap();
        assert_eq!(trash.messages.len(), 1);
        assert_eq!(trash.messages[0].id, message.id);
        assert_eq!(store.claim_action(0).unwrap().unwrap().id, action);
        assert!(another.claim_action(0).unwrap().is_none());
        store
            .fail_action(action, "Synthetic permission denied")
            .unwrap();
        assert!(
            another
                .cached_page("TRASH", "", "")
                .unwrap()
                .unwrap()
                .messages
                .is_empty()
        );
        assert_eq!(
            another
                .cached_page("INBOX", "", "")
                .unwrap()
                .unwrap()
                .messages
                .len(),
            1
        );
    }

    #[test]
    fn successful_trash_membership_survives_old_empty_snapshots() {
        let store = Store::open(None).unwrap();
        let message = fixture(&store);
        let key = crate::worker::key("TRASH", "", "");
        let empty = Page {
            threaded: true,
            ..Default::default()
        };
        store.save_page(&key, &empty).unwrap();
        let action = store
            .enqueue_action(
                Action::Trash {
                    id: message.id.clone(),
                    thread: message.thread_id.clone(),
                },
                std::slice::from_ref(&message),
            )
            .unwrap();
        store
            .complete_action(action, std::slice::from_ref(&message.id))
            .unwrap();
        assert!(
            store
                .action(action)
                .unwrap()
                .unwrap()
                .awaiting_ids
                .is_empty()
        );
        assert_eq!(
            store
                .cached_page("TRASH", "", "")
                .unwrap()
                .unwrap()
                .messages
                .len(),
            1
        );
        store.save_page(&key, &empty).unwrap(); // An older raw snapshot lacks this new member.
        assert_eq!(
            store
                .cached_page("TRASH", "", "")
                .unwrap()
                .unwrap()
                .messages
                .len(),
            1
        );
        store
            .confirm_folder_snapshot(&key, i64::MAX as u64)
            .unwrap();
        assert!(
            store
                .cached_page("TRASH", "", "")
                .unwrap()
                .unwrap()
                .messages
                .is_empty()
        );
    }

    #[test]
    fn bulk_receipts_materialize_without_accumulating_projection_work() {
        let store = Store::open(None).unwrap();
        let sample = crate::model::demo().1.messages.remove(0);
        let mut ids = Vec::new();
        for index in 0..1000 {
            let mut message = sample.clone();
            message.id = format!("bulk-{index}");
            message.thread_id = message.id.clone();
            ids.push(message.id.clone());
            store.save_message(&message).unwrap();
        }
        let action = store
            .enqueue_action(
                Action::MarkAllRead {
                    label: "INBOX".into(),
                    query: String::new(),
                },
                &[],
            )
            .unwrap();
        store.prepare_action_targets(action, &ids).unwrap();
        let mut max_chunk = std::time::Duration::ZERO;
        for chunk in ids.chunks(50) {
            let started = std::time::Instant::now();
            store.record_action_progress(action, chunk).unwrap();
            max_chunk = max_chunk.max(started.elapsed());
        }
        store.complete_action(action, &ids).unwrap();
        assert!(
            store
                .action(action)
                .unwrap()
                .unwrap()
                .awaiting_ids
                .is_empty()
        );
        assert!(store.projecting_actions().unwrap().is_empty());
        let started = std::time::Instant::now();
        for _ in 0..20 {
            assert_eq!(
                store
                    .cached_page("INBOX", "", "")
                    .unwrap()
                    .unwrap()
                    .conversations()
                    .len(),
                25
            );
        }
        eprintln!(
            "bulk receipt 50-ID transaction upper bound: {:?}; projected cached folder mean: {:?}",
            max_chunk,
            started.elapsed() / 20
        );
        assert_eq!(
            store.message(&ids[0]).unwrap().unwrap().text(),
            sample.text()
        );
    }

    #[test]
    fn permanent_tombstone_also_blocks_retained_action_seeds() {
        let store = Store::open(None).unwrap();
        let message = fixture(&store);
        let star = store
            .enqueue_action(
                labels(&message, &["STARRED"], &[]),
                std::slice::from_ref(&message),
            )
            .unwrap();
        store
            .complete_action(star, std::slice::from_ref(&message.id))
            .unwrap();
        store
            .remove_messages(std::slice::from_ref(&message.id))
            .unwrap();
        assert!(store.cached_page("STARRED", "", "").unwrap().is_none());
        let mut stale = vec![message];
        store.project_messages(&mut stale).unwrap();
        assert!(stale.is_empty());
    }

    #[test]
    fn rapid_offline_archives_refill_from_older_indexed_conversations() {
        let store = Store::open(None).unwrap();
        let sample = crate::model::demo().1.messages.remove(0);
        let messages: Vec<_> = (0..60)
            .map(|index| {
                let mut message = sample.clone();
                message.id = format!("mail-{index:02}");
                message.thread_id = message.id.clone();
                message.internal_date = (100000 - index).to_string();
                message
            })
            .collect();
        for message in &messages {
            store.save_message(message).unwrap();
        }
        store
            .save_page(
                &crate::worker::key("INBOX", "", ""),
                &Page {
                    messages: messages[..25].to_vec(),
                    threaded: true,
                    next: "remote-page".into(),
                },
            )
            .unwrap();
        for message in &messages[..30] {
            let first = store.cached_page("INBOX", "", "").unwrap().unwrap();
            assert_eq!(first.representatives()[0].id, message.id);
            store
                .enqueue_action(
                    labels(message, &[], &["INBOX"]),
                    std::slice::from_ref(message),
                )
                .unwrap();
        }
        let page = store.cached_page("INBOX", "", "").unwrap().unwrap();
        assert_eq!(page.conversations().len(), 25);
        assert_eq!(page.representatives()[0].id, "mail-30");
        assert!(page.next.is_empty());
        let raw: String = store
            .db
            .query_row(
                "SELECT summary FROM mail_summaries WHERE id='mail-00'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(
            serde_json::from_str::<Message>(&raw)
                .unwrap()
                .label_ids
                .contains(&"INBOX".into())
        );
    }
}
