//! A safe, theme-independent reading representation of mail.
//!
//! Markdown is generated from structure, never accepted verbatim from the sender.
//! Images use private slots: only the existing approved-image loader may fetch
//! or decode their sources, and the UI resolves slots to approved in-memory bytes.
use crate::model::Message;
use scraper::{ElementRef, Html, Node, Selector};
use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
    sync::Arc,
};

pub struct ReadingBody {
    pub markdown: String,
    pub complex: bool,
    pub quoted: bool,
    pub signature: bool,
    /// HTML with the requested signature visibility; quote visibility is applied
    /// by the HTML renderer independently.
    pub original_layout: Option<String>,
    pub images: Vec<InlineImage>,
    /// Every HTML image reference, including hidden/quoted/tracking images, so
    /// the attachment gallery cannot bring omitted document images back.
    pub image_sources: Vec<String>,
}

pub struct InlineImage {
    pub token: String,
    pub source: String,
}

/// Match HTML image references to the keys emitted by the approved image loader.
/// CID identifiers are case-sensitive, but their scheme is not; RFC 2392 allows
/// percent-encoded bytes in the identifier. A literal '+' is never a space.
pub fn image_key(source: &str) -> String {
    let source = source.trim();
    if let Some((scheme, identifier)) = source.split_once(':')
        && scheme.eq_ignore_ascii_case("cid")
    {
        let identifier = identifier.trim();
        let mut decoded = Vec::new();
        let bytes = identifier.as_bytes();
        let mut index = 0;
        while index < bytes.len() {
            if bytes[index] == b'%'
                && index + 2 < bytes.len()
                && let (Some(a), Some(b)) = (
                    (bytes[index + 1] as char).to_digit(16),
                    (bytes[index + 2] as char).to_digit(16),
                )
            {
                decoded.push((a * 16 + b) as u8);
                index += 3;
            } else {
                decoded.push(bytes[index]);
                index += 1;
            }
        }
        let decoded = String::from_utf8(decoded).unwrap_or_else(|_| identifier.into());
        let identifier = decoded
            .strip_prefix('<')
            .and_then(|id| id.strip_suffix('>'))
            .unwrap_or(&decoded);
        return format!("cid:{identifier}");
    }
    if let Ok(url) = url::Url::parse(source)
        && url.scheme() == "https"
    {
        return url.to_string();
    }
    source.into()
}

#[derive(Default)]
pub struct Cache {
    entries: HashMap<u64, Arc<ReadingBody>>,
}

impl Cache {
    #[cfg(test)]
    pub fn get(&mut self, message: &Message, expanded: bool) -> Arc<ReadingBody> {
        self.get_with_signature(message, expanded, false)
    }

    pub fn get_with_signature(
        &mut self,
        message: &Message,
        expanded: bool,
        show_signature: bool,
    ) -> Arc<ReadingBody> {
        let mut key = std::collections::hash_map::DefaultHasher::new();
        expanded.hash(&mut key);
        show_signature.hash(&mut key);
        message.header("In-Reply-To").hash(&mut key);
        message.header("From").hash(&mut key);
        for part in message.payload.walk().into_iter().filter(|part| {
            part.filename.is_empty()
                && matches!(part.mime_type.as_str(), "text/plain" | "text/html")
        }) {
            (&part.mime_type, &part.filename, &part.body.data).hash(&mut key);
            part.header("Content-Type").hash(&mut key);
        }
        let key = key.finish();
        if let Some(body) = self.entries.get(&key) {
            return body.clone();
        }
        if self.entries.len() >= 64 {
            self.entries.clear();
        }
        let body = Arc::new(parse_with_signature(message, expanded, show_signature));
        self.entries.insert(key, body.clone());
        body
    }
}

#[cfg(test)]
pub fn parse(message: &Message, expanded: bool) -> ReadingBody {
    parse_with_signature(message, expanded, false)
}

pub fn parse_with_signature(
    message: &Message,
    expanded: bool,
    show_signature: bool,
) -> ReadingBody {
    if let Some(source) = crate::html::source(message).filter(|html| html.len() <= 2_000_000) {
        let original = Html::parse_document(&source);
        let image_sources = original
            .root_element()
            .select(&Selector::parse("img[src]").unwrap())
            .filter_map(|image| image.attr("src").filter(|src| !src.trim().is_empty()))
            .map(str::to_owned)
            .collect();
        let signature_info = crate::signatures::html_for_sender(&source, &message.header("From"));
        let signature = signature_info.detected;
        let selected_source = if show_signature {
            source
        } else {
            signature_info.collapsed.clone()
        };
        let original_layout = Some(selected_source.clone());
        let mut doc = Html::parse_document(&selected_source);
        // Both toggles leave the automatic view choice stable: only the new
        // message, without its signature or quoted history, decides complexity.
        let mut current = if show_signature && signature {
            Html::parse_document(&signature_info.collapsed)
        } else {
            doc.clone()
        };
        let styles: Vec<_> = current
            .root_element()
            .select(&Selector::parse("style").unwrap())
            .map(|style| style.text().collect::<String>())
            .collect();
        let indent_styles = stylesheet_indents(&styles);
        prune(&mut doc, false);
        let mut unquoted = doc.clone();
        let quoted = crate::quotes::strip_html(&mut unquoted);
        prune(&mut current, true);
        let complex = complex_layout(&current) || stylesheet_columns(&current, &styles);
        // Some senders put reply markers only in the plain alternative. Retain
        // the established inline-answer trimming for that representation.
        let has_images = current
            .root_element()
            .select(&Selector::parse("img[src]").unwrap())
            .any(|image| image_source(image).is_some());
        if !quoted
            && !signature
            && !has_images
            && text_len(current.root_element()) > 0
            && message
                .payload
                .walk()
                .iter()
                .any(|part| part.mime_type == "text/plain" && part.filename.is_empty())
        {
            let (trimmed, plain_quoted) = crate::quotes::body(message);
            if plain_quoted {
                // The HTML detector found no signature. Keep this fallback
                // equally conservative so its original layout and readable
                // representation do not disagree about a signature toggle.
                let markdown = plain_markdown(&if expanded { message.text() } else { trimmed });
                return ReadingBody {
                    markdown,
                    complex,
                    quoted: true,
                    signature,
                    original_layout,
                    images: vec![],
                    image_sources,
                };
            }
        }
        if !expanded {
            doc = unquoted;
        }
        let body = doc
            .root_element()
            .select(&Selector::parse("body").unwrap())
            .next()
            .unwrap();
        let mut images = Vec::new();
        let markdown = if hidden(doc.root_element()) {
            String::new()
        } else {
            normalize(&render(body, 0, &mut images, &indent_styles))
        };
        // A broken/empty HTML alternative must not hide a useful plain body.
        // Do not restore history that was intentionally collapsed above.
        if markdown.trim().is_empty()
            && !quoted
            && !signature
            && message.payload.walk().iter().any(|part| {
                part.mime_type == "text/plain"
                    && part.filename.is_empty()
                    && !part.decoded_text().trim().is_empty()
            })
        {
            let (trimmed, quoted) = crate::quotes::body(message);
            let (markdown, signature) = plain_body(
                &if expanded { message.text() } else { trimmed },
                show_signature,
                &message.header("From"),
            );
            return ReadingBody {
                markdown,
                complex,
                quoted,
                signature,
                // There is no useful HTML body to switch back to.
                original_layout: None,
                images: vec![],
                image_sources,
            };
        }
        return ReadingBody {
            markdown,
            complex,
            quoted,
            signature,
            original_layout,
            images,
            image_sources,
        };
    }
    let (trimmed, quoted) = crate::quotes::body(message);
    let (markdown, signature) = plain_body(
        &if expanded { message.text() } else { trimmed },
        show_signature,
        &message.header("From"),
    );
    ReadingBody {
        markdown,
        complex: false,
        quoted,
        signature,
        original_layout: None,
        images: vec![],
        image_sources: vec![],
    }
}

