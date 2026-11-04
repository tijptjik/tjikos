//! Conservative display-only signature folding. The caller retains the source.
use scraper::{ElementRef, Html, Node, Selector};

pub struct HtmlSignature {
    pub collapsed: String,
    pub detected: bool,
}

/// Gmail's signature-image endpoint is distinct from ordinary hosted images.
pub fn signature_image(source: &str) -> bool {
    url::Url::parse(source.trim()).ok().is_some_and(|url| {
        url.scheme() == "https"
            && url.host_str().is_some_and(|host| {
                host == "googleusercontent.com" || host.ends_with(".googleusercontent.com")
            })
            && url.path().starts_with("/mail-sig/")
    })
}

fn quote(element: ElementRef<'_>) -> bool {
    element.value().name() == "blockquote"
        || element.value().classes().any(|class| {
            matches!(
                class.to_ascii_lowercase().as_str(),
                "gmail_quote" | "yahoo_quoted"
            )
        })
}

fn in_quote(element: ElementRef<'_>) -> bool {
    element.ancestors().filter_map(ElementRef::wrap).any(quote)
}

fn marked(element: ElementRef<'_>) -> bool {
    element.value().classes().any(|class| {
        matches!(
            class.to_ascii_lowercase().as_str(),
            "gmail_signature" | "applemailsignature" | "moz-signature" | "outlook-signature"
        )
    }) || element
        .attr("data-smartmail")
        .is_some_and(|value| value.eq_ignore_ascii_case("gmail_signature"))
        || element.attr("id").is_some_and(|value| {
            matches!(
                value.to_ascii_lowercase().as_str(),
                "signature" | "ms-outlook-mobile-signature"
            )
        })
}

fn signoff(text: &str) -> bool {
    matches!(
        text.trim()
            .trim_end_matches(['.', ',', '!', ':', ';', '…'])
            .trim()
            .to_lowercase()
            .as_str(),
        "thanks"
            | "thank you"
            | "thank you very much"
            | "many thanks"
            | "thanks a lot"
            | "cheers"
            | "regards"
            | "kind regards"
            | "best regards"
            | "warm regards"
            | "best"
            | "all the best"
            | "sincerely"
            | "yours sincerely"
            | "groeten"
            | "groetjes"
            | "mvg"
            | "met vriendelijke groet"
            | "met vriendelijke groeten"
            | "vriendelijke groeten"
    )
}

/// An explicit closing followed by a name starts a signature even when its
/// address/social footer cannot be classified line by line.
fn named_signoff(text: &str) -> bool {
    matches!(
        text.trim()
            .trim_end_matches(['.', ',', '!', ':', ';', '…'])
            .trim()
            .to_lowercase()
            .as_str(),
        "regards"
            | "cheers"
            | "best regards"
            | "kind regards"
            | "kind regars"
            | "best"
            | "all the best"
            | "have a lovely day"
            | "have a nice day"
    )
}

fn thanks(text: &str) -> bool {
    matches!(
        text.trim()
            .trim_end_matches(['.', ',', '!', ':', ';', '…'])
            .trim()
            .to_lowercase()
            .as_str(),
        "thanks" | "thank you" | "thank you very much" | "thanks a lot" | "many thanks"
    )
}

// Include consecutive closing courtesies immediately before the signature.
fn closing_start(lines: &[String], start: usize) -> usize {
    let mut result = start;
    for index in (0..start).rev() {
        if lines[index].trim().is_empty() {
            continue;
        }
        if thanks(&lines[index]) && lines[..index].iter().any(|line| !line.trim().is_empty()) {
            result = index;
        } else {
            break;
        }
    }
    result
}

