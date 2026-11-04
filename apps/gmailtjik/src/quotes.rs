use crate::model::Message;

/// Remove known quote containers and the complete trailing history after
/// Outlook/Apple/Gmail reply headers, even when it crosses wrapper boundaries.
pub fn strip_html(doc: &mut scraper::Html) -> bool {
    let mut hidden = false;
    // An explicit container owns its boundary: inline answers after it are new
    // content, so remove these before considering uncontained trailing history.
    let selector = scraper::Selector::parse(
        ".gmail_quote, .yahoo_quoted, .protonmail_quote, .moz-cite-prefix, blockquote[type=cite]",
    )
    .unwrap();
    let quotes: Vec<_> = doc
        .root_element()
        .select(&selector)
        .map(|node| node.id())
        .collect();
    hidden |= !quotes.is_empty();
    for id in quotes {
        doc.tree.get_mut(id).unwrap().detach();
    }
    let boundaries: Vec<_> = doc
        .root_element()
        .select(&scraper::Selector::parse("div, p, hr").unwrap())
        .filter(|node| {
            if node
                .attr("id")
                .is_some_and(|id| id.eq_ignore_ascii_case("divRplyFwdMsg"))
            {
                return true;
            }
            let text = node.text().collect::<Vec<_>>().join(" ");
            let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
            let lower = text.to_lowercase();
            let headers = lower.starts_with("from:")
                && lower.contains("sent:")
                && lower.contains("to:")
                && lower.contains("subject:");
            let attribution = text.len() < 600
                && text.starts_with("On ")
                && text.ends_with("wrote:")
                && text.contains('@');
            headers || attribution
        })
        .map(|node| node.id())
        .collect();
    for id in boundaries {
        let Some(node) = doc.tree.get(id) else {
            continue;
        };
        if !node
            .ancestors()
            .any(|node| node.id() == doc.root_element().id())
        {
            continue;
        }
        let mut cursor = id;
        loop {
            let node = doc.tree.get(cursor).unwrap();
            let parent = node.parent().map(|node| node.id());
            let following: Vec<_> = node.next_siblings().map(|node| node.id()).collect();
            for sibling in following {
                doc.tree.get_mut(sibling).unwrap().detach();
            }
            if cursor == id {
                doc.tree.get_mut(cursor).unwrap().detach();
            }
            let Some(parent) = parent else { break };
            if doc
                .tree
                .get(parent)
                .and_then(|node| node.value().as_element())
                .is_some_and(|element| matches!(element.name(), "body" | "html"))
            {
                break;
            }
            cursor = parent;
        }
        hidden = true;
    }
    hidden
}

/// Collapse recognizable reply quotations without discarding inline replies.
pub fn body(message: &Message) -> (String, bool) {
    let full = message.text();
    let mut kept = Vec::new();
    let lines: Vec<_> = full.lines().collect();
    let mut hidden = false;
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index].trim_start();
        if line.starts_with('>') {
            hidden = true;
            index += 1;
            continue;
        }
        if line.starts_with("On ") {
            let end = (index + 5).min(lines.len());
            if let Some(last) = (index..end).find(|&n| lines[n].trim_end().ends_with("wrote:")) {
                if lines[last + 1..]
                    .iter()
                    .find(|line| !line.trim().is_empty())
                    .is_some_and(|line| line.trim_start().starts_with('>'))
                {
                    hidden = true;
                    index = last + 1;
                    continue;
                }
            }
        }
        if line.to_ascii_lowercase().starts_with("from:") && line.contains('@') {
            let header = lines[index..]
                .iter()
                .take(12)
                .map(|line| line.trim().to_ascii_lowercase())
                .collect::<Vec<_>>();
            if ["sent:", "to:", "subject:"]
                .iter()
                .all(|label| header.iter().any(|line| line.starts_with(label)))
            {
                hidden = true;
                break;
            }
        }
        if !message.header("In-Reply-To").is_empty()
            && (line.eq_ignore_ascii_case("-----Original Message-----")
                || line.eq_ignore_ascii_case("________________________________"))
            && lines[index + 1..]
                .iter()
                .take(8)
                .any(|line| line.trim_start().starts_with("From:"))
            && lines[index + 1..]
                .iter()
                .take(8)
                .any(|line| line.trim_start().starts_with("Subject:"))
        {
            hidden = true;
            break;
        }
        kept.push(lines[index]);
        index += 1;
    }
    if hidden {
        return (kept.join("\n").trim().to_string(), true);
    }

    // Some clients identify quote history only in the HTML alternative.
    let parts = message.payload.walk();
    let mut texts = Vec::new();
    for part in parts
        .into_iter()
        .filter(|part| part.mime_type == "text/html" && part.filename.is_empty())
    {
        let mut html = scraper::Html::parse_document(&part.decoded_text());
        hidden |= strip_html(&mut html);
        if let Ok(text) = html2text::from_read(html.html().as_bytes(), 90) {
            texts.push(text);
        }
    }
    if hidden {
        (texts.join("\n\n").trim().into(), true)
    } else {
        (full, false)
    }
}

