//! Account-local ordered rules. Handlers are deliberately compiled, not commands.
use crate::{
    model::{Label, Message},
    store::Store,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Handler {
    AuthCode,
    EmailNotification,
    Silent,
}
impl Handler {
    pub fn label(self) -> &'static str {
        match self {
            Self::AuthCode => "Extract authentication code",
            Self::EmailNotification => "Email notification",
            Self::Silent => "No notification",
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Rule {
    pub name: String,
    pub enabled: bool,
    pub label: String,
    pub sender: String,
    pub subject: String,
    pub handler: Handler,
}
impl Default for Rule {
    fn default() -> Self {
        Self {
            name: "Authentication codes".into(),
            enabled: true,
            label: String::new(),
            sender: String::new(),
            subject: String::new(),
            handler: Handler::AuthCode,
        }
    }
}
pub fn defaults() -> Vec<Rule> {
    vec![
        Rule::default(),
        Rule {
            name: "Inbox mail".into(),
            label: "INBOX".into(),
            handler: Handler::EmailNotification,
            ..Rule::default()
        },
    ]
}
pub fn load(store: &Store) -> anyhow::Result<Vec<Rule>> {
    Ok(store.get("notification_rules")?.unwrap_or_else(defaults))
}
pub fn save(store: &Store, rules: &[Rule]) -> anyhow::Result<()> {
    anyhow::ensure!(rules.len() <= 100, "At most 100 rules are supported");
    for rule in rules {
        anyhow::ensure!(!rule.name.trim().is_empty(), "Give each rule a name");
        anyhow::ensure!(
            [&rule.name, &rule.label, &rule.sender, &rule.subject]
                .iter()
                .all(|s| s.len() <= 512),
            "Rule fields must be at most 512 bytes"
        );
    }
    store.put("notification_rules", &rules)
}
impl Rule {
    fn matches(&self, message: &Message, labels: &[Label]) -> bool {
        let label = self.label.trim();
        self.enabled
            && (label.is_empty()
                || message.label_ids.iter().any(|id| {
                    id.eq_ignore_ascii_case(label)
                        || labels
                            .iter()
                            .any(|l| l.id == *id && l.name.eq_ignore_ascii_case(label))
                }))
            && message
                .header("From")
                .to_lowercase()
                .contains(&self.sender.trim().to_lowercase())
            && message
                .subject()
                .to_lowercase()
                .contains(&self.subject.trim().to_lowercase())
    }
}
pub enum Outcome {
    AuthCode(String),
    Email,
    Silent,
}
pub fn evaluate(rules: &[Rule], message: &Message, labels: &[Label]) -> Outcome {
    for rule in rules.iter().filter(|rule| rule.matches(message, labels)) {
        match rule.handler {
            Handler::AuthCode => {
                if let Some(code) = crate::auth_code::extract(message) {
                    return Outcome::AuthCode(code);
                }
            }
            Handler::EmailNotification => return Outcome::Email,
            Handler::Silent => return Outcome::Silent,
        }
    }
    Outcome::Silent
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Header, Part};
    fn message() -> Message {
        Message {
            label_ids: vec!["UNREAD".into(), "custom-label".into()],
            payload: Part {
                headers: vec![
                    Header {
                        name: "Subject".into(),
                        value: "Your verification code: 004281".into(),
                    },
                    Header {
                        name: "From".into(),
                        value: "Example <login@example.test>".into(),
                    },
                ],
                ..Default::default()
            },
            ..Default::default()
        }
    }
    #[test]
    fn defaults_are_account_independent_and_auth_replaces_email() {
        assert!(
            matches!(evaluate(&defaults(), &message(), &[]),Outcome::AuthCode(code) if code=="004281")
        );
        let mut mail = message();
        mail.payload.headers.clear();
        assert!(matches!(evaluate(&defaults(), &mail, &[]), Outcome::Silent));
        mail.label_ids.push("INBOX".into());
        assert!(matches!(evaluate(&defaults(), &mail, &[]), Outcome::Email));
    }
    #[test]
    fn filters_order_disable_and_roundtrip() {
        let mut rules = defaults();
        rules[0].label = "AUTH".into();
        rules[0].sender = "EXAMPLE.TEST".into();
        rules[0].subject = "verification".into();
        let labels = vec![Label {
            id: "custom-label".into(),
            name: "AUTH".into(),
            ..Default::default()
        }];
        assert!(matches!(
            evaluate(&rules, &message(), &labels),
            Outcome::AuthCode(_)
        ));
        assert!(matches!(evaluate(&rules, &message(), &[]), Outcome::Silent));
        rules.insert(
            0,
            Rule {
                handler: Handler::Silent,
                ..Rule::default()
            },
        );
        assert!(matches!(
            evaluate(&rules, &message(), &labels),
            Outcome::Silent
        ));
        rules[0].enabled = false;
        assert!(matches!(
            evaluate(&rules, &message(), &labels),
            Outcome::AuthCode(_)
        ));
        let store = Store::open(None).unwrap();
        save(&store, &rules).unwrap();
        assert_eq!(load(&store).unwrap().len(), 3);
        save(&store, &[]).unwrap();
        assert!(load(&store).unwrap().is_empty());
    }
}