fn postscript(text: &str) -> bool {
    let text = text.trim().to_ascii_lowercase();
    if text.starts_with("...") || text.starts_with('…') {
        return true;
    }
    ["p.s.", "p.p.s.", "ps.", "pps.", "p.s", "p.p.s", "ps", "pps"]
        .iter()
        .any(|prefix| {
            text.strip_prefix(prefix).is_some_and(|rest| {
                rest.is_empty()
                    || rest.starts_with(|c: char| c.is_whitespace() || ":.,!-—".contains(c))
                    || prefix.ends_with('.')
            })
        })
}

fn reply_boundary(text: &str) -> bool {
    let text = text.trim();
    text.starts_with('>')
        || (text.starts_with("On ") && text.ends_with("wrote:"))
        || text.eq_ignore_ascii_case("-----Original Message-----")
        || text.eq_ignore_ascii_case("----- Forwarded message -----")
}

fn mobile(text: &str) -> bool {
    matches!(
        text.trim()
            .trim_end_matches('.')
            .to_ascii_lowercase()
            .as_str(),
        "sent from my iphone"
            | "sent from my ipad"
            | "sent from my android"
            | "sent from my samsung galaxy smartphone"
            | "verzonden vanaf mijn iphone"
            | "verzonden vanaf mijn ipad"
    )
}

fn contact(text: &str) -> bool {
    let text = text.trim();
    let stripped = text
        .split_once(':')
        .filter(|(label, _)| {
            matches!(
                label.trim().to_ascii_lowercase().as_str(),
                "email"
                    | "e-mail"
                    | "e"
                    | "m"
                    | "t"
                    | "tel"
                    | "telephone"
                    | "phone"
                    | "mobile"
                    | "web"
                    | "website"
            )
        })
        .map_or(text, |(_, value)| value.trim());
    if let Some((local, host)) = stripped.trim_matches(['<', '>']).split_once('@') {
        return !local.is_empty()
            && host.contains('.')
            && !host.ends_with('.')
            && !stripped.chars().any(char::is_whitespace);
    }
    if url::Url::parse(stripped)
        .ok()
        .is_some_and(|url| matches!(url.scheme(), "http" | "https") && url.host_str().is_some())
        && !stripped.chars().any(char::is_whitespace)
    {
        return true;
    }
    let digits = stripped.chars().filter(char::is_ascii_digit).count();
    (7..=18).contains(&digits)
        && stripped
            .chars()
            .all(|c| c.is_ascii_digit() || "+().- /".contains(c))
}

fn identity(text: &str) -> bool {
    let text = text.trim();
    let words: Vec<_> = text.split_whitespace().collect();
    if words.is_empty()
        || words.len() > 6
        || text.chars().count() > 80
        || text.contains(['?', '!', ';', ':', '@'])
        || text.ends_with('.')
        || !text.chars().any(char::is_alphabetic)
    {
        return false;
    }
    let first = words[0].to_lowercase();
    if matches!(
        first.as_str(),
        "please"
            | "can"
            | "could"
            | "would"
            | "i"
            | "we"
            | "you"
            | "your"
            | "the"
            | "this"
            | "that"
            | "here"
            | "note"
            | "next"
            | "if"
            | "remember"
            | "see"
            | "let"
            | "send"
            | "call"
            | "contact"
            | "reach"
            | "reply"
            | "check"
            | "confirm"
            | "do"
            | "don't"
            | "also"
            | "and"
            | "but"
            | "before"
            | "after"
    ) {
        return false;
    }
    text.chars()
        .next()
        .is_some_and(|c| c.is_uppercase() || c.is_ascii_digit())
}