#[cfg(test)]
mod tests {
    use super::body;
    use crate::model::{Body, Message, Part};
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
    fn mail(mime: &str, text: &str) -> Message {
        Message {
            payload: Part {
                mime_type: mime.into(),
                body: Body {
                    data: URL_SAFE_NO_PAD.encode(text),
                    ..Default::default()
                },
                ..Default::default()
            },
            ..Default::default()
        }
    }
    #[test]
    fn preserves_answer_after_explicit_quote_container() {
        let mut doc = scraper::Html::parse_document(
            "<p>First answer</p><div class='gmail_quote'><p>On Monday sender@example.test wrote:</p><p>Old question</p></div><p>Inline answer after quote</p>",
        );
        assert!(super::strip_html(&mut doc));
        assert!(
            doc.html().contains("First answer") && doc.html().contains("Inline answer after quote")
        );
        assert!(!doc.html().contains("Old question"));
    }
    #[test]
    fn removes_complete_outlook_history_across_wrappers() {
        for header in [
            "<div id='divRplyFwdMsg'>From: Sender</div>",
            "<p><b>From:</b> Sender &lt;sender@example.test&gt;<br><b>Sent:</b> Monday<br><b>To:</b> Reader<br><b>Subject:</b> Earlier note</p>",
        ] {
            let source = format!(
                "<div><p>New answer</p><div>{header}<p>Old body</p></div><img src='cid:old'></div><p>Older nested history</p>"
            );
            let mut doc = scraper::Html::parse_document(&source);
            assert!(super::strip_html(&mut doc));
            let result = doc.html();
            assert!(result.contains("New answer"));
            assert!(
                !result.contains("Old body")
                    && !result.contains("cid:old")
                    && !result.contains("Older nested history")
            );
            assert!(!super::strip_html(&mut doc));
        }
    }
    #[test]
    fn removes_attribution_and_history_but_preserves_ordinary_quotes() {
        let mut doc = scraper::Html::parse_document(
            "<p>Answer</p><blockquote>A relevant quotation</blockquote><p>On Monday, Sender &lt;sender@example.test&gt; wrote:</p><blockquote>Old</blockquote><p>Older</p>",
        );
        assert!(super::strip_html(&mut doc));
        assert!(doc.html().contains("A relevant quotation"));
        assert!(!doc.html().contains("wrote:") && !doc.html().contains("Old"));
    }
    #[test]
    fn strips_plain_outlook_header_and_all_trailing_history() {
        let message = mail(
            "text/plain",
            "New answer\n\nFrom: sender@example.test\nSent: Monday\nTo: reader@example.test\nSubject: Earlier\nOld body\nOlder history",
        );
        assert_eq!(body(&message), ("New answer".into(), true));
    }
    #[test]
    fn preserves_inline_answers_and_can_reveal_original() {
        let message = mail(
            "text/plain",
            "Yes!\n\nOn Monday, Mira wrote:\n> First question\nMy answer\n> Second question\nAnother answer",
        );
        let (text, hidden) = body(&message);
        assert!(hidden);
        assert_eq!(text, "Yes!\n\nMy answer\nAnother answer");
        assert!(message.text().contains("First question"));
    }
    #[test]
    fn collapses_html_quote_containers_but_keeps_new_content() {
        let message = mail(
            "text/html",
            "<p>New answer</p><div class='gmail_quote'>Old body</div>",
        );
        let (text, hidden) = body(&message);
        assert!(hidden);
        assert!(text.contains("New answer"));
        assert!(!text.contains("Old body"));
    }
    #[test]
    fn uses_html_quote_boundaries_when_plain_alternative_has_none() {
        let mut message = mail("multipart/alternative", "");
        message.payload.parts = vec![
            mail("text/plain", "New answer
Old body without quote markers").payload,
            mail("text/html", "<p>New answer<img src='https://example.test/new.png'></p><div class='gmail_quote'>Old body<img src='https://example.test/old.png'></div>").payload,
        ];
        let (text, hidden) = body(&message);
        assert!(hidden);
        assert!(text.contains("New answer"));
        assert!(!text.contains("Old body"));
        assert_eq!(message.remote_images().len(), 1);
        assert_eq!(message.remote_images()[0].0, "https://example.test/new.png");
    }
    #[test]
    fn leaves_unrecognized_prose_and_forwarded_mail_intact() {
        let message = mail(
            "text/plain",
            "On Monday we wrote:\nA fresh paragraph\n----- Forwarded message -----\nUseful forwarded content",
        );
        assert_eq!(body(&message), (message.text(), false));
    }
}