fn plain_body(text: &str, show_signature: bool, sender: &str) -> (String, bool) {
    let collapsed = crate::signatures::plain_for_sender(text, sender);
    let signature = collapsed.is_some();
    let selected = if show_signature {
        text
    } else {
        collapsed.as_deref().unwrap_or(text)
    };
    (plain_markdown(selected), signature)
}

fn prune(doc: &mut Html, quotes: bool) {
    if quotes {
        crate::quotes::strip_html(doc);
    }
    let ids: Vec<_> = doc
        .root_element()
        .select(&Selector::parse("*").unwrap())
        .filter(|node| {
            matches!(
                node.value().name(),
                "script"
                    | "style"
                    | "head"
                    | "iframe"
                    | "object"
                    | "embed"
                    | "form"
                    | "button"
                    | "textarea"
                    | "select"
                    | "meta"
                    | "base"
                    | "link"
                    | "audio"
                    | "video"
                    | "svg"
                    | "noscript"
                    | "template"
            ) || (node.value().name() == "input" && node.attr("type") != Some("checkbox"))
                || (hidden(*node) && !matches!(node.value().name(), "html" | "body"))
                || (quotes
                    && (node.value().has_class(
                        "gmail_quote",
                        scraper::CaseSensitivity::AsciiCaseInsensitive,
                    ) || node.value().has_class(
                        "yahoo_quoted",
                        scraper::CaseSensitivity::AsciiCaseInsensitive,
                    ) || (node.value().name() == "blockquote"
                        && node.attr("type") == Some("cite"))))
        })
        .map(|node| node.id())
        .collect();
    for id in ids {
        if let Some(mut node) = doc.tree.get_mut(id) {
            node.detach();
        }
    }
}

fn hidden(node: ElementRef<'_>) -> bool {
    if node.attr("hidden").is_some() || node.attr("aria-hidden") == Some("true") {
        return true;
    }
    let style: String = node
        .attr("style")
        .unwrap_or_default()
        .chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect();
    style
        .split(';')
        .any(|part| part.starts_with("display:none") || part.starts_with("visibility:hidden"))
}

fn text_len(node: ElementRef<'_>) -> usize {
    node.text()
        .flat_map(str::chars)
        .filter(|c| !c.is_whitespace())
        .count()
}

fn complex_layout(doc: &Html) -> bool {
    for table in doc
        .root_element()
        .select(&Selector::parse("table").unwrap())
    {
        let mut rows_with_columns = 0;
        for row in table.select(&Selector::parse("tr").unwrap()) {
            // Nested wrapper tables are evaluated independently.
            if row
                .ancestors()
                .filter_map(ElementRef::wrap)
                .find(|e| e.value().name() == "table")
                .map(|e| e.id())
                != Some(table.id())
            {
                continue;
            }
            let cells: Vec<_> = row
                .child_elements()
                .filter(|e| matches!(e.value().name(), "td" | "th"))
                .collect();
            let lengths: Vec<_> = cells
                .iter()
                .map(|e| text_len(*e))
                .filter(|len| *len > 0)
                .collect();
            if lengths.len() > 1 {
                rows_with_columns += 1;
                if cells.iter().any(|e| e.value().name() == "th")
                    || lengths.iter().filter(|len| **len >= 35).count() >= 2
                {
                    return true;
                }
            }
        }
        if rows_with_columns >= 2 {
            return true;
        }
    }
    for element in doc
        .root_element()
        .select(&Selector::parse("[style]").unwrap())
    {
        if column_style(element.attr("style").unwrap()) && substantive_columns(element) {
            return true;
        }
    }
    let text = text_len(doc.root_element());
    for image in doc.root_element().select(&Selector::parse("img").unwrap()) {
        let dimension = |name| {
            image
                .attr(name)
                .and_then(|n| n.trim_end_matches("px").parse::<u32>().ok())
                .unwrap_or(0)
        };
        let width = dimension("width");
        let height = dimension("height");
        // Tiny tracking pixels and signature marks do not require an HTML view.
        if tracking_image(image) {
            continue;
        }
        if text < 24 || (text < 180 && (width >= 480 || (width >= 240 && height >= 120))) {
            return true;
        }
    }
    false
}

fn column_style(style: &str) -> bool {
    let style: String = style
        .chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect();
    style.contains("display:grid")
        || style.contains("display:inline-grid")
        || ((style.contains("display:flex") || style.contains("display:inline-flex"))
            && !style.contains("flex-direction:column"))
        || style.split(';').any(|property| {
            property
                .strip_prefix("column-count:")
                .and_then(|value| value.trim_end_matches("!important").parse::<u32>().ok())
                .is_some_and(|count| count > 1)
        })
}

fn substantive_columns(element: ElementRef<'_>) -> bool {
    element
        .child_elements()
        .filter(|child| text_len(*child) >= 35)
        .count()
        >= 2
}

fn stylesheet_columns(doc: &Html, styles: &[String]) -> bool {
    // A conservative selector check covers normal class-defined layouts and
    // nested @media rules. CSS still goes only to the offline HTML renderer;
    // this check never fetches styles or attempts to reconstruct the cascade.
    let rule = regex::Regex::new(r"(?s)([^{}]+)\{([^{}]*)\}").unwrap();
    styles.iter().any(|style| {
        rule.captures_iter(style).any(|parts| {
            column_style(&parts[2])
                && Selector::parse(parts[1].trim())
                    .ok()
                    .is_some_and(|selector| {
                        doc.root_element()
                            .select(&selector)
                            .any(substantive_columns)
                    })
        })
    })
}