/// Find a bounded signature slice, retaining a subsequent postscript and quotes.
fn signature_range(lines: &[String], end: usize, sender: &str) -> Option<std::ops::Range<usize>> {
    for start in (0..end).rev() {
        let line = lines[start].trim();
        let standard_separator = lines[start].trim_start() == "-- ";
        if !lines[..start].iter().any(|line| !line.trim().is_empty()) {
            continue;
        }
        if mobile(line)
            && lines[start + 1..end]
                .iter()
                .all(|line| line.trim().is_empty())
        {
            return Some(closing_start(lines, start)..end);
        }
        if line != "--" && !signoff(line) && !named_signoff(line) {
            continue;
        }
        let end = (start + 1..end)
            .find(|&n| postscript(&lines[n]))
            .unwrap_or(end);
        let tail: Vec<_> = lines[start + 1..end]
            .iter()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();
        if named_signoff(line) && tail.first().is_some_and(|name| identity(name)) {
            return Some(closing_start(lines, start)..end);
        }
        if thanks(line) && (tail.is_empty() || (tail.len() == 1 && identity(tail[0]))) {
            return Some(closing_start(lines, start)..end);
        }
        if tail.is_empty() || tail.len() > 12 || tail.iter().map(|s| s.len()).sum::<usize>() > 700 {
            continue;
        }
        let contacts = tail.iter().filter(|line| contact(line)).count();
        let identities = tail.iter().filter(|line| identity(line)).count();
        if ((contacts > 0 && (identities > 0 || contacts > 1))
            || (standard_separator && identities > 0))
            && tail.iter().all(|line| contact(line) || identity(line))
        {
            return Some(closing_start(lines, start)..end);
        }
    }
    let (name, email) = crate::model::addresses(sender).ok()?.into_iter().next()?;
    if name.split_whitespace().count() < 2 || email.is_empty() {
        return None;
    }
    for start in (1..end).rev() {
        if !lines[start].trim().eq_ignore_ascii_case(name.trim()) {
            continue;
        }
        let stop = (start + 1..end)
            .find(|&n| postscript(&lines[n]))
            .unwrap_or(end);
        let tail = &lines[start + 1..stop];
        if tail.len() > 24 || tail.iter().map(String::len).sum::<usize>() > 1600 {
            continue;
        }
        let email_matches = tail.iter().any(|line| {
            line.split_whitespace().any(|word| {
                word.trim_matches(['<', '>', ',', ';'])
                    .eq_ignore_ascii_case(&email)
            })
        });
        let phone = tail.iter().any(|line| {
            let line = line
                .trim()
                .trim_end_matches("(Direct Dial)")
                .trim_end_matches("(direct dial)")
                .trim();
            let digits = line.chars().filter(char::is_ascii_digit).count();
            digits >= 7 && contact(line)
        });
        if email_matches && phone {
            return Some(start..stop);
        }
    }
    None
}

#[cfg(test)]
pub fn plain(text: &str) -> Option<String> {
    plain_for_sender(text, "")
}

pub fn plain_for_sender(text: &str, sender: &str) -> Option<String> {
    let lines: Vec<_> = text.lines().map(str::to_owned).collect();
    let end = lines
        .iter()
        .enumerate()
        .position(|(index, line)| reply_boundary(line) || crate::quotes::header_at(&lines, index))
        .unwrap_or(lines.len());
    let range = signature_range(&lines, end, sender)?;
    Some(
        lines[..range.start]
            .iter()
            .chain(&lines[range.end..])
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
            .trim()
            .to_string(),
    )
}

enum Piece<'a> {
    Text(ElementRef<'a>, usize),
    Element(ElementRef<'a>),
}

#[derive(Default)]
struct Line<'a> {
    text: String,
    pieces: Vec<Piece<'a>>,
    has_image: bool,
    boundary: bool,
}

fn flush<'a>(current: &mut Line<'a>, lines: &mut Vec<Line<'a>>) {
    current.text = current
        .text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if !current.text.is_empty() || current.has_image || current.boundary {
        lines.push(std::mem::take(current));
    } else {
        *current = Line::default();
    }
}

