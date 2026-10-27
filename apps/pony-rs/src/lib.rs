// SPDX-License-Identifier: GPL-3.0-or-later
// Pony template and balloon semantics follow erkin/ponysay (see assets/UPSTREAM.md).
use unicode_width::UnicodeWidthChar;

pub struct Pony {
    pub name: &'static str,
    pub extra: bool,
    pub tty: bool,
    pub art: &'static str,
}
include!(concat!(env!("OUT_DIR"), "/ponies.rs"));

// Retain terminal colour sequences, but don't count them as visible columns.
fn escape_end(s: &str, start: usize) -> usize {
    let bytes = s.as_bytes();
    if bytes.get(start + 1) == Some(&b'[') {
        for (i, &b) in bytes.iter().enumerate().skip(start + 2) {
            if (0x40..=0x7e).contains(&b) {
                return i + 1;
            }
        }
    } else if bytes.get(start + 1) == Some(&b']') {
        // Linux console palette commands are fixed length, not OSC/ST strings.
        if bytes.get(start + 2) == Some(&b'P') {
            return (start + 10).min(s.len());
        }
        if bytes.get(start + 2) == Some(&b'R') {
            return (start + 3).min(s.len());
        }
        for i in start + 2..bytes.len() {
            if bytes[i] == 7 {
                return i + 1;
            }
            if bytes[i] == 27 && bytes.get(i + 1) == Some(&b'\\') {
                return i + 2;
            }
        }
    }
    (start + 1).min(s.len())
}

pub fn display_width(s: &str) -> usize {
    let (mut i, mut width) = (0, 0);
    while i < s.len() {
        if s.as_bytes()[i] == 27 {
            i = escape_end(s, i);
        } else {
            let c = s[i..].chars().next().unwrap();
            width += c.width().unwrap_or(0);
            i += c.len_utf8();
        }
    }
    width
}

pub fn strip_ansi(s: &str) -> String {
    let (mut i, mut result) = (0, String::new());
    while i < s.len() {
        if s.as_bytes()[i] == 27 {
            i = escape_end(s, i);
        } else {
            let c = s[i..].chars().next().unwrap();
            result.push(c);
            i += c.len_utf8();
        }
    }
    result
}

fn body(art: &str) -> &str {
    if let Some(rest) = art.strip_prefix("$$$\n") {
        if let Some(rest) = rest.strip_prefix("$$$\n") {
            return rest;
        }
        if let Some((_, rest)) = rest.split_once("\n$$$\n") {
            return rest;
        }
    }
    art
}

fn prepare_message(message: &str) -> String {
    let mut expanded = String::new();
    let mut col = 0;
    // Quotes are plain text: remove terminal control sequences before rendering.
    for c in strip_ansi(message.trim_end_matches(['\r', '\n'])).chars() {
        match c {
            '\n' => {
                expanded.push(c);
                col = 0;
            }
            '\t' => {
                let n = 8 - col % 8;
                expanded.push_str(&" ".repeat(n));
                col += n;
            }
            c if !c.is_control() => {
                expanded.push(c);
                col += c.width().unwrap_or(0);
            }
            _ => {}
        }
    }
    let indent = expanded
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start_matches(' ').len())
        .min()
        .unwrap_or(0);
    expanded
        .split('\n')
        .map(|l| l.get(indent.min(l.len())..).unwrap_or("").trim_end())
        .collect::<Vec<_>>()
        .join("\n")
}

fn wrap(message: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines = Vec::new();
    for line in message.split('\n') {
        let mut current = String::new();
        for word in line.split_whitespace() {
            if !current.is_empty() && display_width(&current) + 1 + display_width(word) > width {
                lines.push(std::mem::take(&mut current));
            }
            if !current.is_empty() {
                current.push(' ');
            }
            for c in word.chars() {
                if !current.is_empty() && display_width(&current) + c.width().unwrap_or(0) > width {
                    lines.push(std::mem::take(&mut current));
                }
                current.push(c);
            }
        }
        lines.push(current);
    }
    lines
}

fn balloon(message: &str, minimum: usize, columns: usize, indent: usize) -> Vec<String> {
    let lines = wrap(message, columns.saturating_sub(indent + 4).max(8));
    let width = (lines.iter().map(|l| display_width(l)).max().unwrap_or(0) + 4).max(minimum);
    let mut out = vec![format!(" {} ", "_".repeat(width - 2))];
    for (i, line) in lines.iter().enumerate() {
        let (left, right) = if lines.len() == 1 {
            ("< ", " >")
        } else if i == 0 {
            ("/ ", " \\")
        } else if i + 1 == lines.len() {
            ("\\ ", " /")
        } else {
            ("| ", " |")
        };
        out.push(format!(
            "{left}{line}{}{right}",
            " ".repeat(width - 4 - display_width(line))
        ));
    }
    out.push(format!(" {} ", "-".repeat(width - 2)));
    out
}

