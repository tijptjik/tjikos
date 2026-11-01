//! Conservative, local-only recognition. Mail is data, never executable instructions.
use crate::model::Message;
use regex::Regex;
use std::{collections::BTreeMap, sync::LazyLock};
static CONTEXT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:verification|authori[sz]ation|security|authentication|confirmation|login|log.in|sign[ -]?(?:in|up)|one[ -]time|access|launch)(?:\s+\w+){0,3}\s+(?:code|pin)|passcode|\bOTP\b|驗證碼|验证码|認證碼|認証コード|安全碼|確認コード|인증.{0,6}(?:코드|번호)|(?:your|this|the) (?:[0-9]+ digit )?code|enter (?:this |the )?(?:[0-9]+ digit )?code").unwrap()
});
static TOKEN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?-u:\b)(?:[A-Z0-9]{3}-[A-Z0-9]{3}|[0-9]{4,8}|[A-Z0-9]{4,10})(?-u:\b)").unwrap()
});
static LINKS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"https?://\S+|\S+@\S+").unwrap());

pub fn extract(message: &Message) -> Option<String> {
    // Never copy a code from a quoted older message.
    let subject = message.subject();
    if subject.to_lowercase().starts_with("re:") || subject.to_lowercase().starts_with("fwd:") {
        return None;
    }
    let (body, _) = crate::quotes::body(message);
    extract_text(&subject, &body)
}
pub fn extract_text(subject: &str, body: &str) -> Option<String> {
    let subject = LINKS.replace_all(subject, " ");
    let body = LINKS.replace_all(body, " ");
    // html2text table borders and padding must not separate a code from its label.
    let body = body
        .lines()
        .map(|line| {
            line.trim_matches(|c: char| c.is_whitespace() || c == '│')
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        })
        .filter(|line| line.chars().any(char::is_alphanumeric))
        .collect::<Vec<_>>()
        .join("\n");
    let subject_context = CONTEXT.is_match(&subject);
    let mut scores = BTreeMap::<String, i32>::new();
    for (text, is_subject) in [(subject.as_ref(), true), (body.as_ref(), false)] {
        let contexts: Vec<_> = CONTEXT.find_iter(text).collect();
        for token in TOKEN.find_iter(text) {
            let code = token.as_str();
            if !code.bytes().any(|b| b.is_ascii_digit()) && !code.contains('-') {
                continue;
            }
            let before = &text[..token.start()];
            let after = &text[token.end()..];
            // Dates, amounts, URLs, tracking IDs, and phone fragments are not OTPs.
            if before.ends_with(['/', '.', '-', '$', '+', '_', '='])
                || after.starts_with(['/', '-', '_', '='])
                || (after.starts_with('.')
                    && after.as_bytes().get(1).is_some_and(u8::is_ascii_digit))
            {
                continue;
            }
            let line_start = before.rfind('\n').map_or(0, |i| i + 1);
            let line_end = after.find('\n').map_or(text.len(), |i| token.end() + i);
            let standalone = text[line_start..line_end].trim().trim_matches(['*', ' ']) == code;
            let distance = contexts
                .iter()
                .map(|c| {
                    if c.end() <= token.start() {
                        token.start() - c.end()
                    } else if token.end() <= c.start() {
                        c.start() - token.end()
                    } else {
                        0
                    }
                })
                .min();
            let nearby = distance.is_some_and(|d| d <= 100);
            if !nearby && !(subject_context && standalone) {
                continue;
            }
            if code.len() == 4 && !standalone && !is_subject && distance.is_none_or(|d| d > 20) {
                continue;
            }
            let score = if is_subject && subject_context {
                100
            } else if standalone && (subject_context || nearby) {
                90
            } else if nearby {
                80 - distance.unwrap_or(100).min(100) as i32 / 10
            } else {
                0
            };
            scores
                .entry(code.into())
                .and_modify(|s| *s = (*s).max(score))
                .or_insert(score);
        }
    }
    let best = scores.values().max().copied()?;
    let winners: Vec<_> = scores
        .into_iter()
        .filter(|(_, score)| *score == best)
        .collect();
    (winners.len() == 1).then(|| winners[0].0.clone())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn synthetic_auth_and_negative_cases() {
        for (subject, body, expected) in [
            (
                "Your verification code: 004281",
                "Expires in 10 minutes",
                Some("004281"),
            ),
            (
                "Sign in to Example",
                "Enter your code:\n\n832940\n\nDo not share it.",
                Some("832940"),
            ),
            ("Your login code", "ABC-123", Some("ABC-123")),
            ("驗證碼", "您的驗證碼為 123456", Some("123456")),
            (
                "Your authorisation code",
                "Here is your authorisation code:\n\n003942",
                Some("003942"),
            ),
            (
                "Your verification code",
                "Your verification code is 004281.",
                Some("004281"),
            ),
            (
                "Your GitHub launch code",
                "Enter the code below:\n\n12458901",
                Some("12458901"),
            ),
            (
                "Email verification",
                "Here is your verification code:\n──────────────────\n  004281   \n────────────────",
                Some("004281"),
            ),
            (
                "Validate email address",
                "Please enter this 4 digit code:\n**AB3D**",
                Some("AB3D"),
            ),
            (
                "Verification successful",
                "Your business is verified. Reference: 123456.",
                None,
            ),
            ("Your one-time code", "ABC-DEF", Some("ABC-DEF")),
            ("Receipt 123456", "Your order total is $1234.00", None),
            (
                "Security alert",
                "Login from 192.168.1.100 at 2026-11-01",
                None,
            ),
            (
                "Verification link",
                "Click https://example.test/verify/123456",
                None,
            ),
            ("Verification code", "123456\n654321", None),
        ] {
            assert_eq!(
                extract_text(subject, body).as_deref(),
                expected,
                "{subject}"
            );
        }
    }
    #[test]
    fn private_corpus() {
        let Ok(path) = std::env::var("GMAILTJIK_AUTH_CORPUS") else {
            return;
        };
        let messages: Vec<Message> =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        if let Ok(expected_path) = std::env::var("GMAILTJIK_AUTH_EXPECTED") {
            let expected: BTreeMap<String, Option<String>> =
                serde_json::from_slice(&std::fs::read(expected_path).unwrap()).unwrap();
            assert_eq!(expected.len(), messages.len());
            for (index, message) in messages.iter().enumerate() {
                assert!(
                    expected.contains_key(&message.id),
                    "Missing reviewed case {index}"
                );
                assert!(
                    extract(message) == expected[&message.id],
                    "Reviewed case {index} mismatch (private values omitted)"
                );
            }
        }
        let report:Vec<_>=messages.iter().map(|m|serde_json::json!({"id":m.id,"subject":m.subject(),"text":crate::quotes::body(m).0,"code":extract(m)})).collect();
        let audit = std::path::Path::new(&path).with_extension("audit.json");
        crate::store::write_private(&audit, &serde_json::to_vec(&report).unwrap()).unwrap();
        let found = messages.iter().filter(|m| extract(m).is_some()).count();
        eprintln!(
            "Private AUTH validation: {found}/{} extracted (codes omitted)",
            messages.len()
        );
    }
}