fn collect<'a>(
    element: ElementRef<'a>,
    current: &mut Line<'a>,
    lines: &mut Vec<Line<'a>>,
    depth: usize,
) {
    if depth > 100 {
        return;
    }
    let tag = element.value().name();
    if quote(element) {
        flush(current, lines);
        lines.push(Line {
            boundary: true,
            ..Line::default()
        });
        return;
    }
    if matches!(
        tag,
        "head" | "script" | "style" | "pre" | "code" | "textarea"
    ) {
        return;
    }
    if tag == "br" {
        current.pieces.push(Piece::Element(element));
        flush(current, lines);
        return;
    }
    if tag == "img" {
        current.pieces.push(Piece::Element(element));
        current.has_image |= element.attr("src").is_some();
        return;
    }
    let block = matches!(
        tag,
        "p" | "div"
            | "section"
            | "table"
            | "tr"
            | "td"
            | "th"
            | "li"
            | "ul"
            | "ol"
            | "address"
            | "footer"
            | "h1"
            | "h2"
            | "h3"
    );
    if block {
        flush(current, lines);
    }
    for (index, node) in element.children().enumerate() {
        match node.value() {
            Node::Text(text) => {
                current.text.push_str(text);
                current.pieces.push(Piece::Text(element, index));
            }
            Node::Element(_) => collect(ElementRef::wrap(node).unwrap(), current, lines, depth + 1),
            _ => {}
        }
    }
    if block {
        flush(current, lines);
    }
}

#[cfg(test)]
pub fn html(source: &str) -> HtmlSignature {
    html_for_sender(source, "")
}

