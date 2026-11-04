//! Sender-supplied RFC 2369 links. No request is made until the user opens one.
#[derive(Clone, Debug, PartialEq)]
pub struct Target {
    pub url: String,
    pub destination: String,
    pub email: bool,
}

pub fn targets(header: &str) -> Vec<Target> {
    let mut targets = Vec::new();
    for part in header.split('<').skip(1) {
        let Some((value, _)) = part.split_once('>') else {
            continue;
        };
        let value = value.trim();
        if value.chars().any(char::is_control) {
            continue;
        }
        let Ok(url) = url::Url::parse(value) else {
            continue;
        };
        let email = url.scheme() == "mailto";
        let destination = match url.scheme() {
            "https" | "http" if url.username().is_empty() && url.password().is_none() => {
                let Some(host) = url.host_str() else { continue };
                format!("{}://{}", url.scheme(), host)
            }
            "mailto" if !url.path().is_empty() && url.path().contains('@') => {
                url.path().to_string()
            }
            _ => continue,
        };
        if !targets
            .iter()
            .any(|target: &Target| target.url == url.as_str())
        {
            targets.push(Target {
                url: url.to_string(),
                destination,
                email,
            });
        }
    }
    targets
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn extracts_sender_links_without_executing_or_losing_tokens() {
        let links = targets(
            "<mailto:leave@example.test?subject=unsubscribe>, <https://example.test/leave?token=a%2Bb>",
        );
        assert_eq!(links.len(), 2);
        assert!(links[0].email);
        assert_eq!(links[1].destination, "https://example.test");
        assert!(links[1].url.ends_with("token=a%2Bb"));
    }
    #[test]
    fn rejects_unsafe_schemes_credentials_and_malformed_headers() {
        assert!(targets("<javascript:alert(1)>, <file:///tmp/mail>, <https://user:secret@example.test>, <mailto:>, <https://exa\nmple.test>").is_empty());
        assert!(targets("https://example.test/no-angle-brackets").is_empty());
        assert_eq!(
            targets("<https://example.test>, <https://example.test>").len(),
            1
        );
    }
}
