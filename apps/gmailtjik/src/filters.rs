use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Complete the last comma-separated label without offering already selected labels.
pub fn label_suggestions(value: &str, labels: &[crate::model::Label]) -> Vec<String> {
    let (selected, query) = value.rsplit_once(',').unwrap_or(("", value));
    let query = query.trim().to_lowercase();
    let selected: Vec<_> = selected
        .split(',')
        .map(|s| s.trim().to_lowercase())
        .collect();
    let mut names: Vec<_> = labels
        .iter()
        .filter(|label| label.id.starts_with("Label_") || label.name != label.id)
        .filter(|label| {
            let name = label.name.to_lowercase();
            name.contains(&query) && !selected.contains(&name)
        })
        .map(|label| label.name.clone())
        .collect();
    names.sort_by_key(|name| {
        (
            !name.to_lowercase().starts_with(&query),
            name.to_lowercase(),
        )
    });
    names.dedup();
    names.truncate(8);
    names
}

pub fn complete_label(value: &str, name: &str) -> String {
    match value.rsplit_once(',') {
        Some((selected, _)) => format!("{}, {name}, ", selected.trim_end()),
        None => format!("{name}, "),
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Filter {
    #[serde(default)]
    pub id: String,
    #[serde(default = "object")]
    pub criteria: Value,
    #[serde(default = "object")]
    pub action: Value,
}
fn object() -> Value {
    json!({})
}
impl Default for Filter {
    fn default() -> Self {
        Self {
            id: String::new(),
            criteria: object(),
            action: object(),
        }
    }
}
#[derive(Clone)]
pub enum Request {
    List,
    SaveGroup {
        originals: Vec<Filter>,
        conditions: Vec<String>,
        action: Value,
        apply: bool,
    },
    DeleteGroup(Vec<Filter>),
}
pub struct Outcome {
    pub group_saved: bool,
    pub filters: Option<Vec<Filter>>,
    pub note: String,
}

impl Filter {
    pub fn label_ids(&self, field: &str) -> Vec<String> {
        self.action[field]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect()
    }
    pub fn query(&self) -> Result<String> {
        ensure!(
            self.criteria
                .as_object()
                .is_some_and(|fields| fields.keys().all(|field| [
                    "from",
                    "to",
                    "subject",
                    "query",
                    "negatedQuery",
                    "hasAttachment",
                    "excludeChats",
                    "size",
                    "sizeComparison"
                ]
                .contains(&field.as_str()))),
            "This filter contains unsupported criteria; preserve it in Gmail"
        );
        let mut clauses = Vec::new();
        for field in ["from", "to", "subject"] {
            if let Some(value) = self.criteria[field]
                .as_str()
                .filter(|v| !v.trim().is_empty())
            {
                ensure!(
                    !value.chars().any(char::is_control),
                    "Invalid {field} criterion"
                );
                clauses.push(format!("{field}:({value})"));
            }
        }
        for (field, prefix) in [("query", ""), ("negatedQuery", "-")] {
            if let Some(value) = self.criteria[field]
                .as_str()
                .filter(|v| !v.trim().is_empty())
            {
                ensure!(!value.chars().any(char::is_control), "Invalid search query");
                clauses.push(format!("{prefix}({value})"));
            }
        }
        if self.criteria["hasAttachment"] == true {
            clauses.push("has:attachment".into());
        }
        if self.criteria["excludeChats"] == true {
            clauses.push("-in:chats".into());
        }
        if let Some(size) = self.criteria["size"].as_i64().filter(|s| *s > 0) {
            let comparison = match self.criteria["sizeComparison"].as_str() {
                Some("larger") => "larger",
                Some("smaller") => "smaller",
                _ => bail!("Unsupported size comparison"),
            };
            clauses.push(format!("{comparison}:{size}"));
        }
        ensure!(!clauses.is_empty(), "Enter at least one matching criterion");
        Ok(clauses.join(" "))
    }
    pub fn validate(&self, apply: bool) -> Result<()> {
        self.query()?;
        ensure!(
            !self.label_ids("addLabelIds").is_empty()
                || !self.label_ids("removeLabelIds").is_empty()
                || self.action["forward"]
                    .as_str()
                    .is_some_and(|v| !v.is_empty()),
            "Choose at least one action"
        );
        if apply {
            ensure!(
                self.action["forward"].as_str().is_none_or(str::is_empty),
                "Forwarding cannot be applied to existing mail here"
            );
        }
        Ok(())
    }
}

/// The whole action set is the identity of a handling, including forwarding.
pub fn action_key(action: &Value) -> String {
    let mut normalized = std::collections::BTreeMap::new();
    if let Some(fields) = action.as_object() {
        for (key, value) in fields {
            if value.is_null()
                || value.as_str() == Some("")
                || value.as_array().is_some_and(Vec::is_empty)
            {
                continue;
            }
            let value = if let Some(values) = value.as_array() {
                let mut values = values.clone();
                values.sort_by_key(Value::to_string);
                values.dedup();
                Value::Array(values)
            } else {
                value.clone()
            };
            normalized.insert(key.clone(), value);
        }
    }
    serde_json::to_string(&normalized).unwrap()
}

#[derive(Clone, Debug)]
pub struct Handling {
    pub action: Value,
    pub originals: Vec<Filter>,
}
pub fn handlings(filters: &[Filter]) -> Vec<Handling> {
    let mut groups = std::collections::BTreeMap::<String, Handling>::new();
    for filter in filters {
        groups
            .entry(action_key(&filter.action))
            .or_insert_with(|| Handling {
                action: filter.action.clone(),
                originals: vec![],
            })
            .originals
            .push(filter.clone());
    }
    groups.into_values().collect()
}

/// Split only top-level OR, respecting nested groups, braces, quoted strings and escapes.
/// A query with AND outside an OR group remains one condition.
pub fn split_conditions(query: &str) -> Vec<String> {
    fn outer(query: &str) -> bool {
        if !query.starts_with('(') || !query.ends_with(')') {
            return false;
        }
        let mut depth = 0_i32;
        let mut quoted = false;
        let mut escaped = false;
        for (index, ch) in query.char_indices() {
            if escaped {
                escaped = false;
                continue;
            }
            if ch == '\\' {
                escaped = true;
                continue;
            }
            if ch == '"' {
                quoted = !quoted;
            }
            if quoted {
                continue;
            }
            if ch == '(' {
                depth += 1;
            }
            if ch == ')' {
                depth -= 1;
            }
            if depth == 0 && index + ch.len_utf8() != query.len() {
                return false;
            }
            if depth < 0 {
                return false;
            }
        }
        depth == 0 && !quoted
    }
    let mut query = query.trim();
    while outer(query) {
        query = query[1..query.len() - 1].trim();
    }
    let mut quoted = false;
    let mut escaped = false;
    let mut depth = 0_i32;
    let mut braces = 0_i32;
    let mut start = 0;
    let mut rows = Vec::new();
    for (index, ch) in query.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == '"' {
            quoted = !quoted;
        }
        if quoted {
            continue;
        }
        match ch {
            '(' => depth += 1,
            ')' => depth -= 1,
            '{' => braces += 1,
            '}' => braces -= 1,
            _ => {}
        }
        if depth < 0 || braces < 0 {
            return vec![query.into()];
        }
        if depth == 0 && braces == 0 && query[index..].starts_with(" OR ") {
            rows.extend(split_conditions(&query[start..index]));
            start = index + 4;
        }
    }
    if quoted || depth != 0 || braces != 0 {
        return vec![query.into()];
    }
    if start == 0 {
        vec![query.into()]
    } else {
        rows.extend(split_conditions(&query[start..]));
        rows
    }
}

impl Handling {
    pub fn conditions(&self) -> Result<Vec<String>> {
        let mut rows = Vec::new();
        for filter in &self.originals {
            if filter
                .criteria
                .as_object()
                .is_some_and(|fields| fields.len() == 1 && fields.contains_key("query"))
            {
                rows.extend(split_conditions(
                    filter.criteria["query"].as_str().unwrap_or_default(),
                ));
            } else {
                rows.push(filter.query()?);
            }
        }
        rows.sort();
        rows.dedup();
        Ok(rows)
    }
    pub fn title(&self, labels: &[crate::model::Label]) -> String {
        let mut parts = Vec::new();
        if let Some(ids) = self.action["addLabelIds"].as_array() {
            for id in ids.iter().filter_map(Value::as_str) {
                parts.push(match id {
                    "TRASH" => "Delete".into(),
                    "STARRED" => "Star".into(),
                    _ => format!(
                        "Label: {}",
                        labels
                            .iter()
                            .find(|l| l.id == id)
                            .map(|l| l.name.as_str())
                            .unwrap_or(id)
                    ),
                });
            }
        }
        if let Some(ids) = self.action["removeLabelIds"].as_array() {
            for id in ids.iter().filter_map(Value::as_str) {
                parts.push(match id {
                    "INBOX" => "Archive".into(),
                    "UNREAD" => "Mark as read".into(),
                    "SPAM" => "Never Spam".into(),
                    _ => format!(
                        "Remove: {}",
                        labels
                            .iter()
                            .find(|l| l.id == id)
                            .map(|l| l.name.as_str())
                            .unwrap_or(id)
                    ),
                });
            }
        }
        if let Some(address) = self.action["forward"].as_str().filter(|v| !v.is_empty()) {
            parts.push(format!("Forward: {address}"));
        }
        if parts.is_empty() {
            "No actions (legacy)".into()
        } else {
            parts.join(" · ")
        }
    }
}

/// Gmail limits an individual query; larger handlings transparently use several
/// server filters, while the UI continues to show one shared action set.
pub fn compile_handling(action: &Value, conditions: &[String]) -> Result<Vec<Filter>> {
    let mut queries = conditions
        .iter()
        .map(|q| q.trim().to_string())
        .collect::<Vec<_>>();
    ensure!(
        !queries.is_empty()
            && queries
                .iter()
                .all(|q| !q.is_empty() && !q.chars().any(char::is_control)),
        "Every OR condition needs a Gmail search query"
    );
    queries.sort();
    queries.dedup();
    let mut filters = Vec::new();
    let mut query = String::new();
    for condition in queries {
        let branch = format!("({condition})");
        ensure!(
            branch.len() <= 1400,
            "An individual condition is too long for Gmail (1400 bytes maximum)"
        );
        if !query.is_empty() && query.len() + branch.len() + 4 > 1400 {
            filters.push(Filter {
                criteria: json!({"query":query}),
                action: action.clone(),
                ..Default::default()
            });
            query.clear();
        }
        if !query.is_empty() {
            query.push_str(" OR ");
        }
        query.push_str(&branch);
    }
    filters.push(Filter {
        criteria: json!({"query":query}),
        action: action.clone(),
        ..Default::default()
    });
    for filter in &filters {
        filter.validate(false)?;
    }
    Ok(filters)
}

pub trait FilterApi {
    fn list(&mut self) -> Result<Vec<Filter>>;
    fn create(&mut self, filter: &Filter) -> Result<Filter>;
    fn delete(&mut self, id: &str) -> Result<()>;
}
impl FilterApi for crate::google::Google {
    fn list(&mut self) -> Result<Vec<Filter>> {
        self.filters()
    }
    fn create(&mut self, filter: &Filter) -> Result<Filter> {
        self.create_filter(filter)
    }
    fn delete(&mut self, id: &str) -> Result<()> {
        self.delete_filter(id)
    }
}

/// A durable journal lives in the private account cache. Never remove original
/// coverage until every replacement has been created and independently read back.
pub fn replace_handling(
    api: &mut impl FilterApi,
    store: &crate::store::Store,
    originals: &[Filter],
    action: Option<&Value>,
    conditions: &[String],
) -> Result<Vec<Filter>> {
    let current = api.list()?;
    for original in originals {
        ensure!(
            current.iter().any(|row| row == original),
            "Handling changed in Gmail; refresh before saving"
        );
    }
    if let Some(first) = originals.first() {
        let key = action_key(&first.action);
        ensure!(
            current
                .iter()
                .filter(|row| action_key(&row.action) == key)
                .all(|row| originals.iter().any(|old| old.id == row.id)),
            "Handling changed in Gmail; refresh before saving"
        );
    }
    let mut originals = originals.to_vec();
    let mut conditions = conditions.to_vec();
    // Changing handling to an already existing action set joins that handling
    // instead of silently losing its earlier OR conditions.
    if let Some(action) = action {
        let target = current
            .iter()
            .filter(|row| {
                action_key(&row.action) == action_key(action)
                    && !originals.iter().any(|old| old.id == row.id)
            })
            .cloned()
            .collect::<Vec<_>>();
        if !target.is_empty() {
            conditions.extend(
                Handling {
                    action: action.clone(),
                    originals: target.clone(),
                }
                .conditions()?,
            );
            originals.extend(target);
        }
    }
    let replacements = if let Some(action) = action {
        compile_handling(action, &conditions)?
    } else {
        vec![]
    };
    let journal_key = format!(
        "gmail_filter_change_{}_{:016x}",
        chrono::Utc::now().timestamp_millis(),
        rand::random::<u64>()
    );
    let mut journal = json!({"originals":originals,"replacements":replacements,"created":[],"deleted":[],"phase":"prepared"});
    store.put(&journal_key, &journal)?;
    let mut created = Vec::new();
    for replacement in &replacements {
        journal["phase"] = json!("creating");
        journal["pending"] = serde_json::to_value(replacement)?;
        store.put(&journal_key, &journal)?;
        let saved = api.create(replacement)?;
        ensure!(
            !saved.id.is_empty()
                && saved.criteria == replacement.criteria
                && action_key(&saved.action) == action_key(&replacement.action),
            "Gmail returned a different replacement; originals preserved"
        );
        created.push(saved);
        journal["created"] = serde_json::to_value(&created)?;
        journal["pending"] = Value::Null;
        store.put(&journal_key, &journal)?;
    }
    let verified = api.list()?;
    ensure!(
        created.iter().all(|row| verified.contains(row))
            && originals.iter().all(|row| verified.contains(row)),
        "Filter read-back changed; original filters were not deleted"
    );
    journal["phase"] = json!("replacing");
    store.put(&journal_key, &journal)?;
    let mut deleted = Vec::new();
    for original in &originals {
        api.delete(&original.id)?;
        deleted.push(original.id.clone());
        journal["deleted"] = json!(deleted);
        store.put(&journal_key, &journal)?;
    }
    let final_filters = api.list()?;
    ensure!(
        created.iter().all(|row| final_filters.contains(row))
            && originals
                .iter()
                .all(|row| !final_filters.iter().any(|f| f.id == row.id)),
        "Gmail filter verification was incomplete; refresh before editing again"
    );
    journal["phase"] = json!("verified");
    store.put(&journal_key, &journal)?;
    Ok(final_filters)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn changing_to_existing_handling_preserves_its_conditions() {
        struct Fake {
            rows: Vec<Filter>,
        }
        impl FilterApi for Fake {
            fn list(&mut self) -> Result<Vec<Filter>> {
                Ok(self.rows.clone())
            }
            fn create(&mut self, filter: &Filter) -> Result<Filter> {
                let mut filter = filter.clone();
                filter.id = "replacement".into();
                self.rows.push(filter.clone());
                Ok(filter)
            }
            fn delete(&mut self, id: &str) -> Result<()> {
                self.rows.retain(|row| row.id != id);
                Ok(())
            }
        }
        let old = Filter {
            id: "old".into(),
            criteria: json!({"from":"old@example.test"}),
            action: json!({"addLabelIds":["STARRED"]}),
        };
        let target = Filter {
            id: "target".into(),
            criteria: json!({"from":"target@example.test"}),
            action: json!({"removeLabelIds":["INBOX"]}),
        };
        let mut api = Fake {
            rows: vec![old.clone(), target.clone()],
        };
        let store = crate::store::Store::open(None).unwrap();
        let saved = replace_handling(
            &mut api,
            &store,
            &[old],
            Some(&target.action),
            &["from:edited@example.test".into()],
        )
        .unwrap();
        assert_eq!(saved.len(), 1);
        let query = saved[0].criteria["query"].as_str().unwrap();
        assert!(query.contains("target@example.test") && query.contains("edited@example.test"));
        assert_eq!(action_key(&saved[0].action), action_key(&target.action));
    }
    #[test]
    fn group_save_keeps_original_coverage_on_create_or_verification_failure() {
        struct Fake {
            rows: Vec<Filter>,
            creates: usize,
            deletes: usize,
            fail_create: bool,
            hide_created: bool,
        }
        impl FilterApi for Fake {
            fn list(&mut self) -> Result<Vec<Filter>> {
                Ok(self
                    .rows
                    .iter()
                    .filter(|row| !self.hide_created || row.id == "old")
                    .cloned()
                    .collect())
            }
            fn create(&mut self, filter: &Filter) -> Result<Filter> {
                self.creates += 1;
                ensure!(!self.fail_create, "Synthetic create failure");
                let mut filter = filter.clone();
                filter.id = format!("new{}", self.creates);
                self.rows.push(filter.clone());
                Ok(filter)
            }
            fn delete(&mut self, id: &str) -> Result<()> {
                self.deletes += 1;
                self.rows.retain(|row| row.id != id);
                Ok(())
            }
        }
        let old = Filter {
            id: "old".into(),
            criteria: json!({"from":"a@example.test"}),
            action: json!({"removeLabelIds":["INBOX"]}),
        };
        let store = crate::store::Store::open(None).unwrap();
        for (fail_create, hide_created) in [(true, false), (false, true)] {
            let mut api = Fake {
                rows: vec![old.clone()],
                creates: 0,
                deletes: 0,
                fail_create,
                hide_created,
            };
            assert!(
                replace_handling(
                    &mut api,
                    &store,
                    &[old.clone()],
                    Some(&old.action),
                    &["from:b@example.test".into()]
                )
                .is_err()
            );
            assert_eq!(api.deletes, 0);
            assert!(api.rows.contains(&old));
        }
        let mut api = Fake {
            rows: vec![old.clone()],
            creates: 0,
            deletes: 0,
            fail_create: false,
            hide_created: false,
        };
        let result = replace_handling(
            &mut api,
            &store,
            &[old.clone()],
            Some(&old.action),
            &["from:b@example.test".into()],
        )
        .unwrap();
        assert_eq!(api.deletes, 1);
        assert_eq!(result.len(), 1);
        assert!(
            result[0].criteria["query"]
                .as_str()
                .unwrap()
                .contains("b@example.test")
        );
    }
    #[test]
    fn grouping_uses_complete_order_independent_actions() {
        let filters = vec![
            Filter {
                criteria: json!({"from":"a@example.test"}),
                action: json!({"addLabelIds":["Label_1","STARRED"],"removeLabelIds":["INBOX"]}),
                ..Default::default()
            },
            Filter {
                criteria: json!({"from":"b@example.test"}),
                action: json!({"removeLabelIds":["INBOX"],"addLabelIds":["STARRED","Label_1"]}),
                ..Default::default()
            },
            Filter {
                criteria: json!({"from":"c@example.test"}),
                action: json!({"addLabelIds":["Label_1","STARRED"],"forward":"other@example.test"}),
                ..Default::default()
            },
        ];
        let groups = handlings(&filters);
        assert_eq!(groups.len(), 2);
        assert!(groups.iter().any(|g| g.originals.len() == 2));
    }
    #[test]
    fn or_conditions_preserve_nested_and_quoted_queries_and_round_trip_shards() {
        assert_eq!(
            split_conditions("((from:(a OR b)) OR (subject:\"A OR B\" has:attachment))"),
            ["from:(a OR b)", "subject:\"A OR B\" has:attachment"]
        );
        assert_eq!(
            split_conditions("(from:a OR from:b) subject:receipt"),
            ["(from:a OR from:b) subject:receipt"]
        );
        assert_eq!(
            split_conditions("{from:a from:b} subject:x"),
            ["{from:a from:b} subject:x"]
        );
        let conditions = (0..240)
            .map(|n| format!("from:sender{n}@example.test subject:receipt"))
            .collect::<Vec<_>>();
        let action = json!({"addLabelIds":["Label_1"]});
        let filters = compile_handling(&action, &conditions).unwrap();
        assert!(filters.len() > 1);
        assert!(
            filters
                .iter()
                .all(|f| f.criteria["query"].as_str().unwrap().len() <= 1400)
        );
        let mut expected = conditions;
        expected.sort();
        assert_eq!(handlings(&filters)[0].conditions().unwrap(), expected);
        assert!(compile_handling(&action, &[String::new()]).is_err());
    }
    #[test]
    fn label_completion_preserves_selection_and_matches_existing_nested_names() {
        let labels = [
            ("INBOX", "INBOX"),
            ("Label_1", "Mail/Code"),
            ("Label_2", "Mail/Design"),
            ("Label_3", "Receipts"),
            ("Label_4", "財務/收據"),
        ]
        .into_iter()
        .map(|(id, name)| crate::model::Label {
            id: id.into(),
            name: name.into(),
            ..Default::default()
        })
        .collect::<Vec<_>>();
        assert_eq!(label_suggestions("cOd", &labels), ["Mail/Code"]);
        assert_eq!(label_suggestions("收", &labels), ["財務/收據"]);
        assert_eq!(
            label_suggestions("Mail/Code, mail", &labels),
            ["Mail/Design"]
        );
        assert_eq!(
            complete_label("Receipts, de", "Mail/Design"),
            "Receipts, Mail/Design, "
        );
        assert_eq!(complete_label("cod", "Mail/Code"), "Mail/Code, ");
        assert!(label_suggestions("unknown", &labels).is_empty());
        assert!(!label_suggestions("", &labels).contains(&"INBOX".into()));
    }
    #[test]
    fn query_covers_filter_criteria_and_blocks_historical_forwarding() {
        let mut filter = Filter {
            criteria: json!({"from":"person@example.test", "query":"has:document OR has:spreadsheet", "negatedQuery":"is:starred", "hasAttachment":true,"size":100,"sizeComparison":"larger"}),
            action: json!({"removeLabelIds":["INBOX"]}),
            ..Default::default()
        };
        assert_eq!(
            filter.query().unwrap(),
            "from:(person@example.test) (has:document OR has:spreadsheet) -(is:starred) has:attachment larger:100"
        );
        filter.validate(true).unwrap();
        filter.action["forward"] = json!("other@example.test");
        assert!(filter.validate(true).is_err());
        filter.validate(false).unwrap();
        assert!(Filter::default().validate(false).is_err());
    }
}