fn escape(text: &str) -> String {
    let mut out = String::new();
    for character in text.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\\' | '`' | '*' | '_' | '{' | '}' | '[' | ']' | '(' | ')' | '#' | '+' | '-' | '.'
            | '!' | '|' | '~' => {
                out.push('\\');
                out.push(character);
            }
            _ => out.push(character),
        }
    }
    out
}

fn text_node(text: &str) -> String {
    let mut out = String::new();
    let mut whitespace = false;
    for c in text.chars() {
        if c == '\u{a0}' {
            // Composers use repeated &nbsp; for visible indentation. Keep it
            // until block rendering can distinguish it from HTML source space.
            out.push(c);
            whitespace = false;
        } else if c.is_whitespace() {
            if !whitespace {
                out.push(' ');
            }
            whitespace = true;
        } else {
            out.push(c);
            whitespace = false;
        }
    }
    escape(&out)
}

fn children(
    element: ElementRef<'_>,
    depth: usize,
    images: &mut Vec<InlineImage>,
    styles: &[(Selector, String)],
) -> String {
    let mut output = String::new();
    for node in element.children() {
        match node.value() {
            Node::Text(text) => output.push_str(&text_node(text)),
            Node::Element(_) => output.push_str(&render(
                ElementRef::wrap(node).unwrap(),
                depth + 1,
                images,
                styles,
            )),
            _ => {}
        }
    }
    output
}

fn block(text: &str) -> String {
    if text.trim().is_empty() {
        String::new()
    } else {
        format!("\n\n{}\n\n", text.trim())
    }
}

fn quote_block(text: &str) -> String {
    block(
        &normalize(text)
            .lines()
            .map(|line| format!("> {line}"))
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

fn stylesheet_indents(styles: &[String]) -> Vec<(Selector, String)> {
    let rule = regex::Regex::new(r"(?s)([^{}]+)\{([^{}]*)\}").unwrap();
    styles
        .iter()
        .flat_map(|style| {
            rule.captures_iter(style)
                .filter_map(|parts| {
                    let declarations = parts[2].to_ascii_lowercase();
                    if !declarations.contains("margin") && !declarations.contains("padding") {
                        return None;
                    }
                    Some((Selector::parse(parts[1].trim()).ok()?, declarations))
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

fn indented_style(style: &str, cell_padding: bool) -> bool {
    let positive_length = |value: &str| {
        let value = value.trim().trim_end_matches("!important").trim();
        ["rem", "px", "pt", "em", "cm", "mm", "in", "pc", "%"]
            .iter()
            .find_map(|unit| value.strip_suffix(unit))
            .and_then(|number| number.trim().parse::<f32>().ok())
            .is_some_and(|number| number > 0.)
    };
    // Only explicit left indentation belongs in the readable representation;
    // ordinary all-around message/card padding is layout decoration.
    let mut margin = false;
    let mut padding = false;
    for declaration in style.split(';') {
        let Some((name, value)) = declaration.split_once(':') else {
            continue;
        };
        match name.trim() {
            "margin-left" => margin = positive_length(value),
            "padding-left" => padding = positive_length(value),
            "padding" => {
                let values: Vec<_> = value
                    .trim()
                    .trim_end_matches("!important")
                    .split_whitespace()
                    .collect();
                let left = match values.as_slice() {
                    [all] => Some(*all),
                    [_, horizontal] | [_, horizontal, _] => Some(*horizontal),
                    [_, _, _, left] => Some(*left),
                    _ => None,
                };
                padding = cell_padding && left.is_some_and(positive_length);
            }
            "margin" => {
                let values: Vec<_> = value
                    .trim()
                    .trim_end_matches("!important")
                    .split_whitespace()
                    .collect();
                // Four-sided margins explicitly encode the left offset. Other
                // shorthands reset it without treating card spacing as prose.
                margin = values.len() == 4 && positive_length(values[3]);
            }
            _ => {}
        }
    }
    if cell_padding {
        padding
    } else {
        margin || padding
    }
}

fn element_style(element: ElementRef<'_>, styles: &[(Selector, String)]) -> String {
    let mut declarations = String::new();
    for (selector, style) in styles {
        if selector.matches(&element) {
            declarations.push_str(style);
            declarations.push(';');
        }
    }
    declarations.push_str(
        &element
            .attr("style")
            .unwrap_or_default()
            .to_ascii_lowercase(),
    );
    declarations
}

fn element_indented(element: ElementRef<'_>, styles: &[(Selector, String)]) -> bool {
    indented_style(&element_style(element, styles), false)
}

fn padded_table_rows(element: ElementRef<'_>, styles: &[(Selector, String)]) -> bool {
    // Repeated single-column rows can encode an indented address/list using
    // cell padding alone. A one-cell message wrapper or a data grid is layout.
    if element
        .select(&Selector::parse("table, th").unwrap())
        .next()
        .is_some()
    {
        return false;
    }
    let mut rows = 0;
    for row in element.select(&Selector::parse("tr").unwrap()) {
        if hidden(row) {
            continue;
        }
        let cells: Vec<_> = row
            .child_elements()
            .filter(|cell| cell.value().name() == "td" && !hidden(*cell))
            .collect();
        if cells.len() != 1 {
            return false;
        }
        let cell = cells[0];
        if text_len(cell) == 0 {
            continue;
        }
        if !indented_style(&element_style(cell, styles), true) {
            return false;
        }
        rows += 1;
    }
    rows > 1
}

fn indented_html_lines(content: &str) -> String {
    let mut out = Vec::new();
    let mut indented = false;
    for line in content.lines() {
        let leading = line.trim_start_matches(' ');
        let next = leading.starts_with("\u{a0}\u{a0}");
        if next != indented {
            out.push(String::new());
        }
        out.push(if next {
            format!("> {}", leading.trim_start())
        } else {
            line.to_owned()
        });
        indented = next;
    }
    if indented {
        out.push(String::new());
    }
    out.join("\n")
}

fn marked(text: &str, marker: &str) -> String {
    if text.trim().is_empty() {
        return text.into();
    }
    format!(
        "{}{}{}{}{}",
        if text.starts_with(char::is_whitespace) {
            " "
        } else {
            ""
        },
        marker,
        text.trim(),
        marker,
        if text.ends_with(char::is_whitespace) {
            " "
        } else {
            ""
        }
    )
}

fn safe_url(url: &str) -> Option<String> {
    let url = url::Url::parse(url.trim()).ok()?;
    if !matches!(url.scheme(), "http" | "https" | "mailto") {
        return None;
    }
    Some(
        url.as_str()
            .replace('<', "%3C")
            .replace('>', "%3E")
            .replace('\\', "%5C"),
    )
}

fn tracking_image(image: ElementRef<'_>) -> bool {
    ["width", "height"].iter().any(|name| {
        let inline = image.attr("style").unwrap_or_default();
        let value = image.attr(name).or_else(|| {
            inline.split(';').find_map(|declaration| {
                let (property, value) = declaration.split_once(':')?;
                property.trim().eq_ignore_ascii_case(name).then_some(value)
            })
        });
        value
            .and_then(|value| {
                value
                    .trim()
                    .trim_end_matches("!important")
                    .trim()
                    .trim_end_matches("px")
                    .parse::<f32>()
                    .ok()
            })
            .is_some_and(|dimension| (0. ..=2.).contains(&dimension))
    })
}

fn image_source(image: ElementRef<'_>) -> Option<&str> {
    (!tracking_image(image))
        .then(|| image.attr("src"))
        .flatten()
        .filter(|src| !src.trim().is_empty())
}

// Keep real data tables while unwrapping email layout tables. Markdown has no
// merged cells; reserve their occupied columns so later cells stay aligned.
fn table(
    element: ElementRef<'_>,
    depth: usize,
    images: &mut Vec<InlineImage>,
    styles: &[(Selector, String)],
) -> Option<String> {
    if element.attr("role") == Some("presentation") {
        return None;
    }
    let selector = Selector::parse("tr").unwrap();
    let rows: Vec<_> = element
        .select(&selector)
        .filter(|row| {
            row.ancestors()
                .filter_map(ElementRef::wrap)
                .find(|node| node.value().name() == "table")
                .is_some_and(|parent| parent.id() == element.id())
        })
        .filter(|row| !hidden(*row))
        .collect();
    let nested = Selector::parse("table").unwrap();
    if element.select(&nested).next().is_some() || rows.is_empty() {
        return None;
    }
    let has_headers = rows
        .iter()
        .any(|row| row.child_elements().any(|cell| cell.value().name() == "th"));
    if !has_headers
        && !rows.iter().any(|row| {
            row.child_elements()
                .filter(|cell| matches!(cell.value().name(), "td" | "th"))
                .count()
                > 1
        })
    {
        return None;
    }
    let mut grid = Vec::new();
    let mut occupied = vec![0usize; 128];
    for row in &rows {
        let mut values = Vec::new();
        let mut column = 0;
        for cell in row
            .child_elements()
            .filter(|cell| matches!(cell.value().name(), "td" | "th") && !hidden(*cell))
        {
            while column < occupied.len() && occupied[column] > 0 {
                values.push(String::new());
                column += 1;
            }
            if column >= occupied.len() {
                return None;
            }
            let colspan = cell
                .attr("colspan")
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(1)
                .clamp(1, 128 - column);
            let rowspan = cell
                .attr("rowspan")
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(1)
                .clamp(1, rows.len());
            let value = normalize(&children(cell, depth + 1, images, styles));
            // Pipes are escaped even inside code/link destinations. Keep cell
            // paragraphs readable without emitting a new Markdown table row.
            let value = value.replace("\\|", "|").replace('|', "\\|");
            values.push(
                value
                    .lines()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join(" "),
            );
            for offset in 0..colspan {
                occupied[column + offset] = rowspan;
                if offset > 0 {
                    values.push(String::new());
                }
            }
            column += colspan;
        }
        for count in &mut occupied {
            *count = count.saturating_sub(1);
        }
        grid.push(values);
    }
    let width = grid.iter().map(Vec::len).max().unwrap_or(0);
    if width == 0 {
        return None;
    }
    let mut lines = Vec::new();
    let caption = element
        .child_elements()
        .find(|child| child.value().name() == "caption")
        .map(|caption| children(caption, depth + 1, images, styles))
        .unwrap_or_default();
    if !has_headers {
        grid.insert(0, Vec::new());
    }
    for (index, mut row) in grid.into_iter().enumerate() {
        row.resize(width, String::new());
        lines.push(format!("| {} |", row.join(" | ")));
        if index == 0 {
            lines.push(format!("| {} |", vec!["---"; width].join(" | ")));
        }
    }
    Some(format!("{}{}", block(&caption), block(&lines.join("\n"))))
}

fn render(
    element: ElementRef<'_>,
    depth: usize,
    images: &mut Vec<InlineImage>,
    styles: &[(Selector, String)],
) -> String {
    if hidden(element) {
        return String::new();
    }
    // Mail can contain pathological nesting. Preserve its literal text without
    // recursing further, rather than overflowing the reader's stack.
    if depth >= 100 {
        return text_node(&element.text().collect::<String>());
    }
    let tag = element.value().name();
    if tag == "table" {
        if let Some(markdown) = table(element, depth, images, styles) {
            return if element_indented(element, styles) {
                quote_block(&markdown)
            } else {
                markdown
            };
        }
    }
    match tag {
        "input" => {
            return if element.attr("type") == Some("checkbox") {
                if element.attr("checked").is_some() {
                    "[x] "
                } else {
                    "[ ] "
                }
                .into()
            } else {
                String::new()
            };
        }
        "img" => {
            if tracking_image(element) {
                return String::new();
            }
            let Some(source) = image_source(element) else {
                return text_node(element.attr("alt").unwrap_or_default());
            };
            let token = format!("gmailtjik-inline://image/{}", images.len());
            images.push(InlineImage {
                token: token.clone(),
                source: source.into(),
            });
            // A standalone image uses the available width and intrinsic aspect
            // ratio. Inline Markdown images would otherwise be text-sized.
            let label = text_node(element.attr("alt").unwrap_or_default());
            return block(&format!("![{label}](<{token}>)"));
        }
        "br" => return "  \n".into(),
        "hr" => return "\n\n---\n\n".into(),
        "pre" => {
            let text = element.text().collect::<String>();
            let longest = text.split(|c| c != '`').map(str::len).max().unwrap_or(0);
            let fence = "`".repeat((longest + 1).max(3));
            return format!("\n\n{fence}\n{text}\n{fence}\n\n");
        }
        "ul" | "ol" => {
            let start = element
                .attr("start")
                .and_then(|n| n.parse::<u32>().ok())
                .unwrap_or(1);
            let mut out = Vec::new();
            for (index, li) in element
                .child_elements()
                .filter(|n| n.value().name() == "li")
                .enumerate()
            {
                let text = normalize(&children(li, depth + 1, images, styles));
                if text.is_empty() {
                    continue;
                }
                let prefix = if tag == "ol" {
                    format!("{}. ", start.saturating_add(index as u32))
                } else {
                    "- ".into()
                };
                let indent = " ".repeat(prefix.len());
                let mut lines = text.lines();
                let mut item = format!("{prefix}{}", lines.next().unwrap());
                for line in lines {
                    item.push_str(&format!("\n{indent}{line}"));
                }
                out.push(item);
            }
            return block(&out.join("\n"));
        }
        "code" => {
            let text = element.text().collect::<String>().replace('\n', " ");
            let longest = text.split(|c| c != '`').map(str::len).max().unwrap_or(0);
            let marker = "`".repeat(longest + 1);
            return format!("{marker} {text} {marker}");
        }
        _ => {}
    }
    let mut content = children(element, depth, images, styles);
    // Outlook and web composers often express emphasis with inline CSS.
    let style = element
        .attr("style")
        .unwrap_or_default()
        .to_ascii_lowercase();
    for declaration in style.split(';') {
        if let Some((property, value)) = declaration.split_once(':') {
            match (property.trim(), value.trim()) {
                ("font-style", "italic" | "oblique") if !matches!(tag, "em" | "i") => {
                    content = marked(&content, "*")
                }
                ("font-weight", "bold" | "bolder" | "600" | "700" | "800" | "900")
                    if !matches!(tag, "strong" | "b") =>
                {
                    content = marked(&content, "**")
                }
                ("text-decoration" | "text-decoration-line", "line-through")
                    if !matches!(tag, "del" | "s" | "strike") =>
                {
                    content = marked(&content, "~~")
                }
                _ => {}
            }
        }
    }
    match tag {
        "strong" | "b" => marked(&content, "**"),
        "em" | "i" => marked(&content, "*"),
        "del" | "s" | "strike" => marked(&content, "~~"),
        "a" => {
            if let Some(url) = element.attr("href").and_then(safe_url) {
                let label = if content.trim().is_empty() {
                    escape(&url)
                } else {
                    content.trim().into()
                };
                let destination = url.replace('&', "&amp;");
                let linked = label
                    .split("\n\n")
                    .filter(|part| !part.trim().is_empty())
                    .map(|part| format!("[{}](<{destination}>)", part.trim()))
                    .collect::<Vec<_>>()
                    .join("\n\n");
                if label.contains("gmailtjik-inline://image/") {
                    block(&linked)
                } else {
                    linked
                }
            } else {
                content
            }
        }
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => block(&format!(
            "{} {}",
            "#".repeat(tag.as_bytes()[1] as usize - b'0' as usize),
            content.trim()
        )),
        "blockquote" => quote_block(&content),
        "p" | "div" | "section" | "article" | "header" | "footer" | "address" | "main"
        | "table" | "tr" | "td" | "th" | "dl" | "dt" | "dd" => {
            let text = content.trim();
            // Many mail composers encode bullet lists as ordinary paragraphs.
            let bullet = ["\\* ", "\\- ", "• "]
                .iter()
                .find_map(|marker| text.strip_prefix(marker));
            let content = bullet
                .map(|text| format!("- {text}"))
                .unwrap_or_else(|| indented_html_lines(&content));
            if tag == "dd"
                || (matches!(tag, "p" | "div" | "table") && element_indented(element, styles))
                || (tag == "table" && padded_table_rows(element, styles))
            {
                quote_block(&content)
            } else {
                block(&content)
            }
        }
        _ => content,
    }
}

