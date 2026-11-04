use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Debug, Serialize, Deserialize)]
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
    Save { filter: Filter, apply: bool },
    Delete(String),
    Apply(Filter),
}
pub struct Outcome {
    pub saved: Option<Filter>,
    pub deleted: Option<String>,
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
                clauses.push(format!(
                    "{field}:\"{}\"",
                    value.replace('\\', "\\\\").replace('"', "\\\"")
                ));
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
    pub fn title(&self) -> String {
        self.query().unwrap_or_else(|_| self.criteria.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn query_covers_filter_criteria_and_blocks_historical_forwarding() {
        let mut filter = Filter {
            criteria: json!({"from":"person@example.test", "query":"has:document OR has:spreadsheet", "negatedQuery":"is:starred", "hasAttachment":true,"size":100,"sizeComparison":"larger"}),
            action: json!({"removeLabelIds":["INBOX"]}),
            ..Default::default()
        };
        assert_eq!(
            filter.query().unwrap(),
            "from:\"person@example.test\" (has:document OR has:spreadsheet) -(is:starred) has:attachment larger:100"
        );
        filter.validate(true).unwrap();
        filter.action["forward"] = json!("other@example.test");
        assert!(filter.validate(true).is_err());
        filter.validate(false).unwrap();
        assert!(Filter::default().validate(false).is_err());
    }
}