#[derive(Default)]
struct Style {
    foreground: Option<String>,
    background: Option<String>,
    attributes: std::collections::BTreeSet<u16>,
}
impl Style {
    fn observe(&mut self, code: &str) {
        if !code.starts_with("\x1b[") || !code.ends_with('m') {
            return;
        }
        let values: Vec<u16> = code[2..code.len() - 1]
            .split(';')
            .map(|v| v.parse().unwrap_or(0))
            .collect();
        let mut i = 0;
        while i < values.len() {
            let value = values[i];
            i += 1;
            match value {
                0 => *self = Self::default(),
                1..=9 => {
                    self.attributes.insert(value);
                }
                21..=29 => {
                    self.attributes.remove(&(value - 20));
                }
                39 => self.foreground = None,
                49 => self.background = None,
                30..=37 | 90..=97 => self.foreground = Some(value.to_string()),
                40..=47 | 100..=107 => self.background = Some(value.to_string()),
                38 | 48 => {
                    let count = match values.get(i) {
                        Some(5) => 2,
                        Some(2) => 4,
                        _ => 0,
                    };
                    if count > 0 && i + count <= values.len() {
                        let sequence = values[i - 1..i + count]
                            .iter()
                            .map(u16::to_string)
                            .collect::<Vec<_>>()
                            .join(";");
                        if value == 38 {
                            self.foreground = Some(sequence);
                        } else {
                            self.background = Some(sequence);
                        }
                        i += count;
                    }
                }
                _ => {}
            }
        }
    }
    fn insert(&self, text: &str) -> String {
        let mut codes = vec!["0".to_string()];
        if let Some(value) = &self.foreground {
            codes.push(value.clone());
        }
        if let Some(value) = &self.background {
            codes.push(value.clone());
        }
        codes.extend(self.attributes.iter().map(u16::to_string));
        format!("\x1b[0m{text}\x1b[{}m", codes.join(";"))
    }
}

fn expand_links(line: &str, style: &mut Style) -> Result<String, String> {
    let (mut i, mut out, mut col) = (0, String::new(), 0);
    while i < line.len() {
        match line.as_bytes()[i] {
            27 => {
                let end = escape_end(line, i);
                let code = &line[i..end];
                style.observe(code);
                out.push_str(code);
                i = end;
            }
            b'$' => {
                let end = line[i + 1..]
                    .find('$')
                    .ok_or("Unclosed pony template token")?
                    + i
                    + 1;
                let token = &line[i + 1..end];
                let value = match token {
                    "\\" => "\\",
                    "/" => "/",
                    "X" => "X",
                    "" => "$",
                    _ => return Err(format!("Unsupported pony template token: {token}")),
                };
                out.push_str(&style.insert(value));
                col += 1;
                i = end + 1;
            }
            b'\t' => {
                let n = 8 - col % 8;
                out.push_str(&" ".repeat(n));
                col += n;
                i += 1;
            }
            _ => {
                let c = line[i..].chars().next().unwrap();
                out.push(c);
                col += c.width().unwrap_or(0);
                i += c.len_utf8();
            }
        }
    }
    Ok(out)
}

// Insert a balloon over existing spaces/art for the two upstream ponies whose
// balloon is inside the image. Preserve colour state for the remaining artwork.
fn overlay(line: &str, at: usize, replacement: &str) -> String {
    let (mut i, mut col, mut inserted) = (0, 0, false);
    let mut out = String::new();
    let mut style = Style::default();
    while i < line.len() {
        if line.as_bytes()[i] == 27 {
            let end = escape_end(line, i);
            let code = &line[i..end];
            style.observe(code);
            out.push_str(code);
            i = end;
            continue;
        }
        if col >= at && !inserted {
            out.push_str(&style.insert(replacement));
            inserted = true;
        }
        let c = line[i..].chars().next().unwrap();
        if col < at || col >= at + display_width(replacement) {
            out.push(c);
        }
        col += c.width().unwrap_or(0);
        i += c.len_utf8();
    }
    if !inserted {
        out.push_str(&" ".repeat(at.saturating_sub(col)));
        out.push_str(&style.insert(replacement));
    }
    out
}