fn normalize(markdown: &str) -> String {
    let mut lines = Vec::new();
    let mut empty = true;
    let mut fence = 0;
    for line in markdown.lines() {
        let trimmed = line.trim();
        let backticks = trimmed.chars().take_while(|c| *c == '`').count();
        if fence > 0 {
            lines.push(line.to_string());
            if backticks == fence && trimmed.len() == fence {
                fence = 0;
            }
            empty = false;
            continue;
        }
        if backticks >= 3 && trimmed.len() == backticks {
            fence = backticks;
            lines.push(line.to_string());
            empty = false;
            continue;
        }
        if line.trim().is_empty() {
            if !empty {
                lines.push(String::new());
            }
            empty = true;
        } else {
            let hard_break = line.ends_with("  ");
            lines.push(format!(
                "{}{}",
                line.trim_end(),
                if hard_break { "  " } else { "" }
            ));
            empty = false;
        }
    }
    while lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    lines.join("\n")
}

fn plain_markdown(text: &str) -> String {
    let mut out = Vec::new();
    let mut in_list = false;
    for line in text.lines() {
        let trimmed = line.trim();
        let indent = " ".repeat(
            line.chars()
                .take_while(|c| *c == ' ' || *c == '\t')
                .map(|c| if c == '\t' { 4 } else { 1 })
                .sum::<usize>(),
        );
        let bullet = ["* ", "- ", "• "]
            .iter()
            .find_map(|prefix| trimmed.strip_prefix(prefix));
        let ordered = trimmed.split_once(". ").filter(|(n, _)| {
            !n.is_empty() && n.len() <= 9 && n.chars().all(|c| c.is_ascii_digit())
        });
        let line = if let Some(text) = bullet {
            in_list = true;
            format!("{indent}- {}", escape(text))
        } else if let Some((number, text)) = ordered {
            in_list = true;
            format!("{indent}{number}. {}", escape(text))
        } else if let Some(text) = trimmed.strip_prefix("> ") {
            format!("> {}", escape(text))
        } else if trimmed.is_empty() {
            String::new()
        } else {
            let prefix = if indent.is_empty() {
                in_list = false;
                ""
            } else if in_list {
                &indent
            } else {
                "> "
            };
            format!("{prefix}{}  ", escape(trimmed))
        };
        // Close the quote before following prose: Markdown otherwise treats an
        // unmarked line as a lazy continuation of the preceding quote.
        if out.last().is_some_and(|previous: &String| {
            !previous.is_empty() && previous.starts_with("> ") != line.starts_with("> ")
        }) {
            out.push(String::new());
        }
        out.push(line);
    }
    normalize(&out.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Body, Part};
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
    fn outlook_quotes_hide_integrally_and_restore_in_both_views() {
        let message = mail(
            "text/html",
            "<p>Current answer</p><div><p><b>From:</b> sender@example.test<br><b>Sent:</b> Monday<br><b>To:</b> reader@example.test<br><b>Subject:</b> Earlier</p><table><tr><td>Old body</td></tr></table></div><p>Older history</p>",
        );
        let collapsed = parse(&message, false);
        let expanded = parse(&message, true);
        assert!(collapsed.quoted && expanded.quoted);
        assert!(!collapsed.complex && !expanded.complex);
        assert!(collapsed.markdown.contains("Current answer"));
        assert!(
            !collapsed.markdown.contains("Old body")
                && !collapsed.markdown.contains("Older history")
                && !collapsed.markdown.contains("From:")
        );
        assert!(
            expanded.markdown.contains("Old body") && expanded.markdown.contains("Older history")
        );
        let mut layout = scraper::Html::parse_document(collapsed.original_layout.as_ref().unwrap());
        crate::quotes::strip_html(&mut layout);
        assert!(!layout.html().contains("Old body") && !layout.html().contains("Older history"));
    }
    #[test]
    fn preserves_tables_quotes_and_css_emphasis() {
        let body = parse(
            &mail(
                "text/html",
                "<table role='presentation'><tr><td><table><caption>Schedule</caption><tr><th>Item</th><th>Status</th></tr><tr><td><em>Alpha</em> | Beta</td><td><a href='https://example.test'>Ready</a></td></tr><tr><td colspan='2'>Combined</td></tr></table><blockquote>A genuine quotation</blockquote><p><span style='font-style:italic'>Italic</span> <span style='font-weight:700'>Bold</span> <del>Old</del> <code>x</code></p><div class='gmail_quote'>Hidden history</div></td></tr></table>",
            ),
            false,
        );
        assert!(
            body.markdown.contains("| Item | Status |\n| --- | --- |"),
            "{}",
            body.markdown
        );
        assert!(
            body.markdown
                .contains("| *Alpha* \\| Beta | [Ready](<https://example.test/>) |")
        );
        assert!(body.markdown.contains("| Combined |  |"));
        assert!(body.markdown.contains("> A genuine quotation"));
        assert!(body.markdown.contains("*Italic* **Bold** ~~Old~~ ` x `"));
        assert!(!body.markdown.contains("Hidden history"));
    }

    #[test]
    fn preserves_static_task_lists() {
        let body = parse(
            &mail(
                "text/html",
                "<ul><li><input type='checkbox' checked>Done</li><li><input type='checkbox'>Pending</li></ul>",
            ),
            false,
        );
        assert!(body.markdown.contains("- [x] Done\n- [ ] Pending"));
    }

    #[test]
    fn tables_without_headers_preserve_first_row_and_rowspans() {
        let body = parse(
            &mail(
                "text/html",
                "<table><tr><td rowspan='2'>A</td><td>B</td></tr><tr><td>C</td></tr></table>",
            ),
            false,
        );
        assert!(
            body.markdown
                .contains("|  |  |\n| --- | --- |\n| A | B |\n|  | C |"),
            "{}",
            body.markdown
        );
    }

    #[test]
    fn preserves_semantics_and_links_without_sender_styling() {
        let body = parse(
            &mail(
                "text/html",
                "<body bgcolor='white'><h2>Update</h2><p style='color:red;font:40px fantasy'>Hello <b>Mira</b>, <i>please</i> visit <a href='https://example.test/a?q=1&amp;b=2'>the project</a>.</p><ul><li>First</li><li>Second</li></ul><p><br><br></p></body>",
            ),
            false,
        );
        assert!(!body.complex);
        assert!(body.markdown.contains("## Update"));
        assert!(body.markdown.contains("**Mira**"));
        assert!(body.markdown.contains("*please*"));
        assert!(
            body.markdown
                .contains("[the project](<https://example.test/a?q=1&amp;b=2>)")
        );
        assert!(body.markdown.contains("- First\n- Second"));
        assert!(!body.markdown.contains("style"));
        assert!(!body.markdown.contains("\n\n\n"));
    }

    #[test]
    fn simple_wrapper_and_signature_tables_are_not_complex() {
        let body = parse_with_signature(
            &mail(
                "text/html",
                "<table width='600'><tr><td><p>Hello Mira, this is ordinary correspondence with some details to discuss and a simple signature below.</p><table><tr><td><img width='60' height='60' src='cid:logo'></td><td>Sample company<br>mira@example.test</td></tr></table></td></tr></table>",
            ),
            false,
            true,
        );
        assert!(!body.complex);
        assert!(body.markdown.contains("Sample company"));
        assert!(!body.markdown.contains("cid:logo"));
        assert_eq!(body.images[0].source, "cid:logo");
    }

    #[test]
    fn data_tables_and_substantive_columns_retain_layout() {
        for html in [
            "<table><tr><th>Item</th><th>Price</th></tr><tr><td>Tea</td><td>12</td></tr></table>",
            "<div style='display:flex'><div>A detailed product description occupying the first column</div><div>A second detailed product description occupying the other column</div></div>",
        ] {
            assert!(parse(&mail("text/html", html), false).complex);
        }
    }

    #[test]
    fn image_led_mail_stays_html_and_uses_only_private_image_slots() {
        for html in [
            "<img src='https://example.test/news.png'>",
            "<p>This month's announcement</p><img width='600' height='400' src='cid:poster'>",
        ] {
            let body = parse(&mail("text/html", html), false);
            assert!(body.complex);
            assert_eq!(body.images.len(), 1);
            assert!(body.markdown.contains("![](<gmailtjik-inline://image/0>)"));
            assert!(!body.markdown.contains(&body.images[0].source));
            assert!(!body.markdown.contains("src="));
        }
        assert!(
            !parse(
                &mail(
                    "text/html",
                    "<p>Hello</p><img width='1' height='1' src='https://tracker.test/pixel'>"
                ),
                false
            )
            .complex
        );
    }

    #[test]
    fn untrusted_markup_and_urls_cannot_create_active_content() {
        let body = parse(
            &mail(
                "text/html",
                "<script>hidden script</script><style>hidden style</style><p hidden>hidden preheader</p><p>![remote](https://example.test/x) &lt;img src=x&gt; **literal**</p><a href='javascript:alert(1)'>Unsafe</a><a href='file:///etc/passwd'>Local</a><a href='mailto:mira@example.test'>Email</a><img src='https://tracker.test/pixel'>",
            ),
            false,
        );
        assert!(!body.markdown.contains("hidden"));
        assert!(!body.markdown.contains("javascript:"));
        assert!(!body.markdown.contains("file:"));
        assert!(body.markdown.contains("UnsafeLocal"));
        assert!(
            body.markdown
                .contains("[Email](<mailto:mira@example.test>)")
        );
        assert!(body.markdown.contains("\\!\\[remote\\]"));
        assert!(body.markdown.contains("&lt;img src=x&gt;"));
        assert!(body.markdown.contains("\\*\\*literal\\*\\*"));
    }

    #[test]
    fn known_history_is_revealable_without_changing_default_mode() {
        let message = mail(
            "text/html",
            "<p>My answer</p><blockquote>A relevant quotation</blockquote><div class='gmail_quote'><table><tr><th>Old</th><th>Data</th></tr></table></div>",
        );
        let collapsed = parse(&message, false);
        let expanded = parse(&message, true);
        assert!(collapsed.quoted && expanded.quoted);
        assert!(!collapsed.complex && !expanded.complex);
        assert!(!collapsed.markdown.contains("Old"));
        assert!(expanded.markdown.contains("Old"));
        assert!(collapsed.markdown.contains("> A relevant quotation"));
    }

    #[test]
    fn plaintext_lists_and_inline_replies_are_preserved() {
        let message = mail(
            "text/plain",
            "Hello\n\n* Verify the details\n* Reply with **literal** notes\n\nOn Monday, Mira wrote:\n> First question\nMy inline answer\n> Second question\nAnother answer\n\n<not html>",
        );
        let body = parse(&message, false);
        assert!(body.quoted);
        assert!(
            body.markdown
                .contains("- Verify the details\n- Reply with \\*\\*literal\\*\\* notes")
        );
        assert!(body.markdown.contains("My inline answer"));
        assert!(!body.markdown.contains("First question"));
        assert!(body.markdown.contains("&lt;not html&gt;"));
        assert!(parse(&message, true).markdown.contains("> First question"));
    }

    #[test]
    fn indented_prose_is_readable_without_becoming_code_or_hidden_history() {
        let body = parse(
            &mail("text/plain", "Before\n    Test\n    in\n\tMarkdown\nAfter"),
            false,
        );
        assert_eq!(
            body.markdown,
            "Before  \n\n> Test  \n> in  \n> Markdown  \n\nAfter  "
        );
        assert!(!body.quoted);
        assert!(body.original_layout.is_none());
    }

    #[test]
    fn html_indents_preserve_structure_and_original_layout() {
        for source in [
            "<p>Before</p><div style='margin-left:40px'>Test<br>in<br>Markdown</div><p>After</p>",
            "<p>Before</p><p style='padding-left:2rem!important'>Test<br>in<br>Markdown</p><p>After</p>",
            "<p>Before<br>&nbsp;&nbsp;Test<br>&nbsp;&nbsp;in<br>&nbsp;&nbsp;Markdown<br>After</p>",
            "<dl><dt>Before</dt><dd>Test<br>in<br>Markdown</dd><dt>After</dt></dl>",
        ] {
            let body = parse(&mail("text/html", source), false);
            assert_eq!(
                body.markdown
                    .lines()
                    .map(str::trim_end)
                    .collect::<Vec<_>>()
                    .join("\n"),
                "Before\n\n> Test\n> in\n> Markdown\n\nAfter",
                "{source}"
            );
            assert!(
                !body.quoted,
                "Indentation must not be folded as reply history"
            );
            assert!(body.original_layout.unwrap().contains("Markdown"));
        }
        let body = parse(
            &mail(
                "text/html",
                "<div style='padding:24px'><p style='margin-left:0'>Flush</p><p style='margin-left:-4px'>Also flush</p></div>",
            ),
            false,
        );
        assert_eq!(body.markdown, "Flush\n\nAlso flush");
    }

    #[test]
    fn stylesheet_indented_single_column_tables_become_visible_quote_blocks() {
        // Generated mail uses a class margin on the table, ordinary padding on
        // its cells, and zero-margin paragraphs. Only the table is an indent.
        let source = "<style>.entries { margin-left:5.4pt } .cell { padding:0pt 5.4pt 0pt 5.4pt } .entry { text-indent:0pt; margin:0pt 0pt 0pt 0pt }</style><p>Available options:</p><table class='entries' style='border-collapse:collapse'><tr><td class='cell'><p class='entry'>Council Alpha</p></td></tr><tr><td class='cell'><p class='entry'>Council Beta</p></td></tr></table><p>Following details.</p>";
        let body = parse(&mail("text/html", source), false);
        assert_eq!(
            body.markdown,
            "Available options:\n\n> Council Alpha\n>\n> Council Beta\n\nFollowing details\\."
        );
        assert!(!body.quoted && !body.complex);
        assert!(body.original_layout.unwrap().contains("margin-left:5.4pt"));
        let reset = source.replace(
            "border-collapse:collapse",
            "border-collapse:collapse;margin-left:0",
        );
        assert!(
            parse(&mail("text/html", &reset), false)
                .markdown
                .contains("> Council")
        );
        let reset = reset.replace("padding:0pt 5.4pt 0pt 5.4pt", "padding:0");
        assert!(
            !parse(&mail("text/html", &reset), false)
                .markdown
                .contains("> Council")
        );
    }

    #[test]
    fn class_and_shorthand_indents_keep_table_structure_and_inline_resets() {
        for source in [
            "<style>.indent { margin-left:24px }</style><div class='indent'>Indented text</div>",
            "<p style='margin:0 0 0 24px'>Indented text</p>",
            "<table style='margin-left:24px'><tr><td>Indented text</td></tr></table>",
        ] {
            assert_eq!(
                parse(&mail("text/html", source), false).markdown,
                "> Indented text"
            );
        }
        let source = "<style>.indent { margin-left:24px } .indent { margin-left:0 }</style><p class='indent'>Flush</p><p style='margin-left:24px;margin:0'>Also flush</p>";
        assert_eq!(
            parse(&mail("text/html", source), false).markdown,
            "Flush\n\nAlso flush"
        );
        let source = "<style>.indent { margin-left:24px }</style><table class='indent'><tr><th>Item</th><th>Status</th></tr><tr><td>Alpha</td><td>Ready</td></tr></table>";
        let body = parse(&mail("text/html", source), false);
        assert!(
            body.markdown
                .contains("> | Item | Status |\n> | --- | --- |\n> | Alpha | Ready |")
        );
    }

    #[test]
    fn table_cell_padding_indents_repeated_single_column_content_only() {
        for padding in ["0pt 5.4pt", "0pt 5.4pt 0pt", "0pt 5.4pt 0pt 5.4pt"] {
            let source = format!(
                "<style>.cell {{ width:437.9pt; padding:{padding} }} .line {{ margin:0pt 0pt 0pt 0pt }}</style><p>Delivery details:</p><table style='border-collapse:collapse'><tr><td class='cell'><p class='line'>Studio 12</p></td></tr><tr><td class='cell'><p class='line'>Example Building</p></td></tr><tr><td class='cell'><p class='line'>Example City</p></td></tr></table><p>Next field:</p>"
            );
            let body = parse(&mail("text/html", &source), false);
            assert_eq!(
                body.markdown,
                "Delivery details:\n\n> Studio 12\n>\n> Example Building\n>\n> Example City\n\nNext field:"
            );
            assert!(!body.quoted && !body.complex);
            assert!(
                body.original_layout
                    .unwrap()
                    .contains(&format!("padding:{padding}"))
            );
            let reset = source.replace("class='cell'", "class='cell' style='padding:0'");
            assert!(
                !parse(&mail("text/html", &reset), false)
                    .markdown
                    .contains("> Studio")
            );
            let margin = source.replace("border-collapse:collapse", "margin-left:5.4pt");
            assert!(
                !parse(&mail("text/html", &margin), false)
                    .markdown
                    .contains("> >")
            );
        }
        for source in [
            "<table><tr><td style='padding:0 24px'><p>Message heading</p><p>Message body</p></td></tr></table>",
            "<table><tr><td style='padding:0 24px'>A</td><td>B</td></tr><tr><td style='padding:0 24px'>C</td><td>D</td></tr></table>",
            "<table><tr><td style='padding:0 24px 0 0'>A</td></tr><tr><td style='padding:0 24px 0 0'>B</td></tr></table>",
        ] {
            assert!(
                !parse(&mail("text/html", source), false)
                    .markdown
                    .starts_with('>'),
                "{source}"
            );
        }
    }

    #[test]
    fn cache_tracks_content_and_expansion_and_is_bounded() {
        let mut cache = Cache::default();
        let message = mail("text/html", "<p>New</p><div class='gmail_quote'>Old</div>");
        let first = cache.get(&message, false);
        assert!(Arc::ptr_eq(&first, &cache.get(&message, false)));
        assert!(!Arc::ptr_eq(&first, &cache.get(&message, true)));
        for n in 0..100 {
            cache.get(&mail("text/plain", &format!("Message {n}")), false);
        }
        assert!(cache.entries.len() <= 64);
    }

    #[test]
    fn class_defined_columns_are_detected_but_quoted_layout_is_not() {
        let columns = "<div class='columns'><p>First product with a detailed description for comparison.</p><p>Second product with a detailed description for comparison.</p></div>";
        let css = "<style>@media screen { .columns { display: flex; } }</style>";
        assert!(parse(&mail("text/html", &format!("{css}{columns}")), false).complex);
        let quoted = mail(
            "text/html",
            &format!("{css}<p>Thanks</p><div class='gmail_quote'>{columns}</div>"),
        );
        assert!(!parse(&quoted, true).complex);
    }

    #[test]
    fn code_and_nested_plaintext_lists_retain_structure() {
        let code = "  let value = 1;\n\n\n![literal](https://example.test/x)\n```\n<not markup>";
        let body = parse(
            &mail(
                "text/html",
                &format!(
                    "<pre>{}</pre>",
                    code.replace('<', "&lt;").replace('>', "&gt;")
                ),
            ),
            false,
        );
        assert_eq!(body.markdown, format!("````\n{code}\n````"));
        let body = parse(
            &mail(
                "text/plain",
                "* First\n  * Nested\n    More detail\n* Second",
            ),
            false,
        );
        assert!(
            body.markdown
                .contains("- First\n  - Nested\n    More detail")
        );
    }

    #[test]
    fn hidden_document_wrappers_do_not_panic() {
        assert!(
            parse(
                &mail("text/html", "<body hidden><p>Hidden</p></body>"),
                false
            )
            .markdown
            .is_empty()
        );
        assert!(
            parse(
                &mail(
                    "text/html",
                    "<html hidden><body><p>Hidden</p></body></html>"
                ),
                false
            )
            .markdown
            .is_empty()
        );
    }

    #[test]
    fn empty_html_alternative_keeps_plain_body() {
        let mut message = mail("multipart/alternative", "");
        message.payload.parts = vec![
            mail("text/plain", "A useful body\n\n* First item").payload,
            mail("text/html", "<body><p><br></p></body>").payload,
        ];
        let body = parse(&message, false);
        assert!(!body.complex);
        assert!(body.markdown.contains("A useful body"));
        assert!(body.markdown.contains("- First item"));
    }

    #[test]
    fn inline_images_keep_their_positions_and_safe_links() {
        let body = parse(
            &mail(
                "text/html",
                "<p>Before<img src='cid:first' alt='First image'>Between<a href='https://example.test/details'><img src='cid:second' alt='Second image'></a>After</p>",
            ),
            false,
        );
        assert_eq!(body.images.len(), 2);
        assert_eq!(body.images[0].source, "cid:first");
        assert_eq!(body.images[1].source, "cid:second");
        assert_eq!(
            body.markdown,
            "Before\n\n![First image](<gmailtjik-inline://image/0>)\n\nBetween\n\n[![Second image](<gmailtjik-inline://image/1>)](<https://example.test/details>)\n\nAfter"
        );
    }

    #[test]
    fn html_images_survive_plain_alternative_quote_detection() {
        let mut message = mail("multipart/alternative", "");
        message.payload.parts = vec![
            mail("text/plain", "[image: image.png]\nHere is the corrected version.\n\nOn Monday, Mira wrote:\n> Previous version").payload,
            mail("text/html", "<img src='cid:version' alt='image.png'><p>Here is the <b>corrected version</b>.</p><blockquote>Previous version</blockquote>").payload,
        ];
        for expanded in [false, true] {
            let body = parse(&message, expanded);
            assert_eq!(body.images.len(), 1);
            assert_eq!(body.images[0].source, "cid:version");
            assert!(body.markdown.contains("**corrected version**"));
            assert!(
                body.markdown
                    .contains("![image\\.png](<gmailtjik-inline://image/0>)")
            );
            assert!(!body.markdown.contains("[image:"));
            // Unrecognized HTML quotation structure is retained rather than
            // dropping images through a plain-text fallback.
            assert!(body.markdown.contains("Previous version"));
        }
    }

    #[test]
    fn quote_image_slots_follow_visibility_and_never_include_detached_images() {
        let message = mail(
            "text/html",
            "<p>New<img src='cid:new'></p><div class='gmail_quote'><img src='cid:old'><p>Old</p></div><div hidden><img src='cid:hidden'></div><form><img src='cid:form'></form>",
        );
        let collapsed = parse(&message, false);
        let expanded = parse(&message, true);
        assert!(collapsed.quoted && expanded.quoted);
        assert_eq!(collapsed.images.len(), 1);
        assert_eq!(collapsed.images[0].source, "cid:new");
        assert_eq!(expanded.images.len(), 2);
        assert_eq!(expanded.images[1].source, "cid:old");
        assert_eq!(
            collapsed.image_sources,
            ["cid:new", "cid:old", "cid:hidden", "cid:form"]
        );
        assert_eq!(expanded.image_sources, collapsed.image_sources);
        assert!(collapsed.markdown.contains("gmailtjik-inline://image/0"));
        assert!(!collapsed.markdown.contains("gmailtjik-inline://image/1"));
        assert!(expanded.markdown.contains("gmailtjik-inline://image/1"));
    }

    #[test]
    fn sender_image_sources_and_alt_text_cannot_inject_markdown_resources() {
        let body = parse(
            &mail(
                "text/html",
                "<img src='file:///etc/passwd' alt='](https://evil.test/) ![x](&lt;file:///secret&gt;)'><img src='javascript:alert(1)' alt='&lt;img src=https://evil.test/x&gt;'><img src='https://example.test/a?x=&quot;)&lt;script&gt;' alt='New&#10;line'>",
            ),
            false,
        );
        assert_eq!(body.images.len(), 3);
        assert_eq!(body.images[0].source, "file:///etc/passwd");
        assert!(!body.markdown.contains("file:///etc/passwd"));
        assert!(!body.markdown.contains("javascript:alert"));
        assert!(!body.markdown.contains("<script>"));
        assert!(body.markdown.contains("\\]\\(https://evil\\.test/\\)"));
        assert!(
            body.markdown
                .contains("&lt;img src=https://evil\\.test/x&gt;")
        );
        assert!(
            body.markdown
                .contains("![New line](<gmailtjik-inline://image/2>)")
        );
        for (index, image) in body.images.iter().enumerate() {
            assert_eq!(image.token, format!("gmailtjik-inline://image/{index}"));
        }
    }

    #[test]
    fn image_keys_match_cid_encoding_and_remote_loader_urls() {
        assert_eq!(image_key("cid:a%40b"), image_key("CID:a@b"));
        assert_eq!(image_key(" CID:%3CPart+A%40Host%3E "), "cid:Part+A@Host");
        assert_eq!(image_key("cid:<Part+A@Host>"), "cid:Part+A@Host");
        assert_ne!(image_key("cid:Part@Host"), image_key("cid:part@host"));
        assert_eq!(
            image_key(" HTTPS://EXAMPLE.TEST:443/art "),
            "https://example.test/art"
        );
        assert_eq!(image_key("image.png"), "image.png");
        assert_eq!(image_key("cid:a%ZZ"), "cid:a%ZZ");
    }

    #[test]
    fn tiny_tracking_images_do_not_turn_into_document_images() {
        let body = parse(
            &mail(
                "text/html",
                "<p>Hello</p><img src='https://tracker.test/a' width='1'><img src='https://tracker.test/b' style='height: 1px !important'>",
            ),
            false,
        );
        assert!(body.images.is_empty());
        assert_eq!(
            body.image_sources,
            ["https://tracker.test/a", "https://tracker.test/b"]
        );
        assert!(!body.complex);
        assert_eq!(body.markdown, "Hello");
    }

    #[test]
    fn signature_and_quote_visibility_preserve_body_images_independently() {
        let source = "<p>Please review the corrected diagram below and send your notes when ready.</p><img src='cid:body' width='320' height='100'><div class='gmail_signature'><table><tr><th>Mira Chen</th><th>Contact details</th></tr><tr><td>Studio North</td><td>mira@example.test</td></tr></table><img src='cid:logo'></div><div class='gmail_quote'><p>Earlier note</p><img src='cid:old'></div>";
        let message = mail("text/html", source);
        for expanded in [false, true] {
            for show_signature in [false, true] {
                let body = parse_with_signature(&message, expanded, show_signature);
                assert!(body.signature && body.quoted);
                assert!(
                    !body.complex,
                    "Revealing a signature must not change the automatic view"
                );
                assert!(body.markdown.contains("corrected diagram"));
                assert_eq!(body.markdown.contains("Mira Chen"), show_signature);
                assert_eq!(body.markdown.contains("Earlier note"), expanded);
                let images: Vec<_> = body
                    .images
                    .iter()
                    .map(|image| image.source.as_str())
                    .collect();
                assert!(images.contains(&"cid:body"));
                assert_eq!(images.contains(&"cid:logo"), show_signature);
                assert_eq!(images.contains(&"cid:old"), expanded);
                assert_eq!(body.image_sources, ["cid:body", "cid:logo", "cid:old"]);
                let original = body.original_layout.as_deref().unwrap();
                assert_eq!(original.contains("cid:logo"), show_signature);
                assert!(original.contains("cid:body") && original.contains("cid:old"));
                if show_signature {
                    assert_eq!(original, source);
                }
            }
        }
    }

    #[test]
    fn unmarked_signature_links_do_not_return_as_markdown_labels() {
        let message = mail("text/html", include_str!("../assets/demo-signature.html"));
        let hidden = parse(&message, false);
        assert!(hidden.signature);
        assert!(hidden.markdown.contains("+1 202 555 0142"));
        assert!(!hidden.markdown.contains("mira@example"));
        assert!(!hidden.markdown.contains("Mira Chen"));
        assert!(!hidden.markdown.contains("+1 202 555 0118"));
        assert_eq!(hidden.images.len(), 1);
        assert_eq!(hidden.images[0].source, "cid:body-art");
        let visible = parse_with_signature(&message, false, true);
        assert!(visible.markdown.contains("mira@example"));
        assert!(visible.markdown.contains("Mira Chen"));
        assert_eq!(visible.images.len(), 2);
    }

    #[test]
    fn detected_html_signature_cannot_return_through_plain_fallbacks() {
        let mut message = mail("multipart/alternative", "");
        message.payload.parts = vec![
            mail(
                "text/plain",
                "Body request.\n\nMira\nmira@example.test\n\nOn Monday, Sam wrote:\n> Old note",
            )
            .payload,
            mail(
                "text/html",
                "<p>Body request.</p><div class='gmail_signature'>Mira<br>mira@example.test</div>",
            )
            .payload,
        ];
        let body = parse(&message, false);
        assert!(body.signature);
        assert_eq!(body.markdown, "Body request\\.");
        assert!(!body.markdown.contains("Mira"));
        message.payload.parts[1] = mail(
            "text/html",
            "<div class='gmail_signature'>Mira<br>mira@example.test</div>",
        )
        .payload;
        let body = parse(&message, false);
        assert!(body.signature);
        assert!(
            body.markdown.is_empty(),
            "An empty signature-only HTML body must not restore the plain signature"
        );
        assert!(
            parse_with_signature(&message, false, true)
                .markdown
                .contains("Mira")
        );
    }

    #[test]
    fn plain_signature_and_quotes_have_independent_toggles() {
        let message = mail(
            "text/plain",
            "Body request.\n\n-- \nMira\nmira@example.test\n\nOn Monday, Sam wrote:\n> Old note",
        );
        for expanded in [false, true] {
            for show_signature in [false, true] {
                let body = parse_with_signature(&message, expanded, show_signature);
                assert!(body.signature && body.quoted);
                assert!(body.markdown.contains("Body request"));
                assert_eq!(body.markdown.contains("Mira"), show_signature);
                assert_eq!(body.markdown.contains("Old note"), expanded);
                assert!(body.original_layout.is_none());
            }
        }
    }

    #[test]
    fn signature_visibility_participates_in_cache_identity() {
        let message = mail(
            "text/html",
            "<p>Body request.</p><div class='gmail_signature'>Mira<br>mira@example.test</div>",
        );
        let mut cache = Cache::default();
        let hidden = cache.get(&message, false);
        let visible = cache.get_with_signature(&message, false, true);
        assert!(hidden.signature && visible.signature);
        assert!(!Arc::ptr_eq(&hidden, &visible));
        assert!(Arc::ptr_eq(
            &hidden,
            &cache.get_with_signature(&message, false, false)
        ));
        assert!(!hidden.markdown.contains("Mira"));
        assert!(visible.markdown.contains("Mira"));
    }

    #[test]
    fn plain_signature_detection_does_not_promise_an_unmatched_html_change() {
        let mut message = mail("multipart/alternative", "");
        message.payload.parts = vec![
            mail("text/plain", "Body request.\n\n-- \nMira\nmira@example.test\n\nOn Monday, Sam wrote:\n> Old note").payload,
            mail("text/html", "<p>Body request.</p><blockquote>Old note</blockquote>").payload,
        ];
        let body = parse(&message, false);
        assert!(!body.signature);
        assert!(body.markdown.contains("Mira"));
        assert!(body.original_layout.is_some());

        message.payload.parts[1] = mail("text/html", "<p><br></p>").payload;
        let body = parse(&message, false);
        assert!(body.signature);
        assert!(!body.markdown.contains("Mira"));
        assert!(body.original_layout.is_none());
    }
}
