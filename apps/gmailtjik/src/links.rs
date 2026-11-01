use anyhow::{Context, Result, ensure};

#[derive(Clone, Debug, PartialEq)]
pub struct MailLink {
    pub account: String,
    pub message: String,
    pub thread: String,
}
impl MailLink {
    pub fn url(&self) -> String {
        let mut url = url::Url::parse("gmailtjik://mail/open").unwrap();
        url.query_pairs_mut()
            .append_pair("account", &self.account)
            .append_pair("message", &self.message)
            .append_pair("thread", &self.thread);
        url.into()
    }
    pub fn parse(value: &str) -> Result<Self> {
        let url = url::Url::parse(value)?;
        ensure!(
            url.scheme() == "gmailtjik"
                && url.host_str() == Some("mail")
                && url.path() == "/open"
                && url.username().is_empty()
                && url.password().is_none()
                && url.port().is_none()
                && url.fragment().is_none(),
            "Invalid email link"
        );
        let pairs: Vec<_> = url.query_pairs().collect();
        ensure!(pairs.len() == 3, "Invalid email link fields");
        let field = |name| -> Result<String> {
            let values: Vec<_> = pairs.iter().filter(|(k, _)| k == name).collect();
            ensure!(values.len() == 1, "Missing or duplicate email link field");
            Ok(values[0].1.to_string())
        };
        let link = Self {
            account: field("account")?,
            message: field("message")?,
            thread: field("thread")?,
        };
        ensure!(
            link.account.len() <= 320
                && link.account.contains('@')
                && !link.account.chars().any(char::is_control),
            "Invalid email account"
        );
        for id in [&link.message, &link.thread] {
            ensure!(
                !id.is_empty()
                    && id.len() <= 128
                    && id
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_'),
                "Invalid Gmail identity"
            );
        }
        Ok(link)
    }
    pub fn check_account(&self, account: &str) -> Result<()> {
        ensure!(
            self.account.eq_ignore_ascii_case(account),
            "This email belongs to a different Gmail account ({})",
            self.account
        );
        Ok(())
    }
    pub fn select(&self, messages: Vec<crate::model::Message>) -> Result<crate::model::Message> {
        messages
            .into_iter()
            .find(|m| m.id == self.message)
            .context("Linked email is no longer available")
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn links_roundtrip_and_reject_ambiguous_targets() {
        let link = MailLink {
            account: "one+tag@example.test".into(),
            message: "abc123".into(),
            thread: "def456".into(),
        };
        assert_eq!(MailLink::parse(&link.url()).unwrap(), link);
        assert!(link.check_account("other@example.test").is_err());
        assert!(MailLink::parse(&(link.url() + "&message=other")).is_err());
        assert!(MailLink::parse(&link.url().replace("abc123", "..%2Fbad")).is_err());
        assert!(MailLink::parse(&link.url().replace("gmailtjik:", "https:")).is_err());
    }
}