pub fn render(art: &str, message: &str, columns: usize) -> Result<String, String> {
    let message = prepare_message(message);
    let mut style = Style::default();
    let mut result = Vec::new();
    let mut pending: Option<(usize, std::collections::VecDeque<String>)> = None;
    for (line_index, line) in body(art).split_terminator('\n').enumerate() {
        if let Some(start) = line.find("$balloon") {
            let end = line[start + 1..]
                .find('$')
                .ok_or("Unclosed balloon token")?
                + start
                + 1;
            let minimum = line[start + 8..end].parse::<usize>().unwrap_or(0);
            let prefix = expand_links(&line[..start], &mut style)?;
            let indent = display_width(&prefix);
            let bubble = balloon(&message, minimum, columns, indent);
            let bubble_style = style.insert(&bubble[0]);
            let suffix = expand_links(&line[end + 1..], &mut style)?;
            if line_index == 0 {
                result.push(format!("{prefix}{bubble_style}"));
                for row in &bubble[1..] {
                    result.push(format!("{}{}", " ".repeat(indent), style.insert(row)));
                }
                result.last_mut().unwrap().push_str(&suffix);
            } else {
                result.push(format!("{prefix}{bubble_style}{suffix}"));
                pending = Some((indent, bubble.into_iter().skip(1).collect()));
            }
        } else {
            let mut expanded = expand_links(line, &mut style)?;
            if let Some((indent, rows)) = &mut pending {
                if let Some(row) = rows.pop_front() {
                    expanded = overlay(&expanded, *indent, &row);
                }
            }
            result.push(expanded);
        }
    }
    if let Some((indent, rows)) = pending {
        for row in rows {
            result.push(format!("{}{}", " ".repeat(indent), style.insert(&row)));
        }
    }
    Ok(result.join("\n") + "\x1b[0m\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn complete_upstream_catalogue_renders() {
        assert_eq!(PONIES.iter().filter(|p| !p.extra && !p.tty).count(), 474);
        assert_eq!(PONIES.iter().filter(|p| p.extra && !p.tty).count(), 103);
        assert_eq!(PONIES.len(), 1154);
        for pony in PONIES {
            for message in ["Hello pony", "多謝你，世界！\nA fresh quote.", ""] {
                let rendered =
                    render(pony.art, message, 240).unwrap_or_else(|e| panic!("{}: {e}", pony.name));
                assert!(!rendered.contains("$balloon"), "{}", pony.name);
                assert!(rendered.ends_with("\x1b[0m\n"));
                assert!(
                    strip_ansi(&rendered).contains(message.lines().next().unwrap_or("")),
                    "{}",
                    pony.name
                );
            }
        }
    }
    #[test]
    fn embedded_balloon_tty_layout_matches_regular_art() {
        for (name, extra) in [("twilightrage", false), ("velvetremedy", true)] {
            let layout = |tty| {
                let pony = PONIES
                    .iter()
                    .find(|p| p.name == name && p.extra == extra && p.tty == tty)
                    .unwrap();
                strip_ansi(&render(pony.art, "Hi", 240).unwrap())
                    .lines()
                    .map(str::trim_end)
                    .collect::<Vec<_>>()
                    .join("\n")
                    .trim_end()
                    .to_string()
            };
            assert_eq!(layout(false), layout(true), "{name}");
        }
    }
    #[test]
    fn colour_zero_is_not_a_reset_inside_extended_colour() {
        let rendered = render("$balloon$\n\x1b[48;5;128m\x1b[38;5;0m$\\$X", "Hi", 80).unwrap();
        assert!(rendered.contains("\x1b[0;38;5;0;48;5;128mX"));
    }
    #[test]
    fn unicode_and_ansi_width() {
        assert_eq!(display_width("\x1b[31m中e\u{301}\x1b[0m"), 3);
        let rendered = render("$balloon$\n  $\\$\npony", "你好\nworld", 80).unwrap();
        let plain = strip_ansi(&rendered);
        let rows: Vec<_> = plain.lines().take(4).map(display_width).collect();
        assert!(rows.iter().all(|w| *w == rows[0]));
    }
    #[test]
    fn metadata_hidden_and_controls_removed() {
        let rendered = render(
            "$$$\nARTIST: Test\n$$$\n$balloon$\npony",
            "\x1b[31mhello\x1b[0m\x07",
            80,
        )
        .unwrap();
        assert!(!rendered.contains("ARTIST"));
        assert!(!rendered.contains("\x1b[31m"));
        assert!(rendered.contains("hello"));
    }
    #[test]
    fn narrow_width_and_long_words() {
        let rendered = render("$balloon5$\npony", "abcdefghijk 中國香港", 12).unwrap();
        for row in strip_ansi(&rendered).lines().take(6) {
            assert!(display_width(row) <= 12);
        }
    }
}