pub fn html_for_sender(source: &str, sender: &str) -> HtmlSignature {
    let mut doc = Html::parse_document(source);
    let all = Selector::parse("*").unwrap();
    let images = Selector::parse("img[src]").unwrap();
    let mut removed = Vec::new();
    // Marked containers are reliable, but leave quoted signatures alone.
    for element in doc.root_element().select(&all) {
        if in_quote(element) || quote(element) {
            continue;
        }
        if marked(element) {
            // Never collapse a container containing a recognisable postscript.
            let mut lines = Vec::new();
            let mut current = Line::default();
            collect(element, &mut current, &mut lines, 0);
            flush(&mut current, &mut lines);
            if lines
                .iter()
                .any(|line| line.boundary || postscript(&line.text))
            {
                continue;
            }
            removed.push(element.id());
        }
    }
    let mut affected_anchors: Vec<_> = removed
        .iter()
        .flat_map(|id| doc.tree.get(*id).unwrap().ancestors())
        .filter_map(ElementRef::wrap)
        .filter(|element| element.value().name() == "a")
        .map(|element| element.id())
        .collect();
    for id in &removed {
        doc.tree.get_mut(*id).unwrap().detach();
    }
    let mut lines = Vec::new();
    let mut current = Line::default();
    collect(doc.root_element(), &mut current, &mut lines, 0);
    flush(&mut current, &mut lines);
    let texts: Vec<_> = lines.iter().map(|line| line.text.clone()).collect();
    let end = lines
        .iter()
        .enumerate()
        .position(|(index, line)| {
            line.boundary || reply_boundary(&line.text) || crate::quotes::header_at(&texts, index)
        })
        .unwrap_or(lines.len());
    if let Some(range) = signature_range(&texts, end, sender) {
        for line in &lines[range] {
            for piece in &line.pieces {
                match piece {
                    Piece::Text(parent, index) => {
                        if let Some(node) = parent.children().nth(*index) {
                            removed.push(node.id());
                        }
                    }
                    Piece::Element(element) => removed.push(element.id()),
                }
            }
        }
    }
    // A Gmail signature logo is safe to fold even when its surrounding contact
    // layout is too ambiguous to classify. Ordinary logos are not classified.
    for image in doc.root_element().select(&images) {
        if !in_quote(image) && image.attr("src").is_some_and(signature_image) {
            removed.push(image.id());
        }
    }
    let detected = !removed.is_empty();
    for id in &removed {
        for anchor in doc
            .tree
            .get(*id)
            .unwrap()
            .ancestors()
            .filter_map(ElementRef::wrap)
            .filter(|element| element.value().name() == "a")
        {
            if !affected_anchors.contains(&anchor.id()) {
                affected_anchors.push(anchor.id());
            }
        }
    }
    for id in removed {
        doc.tree.get_mut(id).unwrap().detach();
    }
    // A now-empty link can otherwise expose its signature URL as a fallback
    // label. Only inspect links that actually lost signature content.
    for id in affected_anchors {
        let anchor = ElementRef::wrap(doc.tree.get(id).unwrap()).unwrap();
        let has_content = anchor.text().any(|text| !text.trim().is_empty())
            || anchor
                .descendants()
                .filter_map(ElementRef::wrap)
                .any(|element| {
                    matches!(
                        element.value().name(),
                        "img"
                            | "svg"
                            | "picture"
                            | "video"
                            | "audio"
                            | "object"
                            | "embed"
                            | "iframe"
                            | "canvas"
                            | "math"
                    )
                });
        if !has_content {
            doc.tree.get_mut(id).unwrap().detach();
        }
    }
    HtmlSignature {
        collapsed: if detected {
            format!("<!doctype html>{}", doc.root_element().html())
        } else {
            source.to_string()
        },
        detected,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sender_name_requires_matching_email_and_phone_before_folding() {
        let source = "Your status is unclaimed.\nPlease contact the administrator.\nMira Chen\nExample Acquisition\n1384 Example Street, Suite 906\nNew York, NY 10018\n212-555-4333 (Direct Dial)\nmira@example.test\nTelegram: @example\nActive in: Claims, finance.\nPS: Keep this note.";
        let sender = "Mira Chen <mira@example.test>";
        let folded = plain_for_sender(source, sender).unwrap();
        assert!(
            folded.contains("Please contact the administrator.")
                && folded.contains("PS: Keep this note.")
        );
        assert!(!folded.contains("Example Acquisition"));
        assert!(plain_for_sender(source, "Other Person <other@example.test>").is_none());
        assert!(
            plain_for_sender(
                &source.replace("212-555-4333 (Direct Dial)", "No telephone"),
                sender
            )
            .is_none()
        );
        assert!(
            plain_for_sender(
                &source.replace("mira@example.test", "other@example.test"),
                sender
            )
            .is_none()
        );
        let html = source
            .lines()
            .map(|line| format!("<p>{line}</p>"))
            .collect::<String>();
        let folded = html_for_sender(&html, sender);
        assert!(folded.detected && !folded.collapsed.contains("Example Acquisition"));
        assert!(folded.collapsed.contains("PS: Keep this note."));
    }

    #[test]
    fn additional_closings_and_thanks_preserve_postscripts() {
        for closing in [
            "Best,",
            "Have a lovely day,",
            "Thank you very much.",
            "Many thanks!",
        ] {
            let source = format!(
                "Please review the books.\n\n{closing}\nMira\n\nP.S. Keep this instruction."
            );
            assert_eq!(
                plain(&source),
                Some("Please review the books.\n\nP.S. Keep this instruction.".into())
            );
            let source = source
                .lines()
                .map(|line| format!("<p>{line}</p>"))
                .collect::<String>();
            let result = html(&source);
            assert!(result.detected && !result.collapsed.contains("Mira"));
            assert!(result.collapsed.contains("Keep this instruction"));
        }
        assert_eq!(
            plain("Please review the books.\n\nThank you very much."),
            Some("Please review the books.".into())
        );
        assert!(plain("Thank you very much.").is_none());
        assert!(
            plain("Please review the books.\nThank you very much.\nPlease send feedback tomorrow.")
                .is_none()
        );
    }

    #[test]
    fn current_signature_stops_before_german_history() {
        let source = "<p>Any potential books?</p><p>Many thanks!</p><p>Best regards<br>Mira</p><p>Head of Library<br>Example Institute<br>Website: https://example.test</p><hr><p><b>Von:</b> Earlier &lt;earlier@example.test&gt;<br><b>Gesendet:</b> Monday<br><b>An:</b> Reader<br><b>Betreff:</b> Earlier note</p><p>Old body</p><p>Regards,<br>Alex<br>Old Institute</p>";
        let result = html(source);
        assert!(result.detected);
        for removed in ["Many thanks", "Best regards", "Mira", "Head of Library"] {
            assert!(!result.collapsed.contains(removed), "{}", result.collapsed);
        }
        assert!(result.collapsed.contains("Old body") && result.collapsed.contains("Alex"));
        let mut doc = Html::parse_document(&result.collapsed);
        assert!(crate::quotes::strip_html(&mut doc));
        assert!(doc.html().contains("Any potential books?"));
        assert!(!doc.html().contains("Old body"));
    }

    #[test]
    fn named_closings_fold_name_only_and_unstructured_footers_preserving_postscripts() {
        for closing in [
            "Regards,",
            "Cheers,",
            "Best Regards,",
            "Kind Regards,",
            "Kind Regars,",
        ] {
            for ps in ["PS", "P.s.", "PS:", "P.S:", "PPS", "P.P.S.", "...", "…"] {
                let footer = "Mira Chen\nCultural Centre\nSuite 304, 30/F Example Plaza\n18, Example Road\nWebsite: https://example.test/\nFacebook: Cultural Centre\nInstagram: @example";
                let source = format!(
                    "The chosen book is The Letter Carrier.\n\n{closing}\n\n{footer}\n\n{ps}\nPlease bring the revised edition.\nA second postscript line."
                );
                let folded = plain(&source).unwrap();
                assert!(
                    !folded.contains("Mira Chen") && !folded.contains("Instagram"),
                    "{folded}"
                );
                assert!(folded.contains("The Letter Carrier") && folded.contains(ps));
                assert!(
                    folded.contains("Please bring the revised edition.\nA second postscript line.")
                );
                let source = source
                    .lines()
                    .map(|line| format!("<p>{line}</p>"))
                    .collect::<String>();
                let folded = html(&source);
                assert!(folded.detected);
                assert!(
                    !folded.collapsed.contains("Mira Chen")
                        && !folded.collapsed.contains("Instagram")
                );
                assert!(
                    folded
                        .collapsed
                        .contains("Please bring the revised edition.")
                        && folded.collapsed.contains("A second postscript line.")
                );
            }
            assert_eq!(
                plain(&format!("Confirmed.\n{closing}\nMira Chen")),
                Some("Confirmed.".into())
            );
            assert!(
                plain(&format!(
                    "Confirmed.\n{closing}\nPlease bring the book tomorrow."
                ))
                .is_none()
            );
        }
        assert!(!postscript("Psychology department"));
    }

    #[test]
    fn marked_signature_with_postscript_folds_only_the_signature() {
        let source = "<p>Confirmed.</p><div class='gmail_signature'>Regards,<br>Mira Chen<br>Instagram: @example<br>P.S. Bring the new edition.<br>Keep this continuation.</div><blockquote>Earlier request</blockquote>";
        let result = html(source);
        assert!(result.detected);
        assert!(!result.collapsed.contains("Mira Chen"));
        assert!(
            result.collapsed.contains("Bring the new edition")
                && result.collapsed.contains("Keep this continuation")
        );
        assert!(result.collapsed.contains("Earlier request"));
    }

    #[test]
    fn folds_explicit_signatures_but_leaves_quotes_and_body_images() {
        let source = "<p>Please check the drawing.</p><img src='cid:drawing'><div class='gmail_signature'>Mira<img src='cid:logo'></div><div class='gmail_quote'><div class='gmail_signature'>Old sender<img src='cid:old'></div></div>";
        let result = html(source);
        assert!(result.detected);
        assert!(!result.collapsed.contains("Mira"));
        assert!(result.collapsed.contains("Please check the drawing"));
        assert!(result.collapsed.contains("cid:drawing"));
        assert!(result.collapsed.contains("Old sender"));
        assert!(!result.collapsed.contains("cid:logo"));
        assert!(result.collapsed.contains("cid:old"));
    }

    #[test]
    fn folds_unmarked_signature_with_inline_logo_and_br_contact_lines() {
        let source = "<p>Call the venue on +1 202 555 0142 before delivery.</p><p>Thanks a lot..</p><div><img src='https://ci3.googleusercontent.com/mail-sig/logo'></div><div><strong>Mira Chen</strong><br>Studio Sample<br><a href='mailto:mira@example.test'>mira@example.test</a><br>+1 202 555 0118</div><div class='gmail_quote'>Old history</div>";
        let result = html(source);
        assert!(result.detected);
        assert!(result.collapsed.contains("0142"));
        assert!(!result.collapsed.contains("0118"));
        assert!(!result.collapsed.contains("Mira Chen"));
        assert!(!result.collapsed.contains("mailto:mira@example.test"));
        assert!(!result.collapsed.contains("Thanks a lot"));
        assert!(result.collapsed.contains("Old history"));
        assert!(
            !result
                .collapsed
                .contains("https://ci3.googleusercontent.com/mail-sig/logo")
        );
    }

    #[test]
    fn removes_only_emptied_signature_links() {
        let result = html(
            "<p>Please <a href='https://example.test/body'>read the draft</a>.</p><a href='https://example.test/empty-body'></a><p>Thanks,</p><p>Mira Chen<br><a href='mailto:mira@example.test'><strong>mira@example.test</strong></a></p><a href='https://example.test/signature'><img src='https://ci3.googleusercontent.com/mail-sig/logo'></a>",
        );
        assert!(result.detected);
        assert!(!result.collapsed.contains("mailto:mira@example.test"));
        assert!(!result.collapsed.contains("https://example.test/signature"));
        assert!(result.collapsed.contains("https://example.test/body"));
        assert!(result.collapsed.contains("https://example.test/empty-body"));

        let result = html(
            "<a href='https://example.test/shared'>Keep this text<img src='https://ci3.googleusercontent.com/mail-sig/logo'></a><a href='https://example.test/images'><img src='cid:body'><img src='https://ci3.googleusercontent.com/mail-sig/logo2'></a>",
        );
        assert!(result.detected);
        assert!(result.collapsed.contains("https://example.test/shared"));
        assert!(result.collapsed.contains("Keep this text"));
        assert!(result.collapsed.contains("https://example.test/images"));
        assert!(result.collapsed.contains("cid:body"));

        let result = html(
            "<p>Body.</p><a href='mailto:mira@example.test'><span class='gmail_signature'>Mira</span></a>",
        );
        assert!(result.detected);
        assert!(!result.collapsed.contains("mailto:mira@example.test"));
    }

    #[test]
    fn unmarked_signature_is_independent_of_plain_quoted_history() {
        let source = "Please check the draft.\n\n-- \nMira Chen\nmira@example.test\n\nOn Monday, Sam wrote:\n> Older request";
        let collapsed = plain(source).unwrap();
        assert!(collapsed.contains("Please check the draft."));
        assert!(!collapsed.contains("mira@example.test"));
        assert!(collapsed.contains("On Monday, Sam wrote:\n> Older request"));
    }

    #[test]
    fn standard_separator_can_identify_a_name_only_signature() {
        assert_eq!(
            plain("Please review the draft.\n\n-- \nMira Chen"),
            Some("Please review the draft.".into())
        );
        assert!(plain("Please review the draft.\n\n--\nMira Chen").is_none());
        assert!(plain("Please review the draft.\n\n-- \nPlease reply today").is_none());
    }

    #[test]
    fn keeps_postscript_after_a_signature() {
        let text = "Here is the report.\n\nBest regards,\nMira Chen\nmira@example.test\n\nP.S. Please use the revised figures.";
        let collapsed = plain(text).unwrap();
        assert!(!collapsed.contains("mira@example.test"));
        assert!(collapsed.contains("P.S. Please use the revised figures."));
        let result = html(
            "<p>Here is the report.</p><p>Best regards,</p><p>Mira Chen</p><p>mira@example.test</p><p>P.S. Please use the revised figures.</p>",
        );
        assert!(!result.collapsed.contains("mira@example.test"));
        assert!(
            result
                .collapsed
                .contains("P.S. Please use the revised figures.")
        );
        let inline = "<p>Here is the report.</p><span class='gmail_signature'>P.S. Please use the revised figures.</span>";
        let result = html(inline);
        assert!(!result.detected);
        assert_eq!(result.collapsed, inline);
    }

    #[test]
    fn does_not_guess_from_body_contacts_thanks_or_instructions() {
        for text in [
            "Thanks for reviewing.\nPlease email venue@example.test and call +1 202 555 0142.",
            "Venue contacts:\nVenue Team\nvenue@example.test\n+1 202 555 0142",
            "Here is the report.\nThanks,\nPlease send the revised figures\nmira@example.test",
            "Here is the report.\nThanks,\nMira Chen\nmira@example.test\nPlease send the revised figures.",
            "Thanks,\nMira Chen\nmira@example.test",
        ] {
            assert!(plain(text).is_none(), "{text}");
        }
        let source = "<p>Thanks for reviewing.</p><p>Please call +1 202 555 0142.</p><img src='https://example.test/logo.png'>";
        let result = html(source);
        assert!(!result.detected);
        assert_eq!(result.collapsed, source);
    }

    #[test]
    fn gmail_signature_images_require_the_specific_host_and_endpoint() {
        assert!(signature_image(
            "https://ci3.googleusercontent.com/mail-sig/abc"
        ));
        for source in [
            "https://evil.test/mail-sig/abc",
            "https://googleusercontent.com.evil.test/mail-sig/abc",
            "https://ci3.googleusercontent.com/proxy/abc",
            "file:///mail-sig/abc",
            "https://example.test/signature.png",
        ] {
            assert!(!signature_image(source), "{source}");
        }
        let result = html(
            "<p>A full body.</p><img src='https://ci3.googleusercontent.com/mail-sig/abc'><img src='cid:figure'>",
        );
        assert!(result.detected);
        assert!(!result.collapsed.contains("mail-sig"));
        assert!(result.collapsed.contains("cid:figure"));
    }

    #[test]
    fn preserves_quoted_signatures_and_mobile_signoff_in_current_message_only() {
        let source = "<p>Thanks for sending the file.</p><blockquote><div id='Signature'>Mira<img src='https://ci3.googleusercontent.com/mail-sig/old'></div></blockquote>";
        assert!(!html(source).detected);
        let text =
            "An answer.\n\nSent from my iPhone\n\nOn Monday, Sam wrote:\n> Sent from my iPad";
        let result = plain(text).unwrap();
        assert!(!result.contains("my iPhone"));
        assert!(result.contains("my iPad"));
        let malformed = "<p>Body.</p><div class='gmail_signature'>Mira<blockquote>Earlier request</blockquote></div>";
        assert!(!html(malformed).detected);
    }

    #[test]
    fn apple_and_outlook_markers_are_supported() {
        for marker in [
            "class='AppleMailSignature'",
            "class='moz-signature'",
            "id='Signature'",
            "data-smartmail='gmail_signature'",
        ] {
            let result = html(&format!(
                "<p>Body.</p><div {marker}>Mira<img src='cid:logo'></div>"
            ));
            assert!(result.detected, "{marker}");
            assert!(!result.collapsed.contains("Mira"));
            assert!(!result.collapsed.contains("cid:logo"));
        }
    }
}
